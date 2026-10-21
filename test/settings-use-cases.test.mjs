import test from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { openTokenVault } from '../lib/token-vault.mjs';
import { createSettings } from '../lib/application/settings.mjs';
import { transaction } from '../lib/infrastructure/transaction.mjs';
import { createConnectors } from '../lib/application/connectors.mjs';
import jira from '../lib/connectors/jira.mjs';

test('both Jira settings paths clear inherited mappings on instance changes and accept new mappings', () => {
  for (const form of ['partial', 'unchanged', 'new']) {
    let saved = { jiraUrl: 'https://old.example', jiraAutoTransition: true, jiraStatusMap: { doing: '10001' } };
    const service = createSettings({ store: { settings: () => saved, saveSettings: value => saved = value, transaction: fn => fn() }, vault: { save() {} }, initialTokens: {}, repository: x => x, jiraBase: x => x });
    service.update({ jiraUrl: 'https://new.example', ...(form === 'partial' ? {} : { jiraStatusMap: { doing: form === 'new' ? '20001' : '10001' } }) });
    assert.equal(saved.jiraAutoTransition, form === 'new');
    assert.equal(saved.jiraStatusMap.doing || '', form === 'new' ? '20001' : '');

    saved = { connectorSettings: { jira: { url: 'https://old.example', 'auto-transition': true, 'status-doing': '10001' } } };
    const connectors = createConnectors({ modules: [jira], settings: () => saved, saveSettings: value => saved = value, getSecret: () => '', saveSecrets: (_, save) => save(), context: () => ({}) });
    connectors.configure('jira', { url: 'https://new.example', ...(form === 'partial' ? {} : { 'status-doing': form === 'new' ? '20001' : '10001' }) });
    assert.equal(saved.connectorSettings.jira['auto-transition'], form === 'new');
    assert.equal(saved.connectorSettings.jira['status-doing'], form === 'new' ? '20001' : '');
  }
});

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
