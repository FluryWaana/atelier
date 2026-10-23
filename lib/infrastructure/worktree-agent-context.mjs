import { createHash } from 'node:crypto';
import { appendFileSync, chmodSync, existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, sep } from 'node:path';

const contextFile = name => !name.split('/').some(part => ['..', '.git'].includes(part)) && /^(?:(?:CLAUDE(?:\.local)?|AGENTS(?:\.override)?)\.md|\.mcp\.json|\.claude\/(?:settings(?:\.local)?\.json|(?:CLAUDE|AGENTS)\.md|(?:skills|agents|commands|rules|hooks|plugins)\/.+))$/.test(name);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const inside = (root, path) => { const rel = relative(root, path); return !isAbsolute(rel) && rel !== '..' && !rel.startsWith('..' + sep); };

// Keep local agent configuration available without overwriting task edits or publishing private configuration.
export async function syncWorktreeAgentContext(source, worktree, git) {
  const root = realpathSync(worktree);
  const gitDir = await git(worktree, ['rev-parse', '--absolute-git-dir']);
  const manifest = join(gitDir, 'atelier-agent-context.json');
  const previous = existsSync(manifest) ? JSON.parse(readFileSync(manifest, 'utf8')) : {};
  const tracked = new Set((await git(worktree, ['ls-files', '-z'])).split('\0'));
  const names = [...new Set((await git(source, ['ls-files', '--cached', '--others', '-z', '--', 'CLAUDE.md', 'CLAUDE.local.md', 'AGENTS.md', 'AGENTS.override.md', '.mcp.json', '.claude'])).split('\0').filter(contextFile))];
  if (names.length > 2000) throw new Error('Configuration agent trop volumineuse (2 000 fichiers maximum).');
  const files = new Map(); let size = 0;
  for (const name of names) {
    const path = join(source, name);
    if (!existsSync(path)) continue;
    const stat = lstatSync(path);
    if (!stat.isFile() || !inside(realpathSync(source), realpathSync(path))) throw new Error(`Fichier de configuration agent non régulier : ${name}`);
    size += stat.size;
    if (stat.size > 1024 * 1024 || size > 8 * 1024 * 1024) throw new Error('Configuration agent trop volumineuse (1 Mio par fichier, 8 Mio au total).');
    files.set(name, { bytes: readFileSync(path), mode: stat.mode & 0o100 ? 0o700 : 0o600 });
  }
  const next = {}, copies = [];
  for (const name of new Set([...files.keys(), ...Object.keys(previous)])) {
    if (!contextFile(name) || tracked.has(name)) continue;
    const destination = join(root, name);
    let parent = dirname(destination);
    while (!existsSync(parent)) parent = dirname(parent);
    if (!inside(root, realpathSync(parent))) throw new Error('Configuration agent hors du worktree.');
    let current;
    try {
      const stat = lstatSync(destination);
      if (!stat.isFile() || stat.nlink !== 1) throw new Error(`Configuration agent remplacée par un lien ou un dossier : ${name}`);
      current = digest(readFileSync(destination));
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (current && current !== previous[name]) continue;
    if (files.has(name)) { copies.push([name, destination, files.get(name)]); next[name] = digest(files.get(name).bytes); }
    else if (current) unlinkSync(destination);
  }
  if (copies.length) {
    const exclude = await git(worktree, ['rev-parse', '--path-format=absolute', '--git-path', 'info/exclude']);
    mkdirSync(dirname(exclude), { recursive: true });
    const rules = existsSync(exclude) ? readFileSync(exclude, 'utf8').split(/\r?\n/) : [];
    const missing = copies.map(([name]) => '/' + name.replace(/[\\ *?\[\]#!]/g, '\\$&')).filter(rule => !rules.includes(rule));
    if (missing.length) appendFileSync(exclude, '\n' + missing.join('\n') + '\n');
    for (const [, destination, { bytes, mode }] of copies) {
      mkdirSync(dirname(destination), { recursive: true });
      writeFileSync(destination, bytes, { mode });
      chmodSync(destination, mode);
    }
  }
  writeFileSync(manifest + '.tmp', JSON.stringify(next), { mode: 0o600 });
  renameSync(manifest + '.tmp', manifest);
}
