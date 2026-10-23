import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { openStore } from '../../lib/store.mjs';
import { createBackups } from '../../lib/backups.mjs';
import { seedMigrationData } from './migration-data.mjs';
import { snapshot, assertSnapshot } from './rust-storage-reference.mjs';

const [mode, path, reference] = process.argv.slice(2);
assert.ok(['seed', 'verify', 'rescue'].includes(mode) && path && reference, 'Usage: seed|verify|rescue DATABASE REFERENCE_JSON');
const store = openStore(path);
try {
  if (mode === 'seed') {
    seedMigrationData(store);
    store.putCard({ ...store.card('card-0'), status: 'review', automation: { state: 'ready', step: 'review', extra: null } });
    store.putRun({ ...store.run('run-0'), kind: 'pr-review' });
    store.putTime({ id: 'exported', exportState: 'exported', tempoId: 42 });
    store.db.exec("CREATE TABLE token_vault(id INTEGER PRIMARY KEY,encrypted TEXT); INSERT INTO token_vault VALUES(1,'synthetic-only');");
    const backups = createBackups(store, dirname(path));
    const backup = backups.create();
    store.putCard({ id: 'newer', title: 'Donnée à sauver avant restauration', nullable: null });
    store.log('run-0', 'stdout', 'after backup');
    const rescue = snapshot(store);
    store.db.prepare('VACUUM INTO ?').run(path + '.rust.sqlite');
    backups.restore(backup.name);
    writeFileSync(reference, JSON.stringify({ backupName: backup.name, restored: snapshot(store), rescue }));
  } else {
    const expected = JSON.parse(readFileSync(reference, 'utf8'));
    assertSnapshot(snapshot(store), mode === 'rescue' ? expected.rescue : expected.restored);
    assert.equal(store.db.prepare('SELECT COUNT(*) AS count FROM token_vault').get().count, mode === 'rescue' ? 1 : 0);
  }
} finally { store.db.close(); }
