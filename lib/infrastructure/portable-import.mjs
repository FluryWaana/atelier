import { cpSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, renameSync, rmSync, rmdirSync, chmodSync } from 'node:fs';
import { dirname, join, relative, resolve, isAbsolute, sep } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { randomUUID } from 'node:crypto';
import { isDataLockFile } from './data-lock.mjs';

export function importPortableData(source, destination) {
  source = resolve(source); destination = resolve(destination);
  const inside = relative(source, destination);
  if (!inside || inside !== '..' && !inside.startsWith('..' + sep) && !isAbsolute(inside)) throw new Error('Le dossier de destination doit être distinct de la source.');
  if (existsSync(destination) && (lstatSync(destination).isSymbolicLink() || readdirSync(destination).length)) throw new Error('Des données existent déjà. L’import ne les écrasera pas.');
  if (!lstatSync(source).isDirectory() || lstatSync(source).isSymbolicLink()) throw new Error('Choisissez le dossier data de la version portable.');
  const database = join(source, 'kanban.sqlite');
  if (!lstatSync(database).isFile() || lstatSync(database).isSymbolicLink()) throw new Error('Base portable invalide.');
  mkdirSync(dirname(destination), { recursive: true, mode: 0o700 });
  const stage = mkdtempSync(join(dirname(destination), '.atelier-import-'));
  try {
    cpSync(source, stage, { recursive: true, filter: path => {
      if (path.startsWith(source + sep) && !relative(source, path).includes(sep) && isDataLockFile(relative(source, path))) return false;
      if (['local-session', 'local-browser.html'].some(name => path === join(source, name))) return false;
      if (lstatSync(path).isSymbolicLink()) throw new Error('Les liens symboliques sont interdits dans les données importées.');
      return !['', '-wal', '-shm'].some(suffix => path === database + suffix);
    } });
    chmodSync(stage, 0o700);
    const db = new DatabaseSync(database, { readOnly: true });
    try {
      if (db.prepare('PRAGMA quick_check').get().quick_check !== 'ok') throw new Error('La base portable est endommagée.');
      for (const table of ['cards', 'projects', 'settings']) db.prepare(`SELECT * FROM ${table} LIMIT 0`).all();
      // SQLite includes committed WAL changes; copying the database file alone can lose them.
      db.prepare('VACUUM INTO ?').run(join(stage, 'kanban.sqlite'));
    } finally { db.close(); }
    mkdirSync(join(stage, 'backups'), { recursive: true });
    copyFileSync(join(stage, 'kanban.sqlite'), join(stage, 'backups', `atelier-${Date.now()}-${randomUUID()}.sqlite`));
    if (existsSync(destination)) rmdirSync(destination);
    renameSync(stage, destination);
  } finally { if (existsSync(stage)) rmSync(stage, { recursive: true, force: true }); }
}
