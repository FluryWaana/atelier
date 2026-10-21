import test from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { openTokenVault } from '../lib/token-vault.mjs';
import { createSettings } from '../lib/application/settings.mjs';
import { transaction } from '../lib/infrastructure/transaction.mjs';

test('failed settings persistence rolls back encrypted tokens and leaves live credentials unchanged', () => {
  const db = new DatabaseSync(':memory:');
  try {
    const vault = openTokenVault(db, (value, decrypt) => decrypt ? value.slice(7) : 'opaque:' + value);
    vault.save({ jira: 'old' });
    let fail = true;
    const service = createSettings({ vault, platform: 'linux', initialTokens: { jira: 'old' }, repository: value => value, jiraBase: value => value,
      store: { settings: () => ({}), saveSettings() { if (fail) throw new Error('disk error'); }, transaction: change => transaction(db, change) },
    });
    const body = { jiraType: 'cloud', jiraUrl: '', jiraEmail: '', jql: '', token: 'new' };
    assert.throws(() => service.update(body), /disk error/);
    assert.equal(service.token('jira'), 'old'); assert.equal(vault.load().jira, 'old');
    fail = false; service.update(body);
    assert.equal(service.token('jira'), 'new'); assert.equal(vault.load().jira, 'new');
    assert.equal(JSON.stringify(service.publicConfig()).includes('new'), false);
    service.removeToken('jira'); assert.equal(service.token('jira'), '');
    assert.throws(() => service.saveSecrets({ 'connector:example:token': 'private' }, () => { throw new Error('disk error'); }), /disk error/);
    assert.equal(service.token('connector:example:token'), '');
    assert.equal(vault.load()['connector:example:token'], undefined);
  } finally { db.close(); }
});
