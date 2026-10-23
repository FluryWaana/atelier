import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { openStore } from '../lib/store.mjs';
import { createBackups } from '../lib/backups.mjs';
import { migrationShape, seedMigrationData } from './fixtures/migration-data.mjs';
import { migrationInventory } from '../scripts/rust-migration-inventory.mjs';

test('migration fixture survives reopen and WAL backup with unknown fields, null, ordering and log cursors', t => {
  const directory = mkdtempSync(join(tmpdir(), 'atelier-migration-reference-'));
  const path = join(directory, 'reference.sqlite');
  let store = openStore(path), saved;
  t.after(() => { saved?.db.close(); store.db.close(); rmSync(directory, { recursive: true, force: true }); });
  seedMigrationData(store);
  const cards = store.cards();
  assert.equal(cards.length, migrationShape.cards);
  assert.equal(cards[0].id, 'card-499');
  assert.equal(Object.hasOwn(store.card('card-0'), 'nullable'), false);
  assert.equal(store.card('card-1').nullable, null);
  const first = store.logs('run-0');
  assert.equal(first.length, 500);
  assert.equal(store.logs('run-0', first.at(-1).seq)[0].seq, first.at(-1).seq + 1);
  const backup = createBackups(store, directory).create();
  saved = openStore(join(backup.directory, backup.name));
  assert.deepEqual(saved.cards(), cards);
  assert.deepEqual(saved.times(), store.times());
  assert.deepEqual(saved.settings(), store.settings());
  const changed = { ...store.card('card-0'), title: 'Modification synthétique' };
  store.putCard(changed);
  assert.equal(store.cards()[0].id, cards[0].id, 'Modifier une ancienne carte ne doit pas modifier son ordre de lecture.');
  assert.equal(store.cards().at(-1).id, changed.id);
  store.db.close(); store = openStore(path);
  assert.deepEqual(store.card(changed.id), changed);
  assert.deepEqual(store.card('card-1'), saved.card('card-1'));
  assert.equal(saved.db.prepare('SELECT COUNT(*) AS count FROM logs').get().count, migrationShape.logs);
  assert.equal(store.projects().length, migrationShape.projects);
  assert.equal(store.runs().length, migrationShape.runs);
});

test('migration fixture creation rolls back all documents after a write failure', () => {
  const store = openStore(':memory:');
  try {
    assert.throws(() => seedMigrationData({ ...store, putTime() { throw new Error('simulated full disk'); } }), /simulated full disk/);
    assert.deepEqual(store.cards(), []);
    assert.deepEqual(store.settings(), {});
    assert.deepEqual(store.projects(), []);
    assert.deepEqual(store.runs(), []);
  } finally { store.db.close(); }
});

test('migration inventory includes compatibility exports, native helpers, test supports and dynamic route tables', () => {
  const inventory = migrationInventory();
  for (const name of ['lib/infrastructure/windows-updater.mjs', 'scripts/WindowsTray.cs', 'test/linux-tray-check.py', 'test/fixtures/migration-agent.mjs', 'lib/http/card-workflow-routes.mjs']) assert.ok(inventory.includes(name), name);
  assert.match(inventory, /atelier-session/);
  assert.match(inventory, /Environnement et arguments CLI/);
  assert.match(inventory, /brainstorm-draft/);
});

test('migration fake CLI emits separate UTF-8 protocol and diagnostic streams', () => {
  const output = spawnSync(process.execPath, [fileURLToPath(new URL('./fixtures/migration-agent.mjs', import.meta.url))], { encoding: 'utf8', windowsHide: true, timeout: 5000 });
  assert.equal(output.status, 0, output.stderr);
  assert.equal(output.stderr, 'Diagnostic synthétique\n');
  assert.deepEqual(JSON.parse(output.stdout), { type: 'result', result: 'Analyse synthétique terminée — é 🔧' });
});
