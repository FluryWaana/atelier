import test from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openTokenVault, windowsProtect } from '../lib/token-vault.mjs';

test('Windows DPAPI protects tokens and restores them after reopening the database', { skip: process.platform !== 'win32' }, t => {
  try { windowsProtect('availability-check'); }
  catch (error) {
    if (error.cause?.code === 'EPERM') {
      t.skip('The execution sandbox blocks child processes; run this integration test outside the sandbox.');
      return;
    }
    throw error;
  }
  const dir = mkdtempSync(join(tmpdir(), 'kanban-vault-'));
  let db;
  try {
    const path = join(dir, 'test.sqlite');
    db = new DatabaseSync(path);
    const secrets = { jira: 'test-jira-é-🔑', azure: 'test-azure', tempo: 'test-tempo' };
    openTokenVault(db).save(secrets);
    const blob = db.prepare('SELECT encrypted FROM token_vault').get().encrypted;
    for (const value of Object.values(secrets)) assert.equal(blob.includes(value), false);
    db.close();
    db = new DatabaseSync(path);
    const vault = openTokenVault(db);
    assert.deepEqual(vault.load(), secrets);
    vault.save({ ...secrets, jira: '' });
    assert.equal(openTokenVault(db).load().jira, '');
    assert.equal(openTokenVault(db).load().azure, secrets.azure);
  } finally {
    db?.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test('empty vault needs no decryption and failed encryption leaves saved data untouched', () => {
  const db = new DatabaseSync(':memory:');
  try {
    let fail = false;
    const protect = (value, decrypt) => {
      if (fail) throw new Error('unavailable');
      return decrypt ? value.slice(7) : `opaque:${value}`;
    };
    const vault = openTokenVault(db, protect);
    assert.deepEqual(vault.load(), {});
    vault.save({ jira: 'previous' });
    fail = true;
    assert.throws(() => vault.save({ jira: 'replacement' }), /unavailable/);
    fail = false;
    assert.deepEqual(vault.load(), { jira: 'previous' });
  } finally { db.close(); }
});
