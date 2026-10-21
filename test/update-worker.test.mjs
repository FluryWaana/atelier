import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { managedEntries } from '../lib/domain/release-package.mjs';
import { swapFiles, restoreFiles } from '../scripts/update-worker.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'atelier-swap-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const stage = join(root, '.updates', 'test'); mkdirSync(join(stage, 'payload'), { recursive: true });
  mkdirSync(join(root, 'data')); writeFileSync(join(root, 'data', 'keep'), 'personal');
  for (const name of managedEntries) { writeFileSync(join(root, name), 'old'); writeFileSync(join(stage, 'payload', name), 'new'); }
  return { root, stage };
}
test('replacement touches only application entries and rollback restores the old files', t => {
  const { root, stage } = fixture(t);
  swapFiles(root, stage);
  for (const name of managedEntries) assert.equal(readFileSync(join(root, name), 'utf8'), 'new');
  assert.equal(readFileSync(join(root, 'data', 'keep'), 'utf8'), 'personal');
  restoreFiles(root, stage);
  for (const name of managedEntries) assert.equal(readFileSync(join(root, name), 'utf8'), 'old');
});
test('an interrupted replacement restores every moved entry and removes newly introduced entries', t => {
  const { root, stage } = fixture(t);
  rmSync(join(root, 'release.json')); rmSync(join(stage, 'payload', 'README.md'));
  assert.throws(() => swapFiles(root, stage));
  restoreFiles(root, stage);
  assert.equal(existsSync(join(root, 'release.json')), false);
  for (const name of managedEntries.filter(name => name !== 'release.json')) assert.equal(readFileSync(join(root, name), 'utf8'), 'old');
});

test('rollback after startup restores the database snapshot, clears WAL sidecars and can be resumed', t => {
  const { root, stage } = fixture(t);
  const transaction = swapFiles(root, stage); transaction.started = true;
  writeFileSync(join(stage, 'transaction.json'), JSON.stringify(transaction));
  const backupName = 'atelier-1234567890123-12345678-1234-1234-1234-123456789012.sqlite';
  writeFileSync(join(stage, 'config.json'), JSON.stringify({ backupName }));
  mkdirSync(join(root, 'data', 'backups'));
  writeFileSync(join(root, 'data', 'backups', backupName), 'saved database');
  for (const suffix of ['', '-wal', '-shm']) writeFileSync(join(root, 'data', 'kanban.sqlite' + suffix), 'new data');
  restoreFiles(root, stage); restoreFiles(root, stage);
  assert.equal(readFileSync(join(root, 'data', 'kanban.sqlite'), 'utf8'), 'saved database');
  assert.equal(existsSync(join(root, 'data', 'kanban.sqlite-wal')), false);
  assert.equal(existsSync(join(root, 'data', 'kanban.sqlite-shm')), false);
  assert.equal(readFileSync(join(stage, 'failed', 'kanban.sqlite'), 'utf8'), 'new data');
});
