import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { connectorSupports, repositoryProvider } from '../public/connector-availability.mjs';

test('options require an installed provider and a supported operation', () => {
  const catalog = { providers: { tickets: 'demo' }, modules: [{ id: 'demo', operations: { tickets: ['sync'] } }] };
  assert.equal(connectorSupports(catalog, 'tickets', 'sync'), true);
  assert.equal(connectorSupports(catalog, 'tickets', 'create'), false);
  assert.equal(connectorSupports(catalog, 'time', 'send'), false);
  assert.equal(connectorSupports(undefined, 'tickets', 'sync'), false);
  catalog.providers.tickets = '';
  assert.equal(connectorSupports(catalog, 'tickets', 'sync'), false);
  assert.equal(connectorSupports(catalog, 'tickets', 'sync', 'demo'), true);
  catalog.modules = [];
  assert.equal(connectorSupports(catalog, 'tickets', 'sync', 'demo'), false);
});

test('repository provider inherits from the project or general setting, while existing PRs stay pinned', () => {
  const state = { settings: { platform: 'win32' }, projects: [{ repo: 'D:/Repo', repositoryConnector: 'github' }], connectors: { providers: { repositories: 'azure' } } };
  const card = { repo: 'd:/repo' };
  assert.equal(repositoryProvider(state, card), 'github');
  assert.equal(repositoryProvider(state, { repo: 'D:/other' }), 'azure');
  state.projects[0].repositoryConnector = '';
  assert.equal(repositoryProvider(state, card), 'azure');
  state.projects[0].repositoryConnector = 'uninstalled';
  assert.equal(repositoryProvider(state, card), 'uninstalled');
  card.pullRequest = { connector: 'gitlab' };
  assert.equal(repositoryProvider(state, card), 'gitlab');
});
