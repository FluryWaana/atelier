import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { openStore } from '../../lib/store.mjs';
import { createTasks } from '../../lib/application/tasks.mjs';
import { createBackups } from '../../lib/backups.mjs';
import { snapshot, assertSnapshot } from './rust-storage-reference.mjs';

const [mode, path, reference] = process.argv.slice(2);
assert.ok(['seed', 'roundtrip', 'restored'].includes(mode) && path && reference);
const store = openStore(path);
try {
  if (mode === 'seed') {
    const extra = { '\ud800': ['\udc00', 'é 🔧', null], absentIsDifferent: null };
    store.putProject({ id: 'project', name: '\ud800', extra });
    store.putCard({ id: 'card', title: 'a'.repeat(291) + '🔧', status: 'review', automation: { state: 'ready', '\udc00': '\ud800' }, extra });
    const copied = createTasks({ store, newId: () => 'copy', timestamp: () => '2026-10-24T00:00:00.000Z' }).duplicate('card');
    assert.equal(copied.title.charCodeAt(299), 0xd83d);
    store.putRun({ id: 'run', kind: 'pr-review', status: 'completed', summary: '\ud800', extra });
    store.putTime({ id: 'time', exportState: 'draft', description: '\udc00', extra });
    store.saveSettings({ extra, '\udc00': '\ud800' });
    const before = snapshot(store), backups = createBackups(store, dirname(path)), backup = backups.create();
    store.db.prepare('VACUUM INTO ?').run(path + '.rust.sqlite');
    backups.restore(backup.name);
    writeFileSync(reference, JSON.stringify({ before, restored: snapshot(store), backupName: backup.name }));
  } else {
    const expected = JSON.parse(readFileSync(reference, 'utf8'));
    if (mode === 'roundtrip') {
      for (const table of ['projects', 'cards', 'runs', 'times']) for (const document of expected.before[table]) Object.assign(document, { rustRoundTrip: true, rustUnits: '\ud800a\udc00' });
      Object.assign(expected.before.settings, { rustRoundTrip: true, rustUnits: '\ud800a\udc00' });
      assertSnapshot(snapshot(store), expected.before);
    } else assertSnapshot(snapshot(store), expected.restored);
  }
} finally { store.db.close(); }
