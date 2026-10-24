import { constants } from 'node:fs';
import { lstat, open, realpath } from 'node:fs/promises';
import { isAbsolute, join, relative, sep } from 'node:path';
import { runCommand } from '../pull-requests.mjs';
import { sameRepository } from '../domain/paths.mjs';

const fileLimit = 512 * 1024;
const git = (repo, args) => runCommand('git', ['--no-optional-locks', '--literal-pathspecs', '-c', 'core.fsmonitor=false', '-c', 'diff.submodule=short', '-C', repo, ...args], repo);
async function inspectionGit(root) {
  const keys = (await git(root, ['config', '--includes', '--null', '--name-only', '--list'])).split('\0');
  const options = [];
  // Diff can invoke clean/process filters even with --no-ext-diff and --no-textconv.
  for (const key of new Set(keys.filter(key => /^filter\.[\s\S]+\.(clean|smudge|process|required)$/.test(key)))) {
    if (key.includes('=')) throw new Error('Configuration de filtre Git incompatible avec l’aperçu sécurisé.');
    options.push('-c', key + '=' + (key.endsWith('.required') ? 'false' : ''));
  }
  return args => git(root, [...options, ...args]);
}
async function validateWorktree(card, root) {
  if (!card.worktree.source) throw new Error('Le dépôt source ne correspond plus au worktree.');
  const source = await realpath(card.repo || card.worktree.source);
  if (!sameRepository(source, await realpath(card.worktree.source), process.platform)) throw new Error('Le dépôt source ne correspond plus au worktree.');
  const commonArgs = ['rev-parse', '--path-format=absolute', '--git-common-dir'];
  const [sourceCommon, taskCommon, branch] = await Promise.all([
    git(source, commonArgs).then(path => realpath(path.trim())),
    git(root, commonArgs).then(path => realpath(path.trim())),
    git(root, ['rev-parse', '--symbolic-full-name', 'HEAD']),
  ]);
  if (!sameRepository(sourceCommon, taskCommon, process.platform)) throw new Error('Le dépôt source ne correspond plus au worktree.');
  if (branch !== 'refs/heads/' + card.worktree.branch) throw new Error('Le worktree associé a disparu ou sa branche a changé. Vérifie-le dans Git avant de relancer.');
}
function validPath(path) {
  if (typeof path !== 'string' || !path || path.length > 4096 || /[\\\0:]/.test(path) || isAbsolute(path) || path.split('/').some(part => !part || part === '.' || part === '..' || part.toLowerCase() === '.git')) throw new Error('Chemin de fichier invalide.');
  return path;
}
async function readText(root, path) {
  let target = root;
  for (const part of validPath(path).split('/')) {
    target = join(target, part);
    if ((await lstat(target)).isSymbolicLink()) throw new Error('Les liens symboliques ne sont pas ouverts.');
  }
  const resolved = await realpath(target), rel = relative(root, resolved);
  if (isAbsolute(rel) || rel === '..' || rel.startsWith('..' + sep)) throw new Error('Fichier hors du projet.');
  const expected = await lstat(resolved, { bigint: true });
  if (!expected.isFile()) throw new Error('Ce chemin ne désigne pas un fichier ordinaire.');
  // codeql[js/file-system-race] Check the descriptor's inode/type before reading; no-follow and nonblocking open avoid link/FIFO traps.
  const file = await open(resolved, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
  try {
    const stat = await file.stat({ bigint: true });
    if (stat.dev !== expected.dev || stat.ino !== expected.ino || stat.nlink !== expected.nlink) throw new Error('Le fichier a changé pendant sa lecture.');
    if (!stat.isFile()) throw new Error('Ce chemin ne désigne pas un fichier ordinaire.');
    if (stat.size > fileLimit) return { unavailable: 'Fichier trop volumineux pour l’aperçu (512 Kio maximum).' };
    const buffer = Buffer.alloc(fileLimit + 1);
    let size = 0;
    while (size < buffer.length) {
      const { bytesRead } = await file.read(buffer, size, buffer.length - size, null);
      if (!bytesRead) break;
      size += bytesRead;
    }
    if (size > fileLimit) return { unavailable: 'Fichier trop volumineux pour l’aperçu (512 Kio maximum).' };
    const bytes = buffer.subarray(0, size);
    if (bytes.includes(0)) return { unavailable: 'Fichier binaire : aperçu textuel indisponible.' };
    try { return { content: new TextDecoder('utf-8', { fatal: true }).decode(bytes) }; }
    catch { return { unavailable: 'Encodage non UTF-8 : aperçu textuel indisponible.' }; }
  } finally { await file.close(); }
}
async function changes(git, worktree) {
  if (!worktree?.baseCommit) throw new Error('Prépare le worktree pour comparer les changements de cette tâche.');
  if (!/^[a-f0-9]{40,64}$/i.test(worktree.baseCommit)) throw new Error('Commit de départ invalide.');
  const base = (await git(['merge-base', worktree.baseCommit, 'HEAD'])).trim();
  const [tracked, untracked] = await Promise.all([
    git(['diff', '--no-ext-diff', '--no-textconv', '--no-renames', '--name-status', '-z', base, '--']),
    git(['ls-files', '--others', '--exclude-standard', '-z']),
  ]);
  const entries = tracked.split('\0'), files = [];
  for (let i = 0; i + 1 < entries.length; i += 2) files.push({ status: entries[i], path: entries[i + 1] });
  for (const path of untracked.split('\0').filter(Boolean)) files.push({ status: 'A', path, untracked: true });
  files.sort((a, b) => a.path.localeCompare(b.path));
  return { base, files };
}

export async function inspectTaskFiles(card, action, { path, untracked } = {}) {
  const directory = card.worktree?.path || card.repo;
  if (!directory) throw new Error('Choisis un dépôt Git pour consulter ses fichiers.');
  const root = await realpath(directory);
  const top = await realpath((await git(root, ['rev-parse', '--show-toplevel'])).trim());
  if (root !== top) throw new Error('Le dossier de travail ne correspond plus à la racine du dépôt.');
  if (card.worktree) await validateWorktree(card, root);
  const readGit = await inspectionGit(root);
  if (action === 'changes' || action === 'file-diff') {
    const result = await changes(readGit, card.worktree);
    if (action === 'changes') return { ...result, root, branch: card.worktree.branch };
    validPath(path);
    const entry = result.files.find(file => file.path === path && (untracked === undefined || Boolean(file.untracked) === (untracked === 'true')));
    if (!entry) throw new Error('Ce fichier ne fait plus partie des changements. Actualise la liste.');
    if (entry.untracked) {
      const file = await readText(root, path);
      if (file.unavailable) return { path, ...file };
      const lines = file.content.split('\n');
      if (lines.at(-1) === '') lines.pop();
      return { path, patch: lines.length ? `@@ -0,0 +1,${lines.length} @@\n` + lines.map(line => '+' + line).join('\n') : '', empty: !lines.length };
    }
    const patch = await readGit(['diff', '--no-ext-diff', '--no-textconv', '--no-renames', '--no-color', '--unified=3', result.base, '--', path]);
    return { path, patch };
  }
  const files = [...new Set((await readGit(['ls-files', '--cached', '--others', '--exclude-standard', '-z'])).split('\0').filter(Boolean))].sort();
  if (action === 'files') return { root, worktree: Boolean(card.worktree), files };
  validPath(path);
  if (!files.includes(path)) throw new Error('Fichier absent du projet ou ignoré par Git.');
  return { path, ...await readText(root, path) };
}
