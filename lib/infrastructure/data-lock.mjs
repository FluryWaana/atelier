import { closeSync, lstatSync, mkdirSync, openSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export const dataLockName = '.atelier-data-lock.sqlite';
export function isDataLockFile(name) { return ['', '-journal', '-wal', '-shm'].some(suffix => name === dataLockName + suffix); }

// N1 acquires this before opening kanban.sqlite and closes it only after that database.
export function acquireDataLock(directory) {
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  directory = realpathSync(directory);
  const path = join(directory, dataLockName);
  try { closeSync(openSync(path, 'wx', 0o600)); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Verrou de données invalide.');
  // Never read this inode through another descriptor: POSIX close can release SQLite's locks.
  const db = new DatabaseSync(path);
  try { db.exec('PRAGMA busy_timeout=0; BEGIN EXCLUSIVE'); }
  catch (cause) { db.close(); throw new Error('Ce dossier est déjà ouvert ou son verrou de données est invalide.', { cause }); }
  return { directory, close: () => db.close() };
}
