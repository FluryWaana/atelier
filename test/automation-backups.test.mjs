import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore } from '../lib/store.mjs';
import { createBackups } from '../lib/backups.mjs';

test('restoring a backup invalidates the automatic validation and refuses unfinished workflows', () => {
  const directory = mkdtempSync(join(tmpdir(), 'automation-backup-'));
  const store = openStore(join(directory, 'kanban.sqlite')), backups = createBackups(store, directory);
  try {
    store.putCard({ id: 'card', status: 'review', automation: { state: 'ready', step: 'review' } });
    const saved = backups.create(); backups.restore(saved.name);
    assert.equal(store.card('card').automation.state, 'interrupted'); assert.equal(store.card('card').automation.step, 'tests');
    assert.equal(store.card('card').status, 'standby');
    store.putCard({ id: 'card', status: 'doing', automation: { state: 'running', step: 'tests' } });
    const unfinished = backups.create();
    assert.throws(() => backups.restore(unfinished.name), /inachevées/);
  } finally { store.db.close(); rmSync(directory, { recursive: true, force: true }); }
});
