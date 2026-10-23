import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { openTokenVault, systemProtect, windowsProtect } from '../../lib/token-vault.mjs';

const [mode, database, reference] = process.argv.slice(2);
const tokens = { jira: 'synthetic-jira-é-🔑', azure: '', tempo: 'synthetic-tempo', extra: null };
const execute = (_command, _args, options) => {
  assert.equal(options.input, undefined, 'The fixed synthetic key must already exist.');
  return { status: 0, stdout: '11'.repeat(32) };
};
if (mode === 'seed') {
  const envelopes = {};
  for (const platform of ['linux', 'darwin']) envelopes[platform] = systemProtect({ platform, execute })(JSON.stringify(tokens));
  if (process.platform === 'win32') {
    const db = new DatabaseSync(database);
    try { openTokenVault(db, windowsProtect).save(tokens); }
    finally { db.close(); }
  }
  writeFileSync(reference, JSON.stringify({ tokens, envelopes }));
} else if (mode === 'verify') {
  const actual = JSON.parse(readFileSync(reference, 'utf8'));
  for (const platform of ['linux', 'darwin']) assert.deepEqual(JSON.parse(systemProtect({ platform, execute })(actual.envelopes[platform], true)), tokens);
  if (process.platform === 'win32') {
    const db = new DatabaseSync(database);
    try { assert.deepEqual(openTokenVault(db, windowsProtect).load(), tokens); }
    finally { db.close(); }
  }
} else throw new Error('Unknown vault fixture mode.');
