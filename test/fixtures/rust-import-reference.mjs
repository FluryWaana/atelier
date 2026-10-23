import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { openStore } from '../../lib/store.mjs';
import { importPortableData } from '../../lib/infrastructure/portable-import.mjs';
import { acquireDataLock, dataLockName } from '../../lib/infrastructure/data-lock.mjs';
import { seedMigrationData } from './migration-data.mjs';
import { snapshot, assertSnapshot } from './rust-storage-reference.mjs';

const [mode, directory, reference] = process.argv.slice(2);
if (mode === 'seed') {
  const source = join(directory, 'source'), destination = join(directory, 'node');
  mkdirSync(source, { recursive: true });
  const lock = acquireDataLock(source);
  const store = openStore(join(source, 'kanban.sqlite'));
  try {
    seedMigrationData(store);
    mkdirSync(join(source, 'documents'));
    writeFileSync(join(source, 'documents', 'pièce.txt'), 'À conserver 🔑');
    writeFileSync(join(source, 'local-session'), 'synthetic-expired');
    writeFileSync(join(source, 'local-browser.html'), 'synthetic-expired');
    importPortableData(source, destination);
    const imported = openStore(join(destination, 'kanban.sqlite'));
    try { writeFileSync(reference, JSON.stringify(snapshot(imported))); }
    finally { imported.db.close(); }
  } finally { store.db.close(); lock.close(); }
} else if (mode === 'verify') {
  const store = openStore(join(directory, 'kanban.sqlite'));
  try { assertSnapshot(snapshot(store), JSON.parse(readFileSync(reference, 'utf8'))); }
  finally { store.db.close(); }
  assert.equal(readFileSync(join(directory, 'documents', 'pièce.txt'), 'utf8'), 'À conserver 🔑');
  for (const name of ['local-session', 'local-browser.html', dataLockName]) assert.equal(existsSync(join(directory, name)), false);
  const names = readdirSync(join(directory, 'backups'));
  assert.equal(names.length, 1);
  assert.match(names[0], /^atelier-\d{13}-[a-f0-9-]{36}\.sqlite$/);
  const saved = openStore(join(directory, 'backups', names[0]));
  try { assertSnapshot(snapshot(saved), JSON.parse(readFileSync(reference, 'utf8'))); }
  finally { saved.db.close(); }
} else throw new Error('Unknown import fixture mode.');
