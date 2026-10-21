import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, mkdirSync, copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';
import { loadConnectors } from '../lib/infrastructure/connector-loader.mjs';
import demo from '../examples/connectors/demo/index.mjs';

async function fixture(run, connectorModules = [demo], options = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'atelier-connectors-'));
  const app = createApp({ dataDir: directory, connectorModules, protectTokens: (value, decrypt) => decrypt ? value.slice(7) : 'opaque:' + value, ...options });
  await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
  const origin = 'http://127.0.0.1:' + app.server.address().port;
  async function request(path, method = 'GET', body) {
    const response = await fetch(origin + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    const result = await response.json();
    assert.equal(response.status, 200, JSON.stringify(result));
    return result;
  }
  try { await run({ app, request, origin, directory }); }
  finally { await app.close(); rmSync(directory, { recursive: true, force: true }); }
}

test('application runs without builtins and rejects unavailable ticket operations', async () => {
  await fixture(async ({ app, request, origin }) => {
    const catalog = await request('/api/connectors');
    assert.deepEqual(catalog.modules, []);
    assert.equal(catalog.providers.tickets, '');
    const response = await fetch(origin + '/api/tickets/sync', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: '{}' });
    assert.equal(response.ok, false);
  }, [], { builtinModules: [] });
});

test('a provider can be disabled without uninstalling its module', async () => {
  await fixture(async ({ request }) => {
    await request('/api/connectors/providers', 'PUT', { tickets: 'demo' });
    await request('/api/connectors/providers', 'PUT', { tickets: '' });
    const catalog = await request('/api/connectors');
    assert.equal(catalog.providers.tickets, '');
    assert.ok(catalog.modules.some(module => module.id === 'demo'));
  });
});

test('invalid modules leave the application usable and expose load errors', async () => {
  await fixture(async ({ request }) => {
    const catalog = await request('/api/connectors');
    assert.equal(catalog.errors.length, 2);
    assert.ok(catalog.modules.some(module => module.id === 'demo'));
    assert.ok(!catalog.modules.some(module => module.id === 'broken'));
    await request('/api/connectors/providers', 'PUT', { tickets: 'demo' });
    assert.deepEqual(await request('/api/tickets/sync', 'POST', {}), { count: 1 });
  }, [demo, { id: 'broken', name: 'Broken', apiVersion: 99 }], { connectorErrors: [{ id: 'missing-file', message: 'Import impossible' }] });
});

test('GitHub and GitLab are selectable repository providers with encrypted secrets', async () => {
  await fixture(async ({ request }) => {
    for (const provider of ['github', 'gitlab']) {
      const catalog = await request('/api/connectors/providers', 'PUT', { repositories: provider });
      assert.equal(catalog.providers.repositories, provider);
      assert.deepEqual(catalog.modules.find(module => module.id === provider).operations.repositories, ['preview', 'publish', 'refresh', 'review']);
      const saved = await request(`/api/connectors/${provider}/settings`, 'PUT', { url: `https://${provider}.com`, token: 'fake-private-token' });
      assert.equal(saved.modules.find(module => module.id === provider).secrets.token, true);
      assert.equal(JSON.stringify(saved).includes('fake-private-token'), false);
      const changed = await request(`/api/connectors/${provider}/settings`, 'PUT', { url: 'https://forge.example' });
      assert.equal(changed.modules.find(module => module.id === provider).secrets.token, false);
    }
  });
});

test('installed example imports generic tickets, keeps local work and runs its contributed tab', async () => {
  await fixture(async ({ app, request, origin, directory }) => {
    const install = join(directory, 'connectors'); mkdirSync(join(install, 'demo'), { recursive: true });
    copyFileSync(new URL('../examples/connectors/demo/index.mjs', import.meta.url), join(install, 'demo/index.mjs'));
    assert.equal((await loadConnectors(install))[0].id, 'demo');
    await request('/api/connectors/demo/settings', 'PUT', { title: 'Ticket importé' });
    await request('/api/connectors/providers', 'PUT', { tickets: 'demo' });
    assert.deepEqual(await request('/api/tickets/sync', 'POST', {}), { count: 1 });
    const first = app.store.cards()[0];
    assert.equal(first.title, 'Ticket importé'); assert.equal(first.ticket.connector, 'demo');
    app.store.putCard({ ...first, status: 'review', instructions: 'À conserver' });
    await request('/api/tickets/sync', 'POST', {});
    assert.equal(app.store.cards().length, 1);
    assert.equal(app.store.card(first.id).status, 'review');
    assert.equal(app.store.card(first.id).instructions, 'À conserver');
    assert.deepEqual(await request('/api/connectors/demo/actions/count', 'POST', {}), { tickets: 1 });
    await request('/api/settings', 'PUT', { agent: 'claude' });
    const catalog = await request('/api/connectors');
    assert.equal(catalog.providers.tickets, 'demo');
    assert.equal(catalog.modules.find(module => module.id === 'demo').values.title, 'Ticket importé');
    const html = await (await fetch(origin)).text();
    assert.match(html, /id="settings-page"/); assert.doesNotMatch(html, /id="settings-dialog"/);
    assert.equal((await fetch(origin + '/views/settings.mjs')).status, 200);
  });
});

test('module secrets are encrypted, never returned, and invalidated when the destination changes', async () => {
  await fixture(async ({ app, request }) => {
    await request('/api/connectors/jira/settings', 'PUT', { type: 'cloud', url: 'https://jira.example', email: 'demo@example.org', token: 'secret-fixture', jql: 'project = DEMO' });
    let state = await request('/api/state');
    assert.equal(state.settings.jiraUrl, 'https://jira.example'); assert.equal(state.settings.tokenSet, true);
    assert.equal(JSON.stringify(state).includes('secret-fixture'), false);
    assert.equal(JSON.stringify(app.store.settings()).includes('secret-fixture'), false);
    await request('/api/connectors/jira/settings', 'PUT', { url: 'https://another.example' });
    state = await request('/api/state'); assert.equal(state.settings.tokenSet, false);
  });
});

