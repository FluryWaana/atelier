import test from 'node:test';
import assert from 'node:assert/strict';
import { createConnectors } from '../lib/application/connectors.mjs';

test('repository diagnostics follow the selected project and report a missing module without fallback', async () => {
  const checked = [];
  const module = id => ({ id, name: id, apiVersion: 1, capabilities: { repositories: Object.fromEntries(['preview', 'publish', 'refresh', 'review'].map(operation => [operation, () => ({})])) }, check(_context, input) { checked.push([id, input.repo]); return 'OK'; } });
  const connectors = createConnectors({
    modules: [module('general'), module('specific')],
    settings: () => ({ connectorProviders: { repositories: 'general', tickets: '', time: '' } }),
    getSecret: () => '', context: () => ({}),
    repositoryProviderFor: repo => repo === 'project' ? 'specific' : repo === 'removed' ? 'missing' : '',
  });
  assert.equal((await connectors.checkConnections({ repo: 'project' }))[0].name, 'specific');
  assert.equal((await connectors.checkConnections())[0].name, 'general');
  const missing = (await connectors.checkConnections({ repo: 'removed' }))[0];
  assert.equal(missing.name, 'missing'); assert.equal(missing.ok, false);
  assert.deepEqual(checked, [['specific', 'project'], ['general', undefined]]);
});
