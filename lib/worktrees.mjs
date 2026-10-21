import { execFileSync } from 'node:child_process';
import { appendFileSync, closeSync, openSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, symlinkSync, unlinkSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { createHash } from 'node:crypto';

function git(repo, args) {
  try { return execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8', windowsHide: true, timeout: 30000, stdio: ['ignore', 'pipe', 'pipe'] }).trim(); }
  catch (error) { throw new Error(`Git : ${error.stderr?.toString().trim() || error.message}`); }
}
const hash = value => createHash('sha256').update(value).digest('hex').slice(0, 10);
const slug = value => value.normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 55).replace(/-$/, '') || 'tache';
const samePath = (a, b) => process.platform === 'win32' ? resolve(a).toLowerCase() === resolve(b).toLowerCase() : resolve(a) === resolve(b);

function sharedDependencies(repo, path) {
  const folders = new Set(['']);
  for (const file of git(repo, ['ls-files', '-z']).split('\0').filter(Boolean)) {
    let folder = dirname(file);
    while (folder !== '.') { folders.add(folder); folder = dirname(folder); }
  }
  const links = [];
  for (const folder of folders) for (const name of ['node_modules', 'packages']) {
    const entry = join(folder, name), source = join(repo, entry), destination = join(path, entry);
    if (!existsSync(source) || !existsSync(dirname(destination)) || existsSync(destination) || lstatSync(source).isSymbolicLink()) continue;
    if (!lstatSync(source).isDirectory()) continue;
    if (name === 'packages' && !readdirSync(source, { withFileTypes: true }).some(item => item.isDirectory() && readdirSync(join(source, item.name)).some(file => file.endsWith('.nupkg')))) continue;
    if (git(repo, ['ls-files', '--', entry])) continue;
    try { git(repo, ['check-ignore', '--', entry + '/']); } catch { continue; }
    if (!samePath(realpathSync(dirname(destination)), join(path, folder))) throw new Error('Dossier de dépendances hors du worktree.');
    links.push({ path: entry, target: realpathSync(source) });
  }
  // A trailing-slash ignore rule does not cover a Unix directory symlink.
  if (links.length) {
    const exclude = git(repo, ['rev-parse', '--path-format=absolute', '--git-path', 'info/exclude']);
    mkdirSync(dirname(exclude), { recursive: true });
    const fd = openSync(exclude, 'a+');
    try {
      const rules = readFileSync(fd, 'utf8').split(/\r?\n/);
      const missing = links.map(link => '/' + link.path.split(sep).join('/').replace(/[\\ *?\[\]#!]/g, '\\$&')).filter(rule => !rules.includes(rule));
      if (missing.length) appendFileSync(fd, '\n' + missing.join('\n') + '\n');
    } finally { closeSync(fd); }
  }
  const created = [];
  try {
    for (const link of links) { symlinkSync(link.target, join(path, link.path), process.platform === 'win32' ? 'junction' : 'dir'); created.push(link); }
  } catch (error) { for (const link of created) unlinkSync(join(path, link.path)); throw error; }
  return links;
}

export function defaultBranch(card) {
  return `feature/${card.ticket?.key || `LOCAL-${hash(card.id)}`}-${slug(card.title)}`;
}
function list(repo) {
  return git(repo, ['worktree', 'list', '--porcelain', '-z']).split('\0\0').filter(Boolean).map(block => {
    const fields = Object.fromEntries(block.split('\0').filter(Boolean).map(line => { const i = line.indexOf(' '); return i < 0 ? [line, true] : [line.slice(0, i), line.slice(i + 1)]; }));
    return { path: fields.worktree, branch: fields.branch?.replace(/^refs\/heads\//, ''), locked: fields.locked };
  });
}

export function prepareWorktree(card, settings) {
  if (!card.repo && !settings.repo) throw new Error('Choisis le dépôt Git source de cette carte.');
  const source = realpathSync(card.repo || settings.repo);
  const repo = realpathSync(git(source, ['rev-parse', '--show-toplevel']));
  const branch = card.branch || defaultBranch(card);
  if (branch.startsWith('-') || !branch || branch.length > 180) throw new Error('Nom de branche invalide.');
  git(repo, ['check-ref-format', '--branch', branch]);
  if (card.ticket && !branch.split(/[^A-Za-z0-9-]+/).some(part => part === card.ticket.key || part.startsWith(card.ticket.key + '-'))) throw new Error(`Le nom de branche doit contenir ${card.ticket.key}.`);
  const worktrees = list(repo);
  if (card.worktree) {
    const found = worktrees.find(w => samePath(w.path, card.worktree.path));
    if (!found || found.branch !== branch || !existsSync(found.path)) throw new Error('Le worktree associé a disparu ou sa branche a changé. Vérifie-le dans Git avant de relancer.');
    if (!samePath(repo, card.worktree.source)) throw new Error('Le dépôt source ne correspond plus au worktree.');
    return card.worktree;
  }
  if (worktrees.some(w => w.branch === branch)) throw new Error(`La branche ${branch} est déjà utilisée par un autre worktree.`);
  const base = card.baseBranch || settings.baseBranch || 'HEAD';
  if (base.startsWith('-') || base.length > 180) throw new Error('Référence de départ invalide.');
  // Fetch directly into FETCH_HEAD: the source checkout and local base branch stay untouched.
  const remoteBase = base.replace(/^refs\/remotes\/origin\//, '').replace(/^origin\//, '').replace(/^refs\/heads\//, '');
  git(repo, ['fetch', '--no-tags', '--', 'origin', remoteBase]);
  const commit = git(repo, ['rev-parse', '--verify', 'FETCH_HEAD^{commit}']);
  const common = git(repo, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
  const parent = join(dirname(repo), '.atelier-worktrees', `${basename(repo)}-${hash(common)}`);
  const path = join(parent, `${slug(branch)}-${hash(branch)}`);
  if (existsSync(path)) throw new Error(`Le dossier cible existe déjà : ${path}`);
  mkdirSync(parent, { recursive: true });
  const existing = git(repo, ['for-each-ref', '--format=%(refname)', `refs/heads/${branch}`]).split('\n').includes(`refs/heads/${branch}`);
  git(repo, ['worktree', 'add', ...(existing ? [] : ['-b', branch]), '--', path, existing ? branch : commit]);
  let links;
  try { links = sharedDependencies(repo, path); }
  catch (error) {
    try { git(repo, ['worktree', 'remove', '--', path]); }
    catch (cleanup) { throw new Error(`${error.message} Nettoyage impossible : ${cleanup.message} Dossier : ${path}`); }
    throw error;
  }
  return { path: realpathSync(path), source: repo, branch, base, baseCommit: commit, links };
}

export function prepareApplicationWorktrees(card, project, save) {
  for (const role of ['front', 'back']) {
    if (!project?.[role + 'Command'] || !project[role + 'Repo']) continue;
    const source = realpathSync(project[role + 'Repo']);
    if (samePath(source, card.worktree.source)) continue;
    const existing = card.serviceWorktrees?.[role] || Object.values(card.serviceWorktrees || {}).find(worktree => samePath(worktree.source, source));
    const worktree = prepareWorktree({ ...card, repo: source, branch: card.worktree.branch, baseBranch: 'HEAD', worktree: existing }, { baseBranch: 'HEAD' });
    card.serviceWorktrees = { ...card.serviceWorktrees, [role]: worktree };
    save(card);
  }
}

export function removeApplicationWorktrees(card, discardChanges, save) {
  for (const worktree of Object.values(card.serviceWorktrees || {})) {
    if (!Object.values(card.serviceWorktrees || {}).some(item => samePath(item.path, worktree.path))) continue;
    removeWorktree({ worktree }, discardChanges);
    card.serviceWorktrees = Object.fromEntries(Object.entries(card.serviceWorktrees).filter(([, item]) => !samePath(item.path, worktree.path)));
    save(card);
  }
}

export function removeWorktree(card, discardChanges = false) {
  if (!card.worktree) throw new Error('Aucun worktree associé.');
  const { source, path, branch } = card.worktree;
  const found = list(source).find(w => samePath(w.path, path));
  if (!found || found.branch !== branch || samePath(source, path)) throw new Error('Association Git incohérente : suppression refusée.');
  const links = (card.worktree.links || []).filter(link => {
    const destination = resolve(path, link.path), rel = relative(resolve(path), destination);
    if (!rel || rel.startsWith('..') || isAbsolute(rel)) throw new Error('Lien de dépendances hors du worktree.');
    let entry;
    try { entry = lstatSync(destination); } catch (error) { if (error.code === 'ENOENT') return false; throw error; }
    if (!samePath(realpathSync(dirname(destination)), dirname(destination))) throw new Error('Lien de dépendances hors du worktree.');
    if (!entry.isSymbolicLink() || !samePath(realpathSync(destination), link.target)) throw new Error(`Le lien ${link.path} a changé : suppression refusée.`);
    return true;
  });
  for (const link of links) unlinkSync(join(path, link.path));
  // Discard local files only after explicit confirmation; locked worktrees remain protected.
  try { git(source, ['worktree', 'remove', ...(discardChanges === true ? ['--force'] : []), '--', path]); }
  catch (error) {
    for (const link of links) symlinkSync(link.target, join(path, link.path), process.platform === 'win32' ? 'junction' : 'dir');
    throw error;
  }
}