test('a third-party ticket creation attaches the remote ticket to the existing local card', async () => {
  const module = { ...demo, capabilities: { tickets: { ...demo.capabilities.tickets,
    create(context, { card }) { return context.tickets.attach(card.id, { id: 'remote-1', key: 'REMOTE-1', title: card.title, description: card.description, url: 'https://example.org/remote/1', status: 'Open' }); },
  } } };
  await fixture(async ({ app, request }) => {
    await request('/api/connectors/providers', 'PUT', { tickets: 'demo' });
    const local = await request('/api/cards', 'POST', { title: 'Publier cette carte' });
    const linked = await request('/api/cards/' + local.id + '/create-ticket', 'POST', {});
    assert.equal(linked.id, local.id); assert.equal(linked.ticket.key, 'REMOTE-1');
    assert.equal(app.store.cards().length, 1);
  }, [module]);
});

test('repository providers receive their own context and existing PRs keep their provider', async () => {
  const calls = [];
  const forge = { id: 'forge', name: 'Forge', apiVersion: 1, capabilities: { repositories: {
    preview(context, input) { calls.push(input); return { title: 'PR', body: '', targetBranch: 'main' }; },
    publish(context, input) { calls.push(input); return { number: 12, title: 'PR', url: 'https://forge.example/pr/12', status: 'active' }; },
    refresh(context, input) { calls.push(input); return { status: 'completed' }; },
    review() { return { source: 'abc', target: 'def' }; },
  } } };
  await fixture(async ({ app, request }) => {
    await request('/api/connectors/providers', 'PUT', { repositories: 'forge' });
    const card = await request('/api/cards', 'POST', { title: 'A task' });
    await request('/api/cards/' + card.id + '/pr-preview');
    await request('/api/cards/' + card.id + '/pull-request', 'POST', { title: 'PR' });
    assert.equal(app.store.card(card.id).pullRequest.connector, 'forge');
    await request('/api/connectors/providers', 'PUT', { repositories: 'azure' });
    assert.deepEqual((await request('/api/connectors')).pullRequestProviders, ['forge']);
    app.store.putCard({ id: 'legacy-pr', archived: true, pullRequest: { number: 1 } });
    assert.deepEqual(new Set((await request('/api/connectors')).pullRequestProviders), new Set(['azure', 'forge']));
    await request('/api/cards/' + card.id + '/refresh-pr', 'POST', {});
    assert.equal(app.store.card(card.id).pullRequest.status, 'completed');
    assert.ok(calls.every(input => !Array.isArray(input) && Object.keys(input).every(key => ['card', 'body'].includes(key))));
  }, [forge]);
});

test('project repository override wins, empty inherits, and published PRs remain pinned', async () => {
  const calls = [];
  const forge = id => ({ id, name: id, apiVersion: 1, capabilities: { repositories: {
    preview() { calls.push(id); return { title: 'PR', body: '', base: 'main' }; },
    publish() { calls.push(id); return { number: 12, title: 'PR', url: 'https://example.org/pr/12', status: 'active' }; },
    refresh() { calls.push(id); return { status: 'completed' }; },
    review() { calls.push(id); return { source: 'abc', target: 'def' }; },
  } } });
  await fixture(async ({ app, request, directory, origin }) => {
    app.store.putProject({ id: 'project', name: 'Projet', repo: directory });
    await request('/api/connectors/providers', 'PUT', { repositories: 'general' });
    const card = await request('/api/cards', 'POST', { title: 'Projet' });
    app.store.putCard({ ...card, repo: directory });
    await request(`/api/cards/${card.id}/pr-preview`);
    assert.equal(calls.pop(), 'general');
    const saved = await request('/api/projects/project', 'PATCH', { repositoryConnector: 'specific' });
    assert.equal(saved.repositoryConnector, 'specific');
    assert.deepEqual((await request('/api/connectors')).projectRepositoryProviders, ['specific']);
    await request(`/api/cards/${card.id}/pr-preview`);
    assert.equal(calls.pop(), 'specific');
    await request('/api/connectors/providers', 'PUT', { repositories: '' });
    await request(`/api/cards/${card.id}/pull-request`, 'POST', { title: 'PR' });
    assert.equal(calls.pop(), 'specific');
    assert.equal(app.store.card(card.id).pullRequest.connector, 'specific');
    await request('/api/projects/project', 'PATCH', { repositoryConnector: '' });
    await request('/api/connectors/providers', 'PUT', { repositories: 'general' });
    await request(`/api/cards/${card.id}/refresh-pr`, 'POST', {});
    assert.equal(calls.pop(), 'specific');
    app.store.putCard({ ...app.store.card(card.id), pullRequest: null });
    await request(`/api/cards/${card.id}/pr-preview`);
    assert.equal(calls.pop(), 'general');
    for (const repositoryConnector of ['missing', 'jira', {}, 'invalid/id']) {
      const response = await fetch(origin + '/api/projects/project', { method: 'PATCH', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify({ repositoryConnector }) });
      assert.equal(response.ok, false);
      assert.equal(app.store.projects()[0].repositoryConnector, '');
    }
    app.store.putProject({ ...app.store.projects()[0], repositoryConnector: 'removed' });
    const unavailable = await fetch(origin + `/api/cards/${card.id}/pr-preview`, { headers: { Authorization: 'Bearer ' + app.authToken } });
    assert.equal(unavailable.ok, false);
    assert.equal(calls.length, 0);
  }, [forge('general'), forge('specific')]);
});
