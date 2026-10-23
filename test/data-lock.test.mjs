import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { acquireDataLock, dataLockName } from '../lib/infrastructure/data-lock.mjs';

test('the N1 data lock excludes aliases before opening application storage and releases on close', t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-lock-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const lock = acquireDataLock(root);
  try {
    assert.throws(() => acquireDataLock(join(root, '.')), /déjà ouvert/);
    assert.throws(() => acquireDataLock(root), /déjà ouvert/);
    assert.equal(existsSync(join(root, 'kanban.sqlite')), false);
  } finally { lock.close(); }
  acquireDataLock(root).close();
  assert.equal(existsSync(join(root, dataLockName)), true);
  writeFileSync(join(root, dataLockName), 'corrupt');
  assert.throws(() => acquireDataLock(root), /invalide/);
});
