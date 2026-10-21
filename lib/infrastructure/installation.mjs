import { existsSync, readFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { homedir } from 'node:os';

export function installationMode(root) {
  const file = join(root, 'installation.json');
  if (!existsSync(file)) return 'portable';
  const { mode } = JSON.parse(readFileSync(file, 'utf8'));
  if (!['user', 'system'].includes(mode)) throw new Error('Mode d’installation invalide.');
  return mode;
}

export function dataDirectory(root, { platform = process.platform, env = process.env, home = homedir() } = {}) {
  if (installationMode(root) === 'portable') return join(root, 'data');
  if (platform === 'win32') return join(env.LOCALAPPDATA || join(home, 'AppData', 'Local'), 'AtelierKanban', 'data');
  if (platform === 'linux') return join(env.XDG_DATA_HOME && isAbsolute(env.XDG_DATA_HOME) ? env.XDG_DATA_HOME : join(home, '.local', 'share'), 'atelier-kanban');
  throw new Error('Installation native non prise en charge sur ce système.');
}
