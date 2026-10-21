import test from 'node:test';
import assert from 'node:assert/strict';
import { createConnectors } from '../lib/application/connectors.mjs';
import { validateConnector } from '../lib/domain/connectors.mjs';
import { connectorRoute } from '../lib/http/connector-routes.mjs';
import jira from '../lib/connectors/jira.mjs';
import azure from '../lib/connectors/azure.mjs';
import tempo from '../lib/connectors/tempo.mjs';
import { createConnectorTime } from '../lib/application/connector-time.mjs';

function fixture() {
  let settings = {}, secrets = {};
  const example = { id: 'example', name: 'Example', apiVersion: 1,
    fields: [{ id: 'url', label: 'URL', type: 'url' }, { id: 'token', label: 'Token', type: 'password' }],
    capabilities: { tickets: { sync: context => ({ count: 3, url: context.settings.url, authenticated: Boolean(context.secret('token')) }), reset: () => ({ count: 0 }) } },
    tabs: [{ id: 'status', label: 'État', actions: [{ id: 'check', label: 'Vérifier' }] }],
    actions: { check: context => context.settings.url },
  };
  const registry = createConnectors({ modules: [jira, azure, tempo, example], settings: () => settings, saveSettings: value => { settings = value; }, getSecret: key => secrets[key],
    saveSecrets: (values, save = () => {}) => { save(); secrets = { ...secrets, ...values }; }, context: () => ({}),
  });
  return registry;
}

test('invalid and duplicate modules are reported while valid providers keep working', async () => {
  const errors = [{ id: 'broken-file', message: 'Erreur d’import' }];
  const healthy = { id: 'healthy', name: 'Healthy', apiVersion: 1, capabilities: { tickets: { sync: () => ({ count: 1 }), reset() {} } } };
  const registry = createConnectors({ modules: [null, { id: 'invalid', name: 'Invalid', apiVersion: 99 }, healthy, { ...healthy, name: 'Duplicate' }], errors,
    settings: () => ({ connectorProviders: { tickets: 'healthy' } }), getSecret: () => '', context: () => ({}),
    pullRequestProviders: () => ['github', 'github', 'azure'],
  });
  const catalog = registry.describe();
  assert.equal(catalog.errors.length, 4);
  assert.equal(errors.length, 1);
  assert.deepEqual(catalog.modules.map(module => module.name), ['Healthy']);
  assert.deepEqual(catalog.pullRequestProviders, ['github', 'azure']);
  assert.deepEqual(await registry.invoke('tickets', 'sync', {}), { count: 1 });
  await assert.rejects(registry.invoke('tickets', 'sync', {}, 'invalid'), /introuvable/);
});

test('third-party connector configures, supplies a capability and adds a working tab', async () => {
  const registry = fixture();
  registry.configure('example', { url: 'https://tickets.example', token: 'private-token' });
  registry.select({ tickets: 'example' });
  assert.deepEqual(await registry.invoke('tickets', 'sync', {}), { count: 3, url: 'https://tickets.example', authenticated: true });
  assert.equal(await connectorRoute(registry, 'POST', '/api/connectors/example/actions/check', {}), 'https://tickets.example');
  assert.equal(registry.describe().modules.at(-1).tabs[0].id, 'status');
  assert.equal(JSON.stringify(registry.describe()).includes('private-token'), false);
  registry.configure('example', { token: '' });
  assert.equal(registry.describe().modules.at(-1).secrets.token, true);
  registry.forget('example', 'token');
  assert.equal(registry.describe().modules.at(-1).secrets.token, false);
});

test('rejects unsupported contracts, partial capabilities and invalid settings', async () => {
  assert.throws(() => validateConnector({ id: 'x', name: 'X', apiVersion: 2 }), /version/);
  assert.throws(() => validateConnector({ id: 'x', name: 'X', apiVersion: 1, capabilities: { tickets: { sync() {} } } }), /incomplète/);
  const registry = fixture();
  assert.throws(() => registry.select({ repositories: 'example' }), /capacité/);
  assert.throws(() => registry.select({ tickets: 'missing' }), /introuvable/);
  assert.throws(() => registry.configure('example', { unknown: 'value' }), /inconnu/);
  assert.throws(() => registry.configure('example', { url: 'javascript:alert(1)' }), /Adresse/);
  assert.throws(() => registry.configure('jira', { type: 'invalid' }), /Choix/);
  await assert.rejects(registry.action('example', 'missing', {}), /inconnue/);
  assert.equal(connectorRoute(registry, 'GET', '/api/connectors/example/actions/check', {}), undefined);
});

test('time export requires confirmation, prevents duplicate sends and reconciles with the original provider', async () => {
  let entry = { id: 'entry', exportState: 'draft' }, provider = 'first';
  const calls = [];
  const service = createConnectorTime({ store: { time: () => entry, putTime: value => { entry = value; } }, changed() {},
    connectors: { selected: () => provider, async invoke(capability, operation, input, id) {
      calls.push({ operation, id });
      if (operation === 'send') { entry = { ...entry, exportState: 'sending' }; throw new Error('Réponse distante perdue'); }
      entry = { ...entry, exportState: 'exported' }; return entry;
    } },
  });
  await assert.rejects(service.execute('send', 'entry'), /Confirmez/);
  assert.equal(calls.length, 0);
  await assert.rejects(service.execute('send', 'entry', { confirmed: true }), /perdue/);
  assert.equal(entry.exportState, 'uncertain');
  provider = 'second';
  await assert.rejects(service.execute('send', 'entry', { confirmed: true }), /Confirmez/);
  await service.execute('reconcile', 'entry');
  assert.deepEqual(calls, [{ operation: 'send', id: 'first' }, { operation: 'reconcile', id: 'first' }]);
  await service.execute('send', 'entry', { confirmed: true }); assert.equal(calls.length, 2);
});
