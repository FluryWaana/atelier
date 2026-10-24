import test from 'node:test';
import assert from 'node:assert/strict';
import { createAgentProviders } from '../lib/application/agent-providers.mjs';
import { createSettings } from '../lib/application/settings.mjs';

test('provider mutations preserve UTF-16 secrets and historical guard precedence', () => {
  let config = { taskQueue: { ids: ['keep'] }, future: { keep: true } };
  let tokens = { jira: 'unrelated' }, busy = false, cards = [], projects = [];
  const store = { settings: () => config, saveSettings: value => config = value, transaction: change => change(), cards: () => cards, projects: () => projects };
  const settings = createSettings({ store, vault: { save: value => tokens = value }, initialTokens: tokens });
  const providers = createAgentProviders({ store, settings, busy: () => busy, invalidate() {} });
  const body = { id: 'api', name: '\ud800', type: 'codex', auth: 'api', apiKey: ' synthetic-\udc00 ', model: 'custom/model', effort: 'high' };
  let catalog = providers.save(body);
  assert.equal(tokens.jira, 'unrelated'); assert.equal(tokens['agent:api:key'], 'synthetic-\udc00');
  assert.equal(config.apiModel, 'custom/model'); assert.deepEqual(config.future, { keep: true });
  assert.deepEqual(config.taskQueue, { ids: ['keep'] }); assert.equal(catalog.length, 3);
  assert.equal(catalog[2].apiKeySet, true); assert.equal(JSON.stringify(catalog).includes('synthetic-'), false);
  delete body.apiKey; providers.save(body); assert.equal(tokens['agent:api:key'], 'synthetic-\udc00');
  cards = [{ agent: 'api', archived: true }]; assert.throws(() => providers.remove('api'), { code: 'conflict' });
  cards = []; projects = [{ agent: 'api' }]; assert.throws(() => providers.remove('api'), { code: 'conflict' });
  projects = []; assert.throws(() => providers.remove('codex'), { code: 'conflict' });
  busy = true; assert.throws(() => providers.save(body), { code: 'conflict' });
  assert.throws(() => providers.save({ ...body, type: 'claude', auth: 'cli' }), /type d’un profil existant/);
  assert.throws(() => providers.save({ ...body, type: 'claude', auth: 'cli', clearApiKey: 'false' }), /Clé API invalide/);
  busy = false; catalog = providers.remove('api'); assert.equal(catalog.length, 2);
  assert.equal(config.apiModel, 'custom/model'); assert.equal(tokens['agent:api:key'], '');
  busy = true; assert.throws(() => providers.remove('api'), { code: 'not-found' });
});

test('provider limits precede key checks and allow replacing an existing profile', () => {
  let config = { agentProviders: Array.from({ length: 50 }, (_, i) => ({ id: 'p' + i, type: 'codex' })) }, tokens = {};
  const store = { settings: () => config, saveSettings: value => config = value, transaction: change => change() };
  const settings = createSettings({ store, vault: { save: value => tokens = value }, initialTokens: tokens });
  const providers = createAgentProviders({ store, settings, busy: () => false, invalidate() {} });
  const body = { id: 'new', name: 'New', type: 'codex', auth: 'api', apiKey: 'a\nb' };
  assert.throws(() => providers.save(body), /50 fournisseurs maximum/);
  body.id = 'p0'; assert.throws(() => providers.save(body), /Clé API invalide/);
  body.auth = 'cli'; assert.throws(() => providers.save(body), /Sélectionnez l’authentification par clé API/);
  body.apiKey = ''; assert.equal(providers.save(body).length, 50); assert.equal(tokens['agent:p0:key'], '');
  body.auth = 'api'; body.apiKey = '\n key \n'; providers.save(body); assert.equal(tokens['agent:p0:key'], 'key');
  providers.save({ ...body, clearApiKey: true }); assert.equal(tokens['agent:p0:key'], '');
});
