import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, readFileSync, statSync } from 'node:fs';
import { join, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { request as httpRequest } from 'node:http';
import { DatabaseSync } from 'node:sqlite';
import { createApp } from '../server.mjs';
import { authenticatedHealth, readSession, writeSession, sameSecret, sessionProof, browserAddress } from '../lib/infrastructure/local-session.mjs';

async function fixture(t, options = {}) {
  const dataDir = mkdtempSync(join(tmpdir(), 'atelier-security-'));
  let app;
  const start = async () => { app = createApp({ dataDir, ...options }); await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve)); return app; };
  await start();
  t.after(async () => { await app.close(); assert.ok(resolve(dataDir).startsWith(resolve(tmpdir()) + sep)); rmSync(dataDir, { recursive: true, force: true }); });
  const request = (path, method = 'GET', body, headers = {}) => new Promise((resolve, reject) => {
    const req = httpRequest({ hostname: '127.0.0.1', port: app.server.address().port, path, method, headers: { Authorization: 'Bearer ' + app.authToken, 'Content-Type': 'application/json', 'X-Local-Request': '1', ...headers } }, res => {
      let text = ''; res.setEncoding('utf8'); res.on('data', chunk => text += chunk); res.on('end', () => resolve({ status: res.statusCode, body: JSON.parse(text) }));
    }); req.on('error', reject); req.end(body === undefined ? undefined : JSON.stringify(body));
  });
  return { app, request, dataDir, restart: async () => { await app.close(); return start(); } };
}

test('local health refuses remote or malformed addresses before reading credentials', async () => {
  const unexpected = () => { assert.fail('Invalid addresses must not cause network or credential access'); };
  for (const url of ['https://127.0.0.1:4317', 'http://example.com:4317', 'http://127.0.0.1:4317@evil.invalid', 'http://127.0.0.1:0', 'http://127.0.0.1:65536', 'http://127.0.0.1:4317/path']) {
    await assert.rejects(authenticatedHealth(url, unexpected, unexpected), /Adresse du serveur local invalide/);
  }
  const token = 'a'.repeat(64);
  const health = await authenticatedHealth('http://127.0.0.1:80', () => token, async url => {
    const challenge = new URL(url).searchParams.get('challenge');
    return Response.json(challenge ? { proof: sessionProof(token, challenge, 80) } : { app: 'atelier-kanban' });
  });
  assert.equal(health.app, 'atelier-kanban');
});

test('private API, event stream and shutdown reject anonymous or foreign requests', async t => {
  let stopped = 0;
  const { app, request } = await fixture(t, { shutdown: async () => { stopped++; } });
  for (const path of ['/api/state', '/api/health', '/api/events', '/api/backups']) {
    assert.equal((await request(path, 'GET', undefined, { Authorization: '' })).status, 401, path);
  }
  assert.equal((await request('/api/cards', 'POST', { title: 'Forbidden' }, { Authorization: '' })).status, 401);
  assert.equal((await request('/api/shutdown', 'POST', {}, { Authorization: '' })).status, 401);
  assert.equal(stopped, 0);
  assert.equal((await request('/api/state', 'GET', undefined, { Host: 'foreign.example' })).status, 403);
  assert.equal((await request('/api/state', 'GET', undefined, { Origin: 'https://foreign.example' })).status, 403);
  assert.equal((await request('/api/cards', 'POST', { title: 'Forbidden' }, { 'X-Local-Request': '' })).status, 403);
  assert.equal((await request('/api/state?session=' + app.authToken, 'GET', undefined, { Authorization: '' })).status, 401);
  assert.equal((await request('/api/state')).status, 200);
  assert.equal((await request('/api/shutdown', 'POST', {})).status, 200);
  assert.equal(stopped, 1);
  assert.equal(sameSecret('é'.repeat(64), 'a'.repeat(64)), false);
});

test('server identity is proved before disclosing credentials and sessions rotate on restart', async t => {
  const context = await fixture(t);
  const url = 'http://127.0.0.1:' + context.app.server.address().port;
  const health = await authenticatedHealth(url, () => context.app.authToken);
  assert.equal(health.app, 'atelier-kanban');
  let calls = 0;
  await assert.rejects(authenticatedHealth(url, () => context.app.authToken, async (path, options) => {
    calls++; assert.equal(options.headers?.Authorization, undefined);
    return Response.json({ app: 'atelier-kanban', pid: process.pid, proof: 'a'.repeat(64) });
  }), /Identité/);
  assert.equal(calls, 1);
  for (const code of ['ECONNREFUSED', 'ECONNRESET', 'UND_ERR_SOCKET']) {
    assert.equal(await authenticatedHealth(url, () => { throw Error('Unavailable server must not need credentials'); }, async () => { throw new TypeError('fetch failed', { cause: { code } }); }), null);
  }
  await assert.rejects(authenticatedHealth(url, () => context.app.authToken, async () => { throw new Error('timeout'); }), /timeout/);
  let started = false;
  await authenticatedHealth(url, () => { assert.equal(started, true); return context.app.authToken; }, async path => {
    started = true;
    const target = new URL(path), challenge = target.searchParams.get('challenge');
    return Response.json(challenge ? { proof: sessionProof(context.app.authToken, challenge, target.port) } : health);
  });
  writeSession(context.dataDir, context.app.authToken);
  assert.equal(readSession(context.dataDir), context.app.authToken);
  const address = browserAddress(context.dataDir, url, context.app.authToken, 'linux');
  assert.equal(address.includes(context.app.authToken), false);
  assert.equal(new URL(address).protocol, 'file:');
  assert.ok(readFileSync(new URL(address), 'utf8').includes(url + '/#session=' + context.app.authToken));
  if (process.platform !== 'win32') assert.equal(statSync(new URL(address)).mode & 0o077, 0);
  assert.equal(browserAddress(context.dataDir, url, context.app.authToken, 'win32'), url + '/#session=' + context.app.authToken);
  const previous = context.app.authToken;
  const restarted = await context.restart();
  assert.notEqual(restarted.authToken, previous);
  assert.equal((await context.request('/api/state', 'GET', undefined, { Authorization: 'Bearer ' + previous })).status, 401);
});

test('restoration clears persisted credentials and environment fallback after restart', async t => {
  const sent = [];
  const environmentToken = process.env.JIRA_TOKEN;
  process.env.JIRA_TOKEN = 'FAKE_ENV_TOKEN';
  t.after(() => { if (environmentToken === undefined) delete process.env.JIRA_TOKEN; else process.env.JIRA_TOKEN = environmentToken; });
  const context = await fixture(t, { protectTokens: (value, decrypt) => decrypt ? value.slice(5) : 'test:' + value,
    requestJira: async (config, token) => { sent.push({ url: config.jiraUrl, token }); return { issues: [], isLast: true }; } });
  const configure = body => context.request('/api/connectors/jira/settings', 'PUT', body);
  assert.equal((await configure({ url: 'https://old.example', type: 'server', token: 'FAKE_OLD' })).status, 200);
  const backup = await context.request('/api/backups', 'POST', {});
  assert.equal((await configure({ url: 'https://new.example', token: 'FAKE_NEW' })).status, 200);
  assert.equal((await context.request('/api/backups/restore', 'POST', { name: backup.body.name, confirmed: true })).status, 200);
  assert.equal(context.app.store.db.prepare('SELECT count(*) AS n FROM token_vault').get().n, 0);
  await context.restart();
  assert.equal((await context.request('/api/state')).body.settings.tokenSet, false);
  await context.request('/api/tickets/sync', 'POST', {});
  assert.equal(sent.at(-1).token, '');
  assert.equal(sent.at(-1).url, 'https://old.example');
  await configure({ token: 'FAKE_REENTERED' });
  await context.restart();
  await context.request('/api/tickets/sync', 'POST', {});
  assert.equal(sent.at(-1).token, 'FAKE_REENTERED');
});

test('failed restoration rolls back credential removal with the original settings', async t => {
  const context = await fixture(t, { protectTokens: (value, decrypt) => decrypt ? value.slice(5) : 'test:' + value });
  assert.equal((await context.request('/api/connectors/jira/settings', 'PUT', { url: 'https://current.example', token: 'FAKE_CURRENT' })).status, 200);
  const backup = await context.request('/api/backups', 'POST', {});
  const source = new DatabaseSync(join(context.dataDir, 'backups', backup.body.name));
  source.exec("UPDATE settings SET data='invalid json'"); source.close();
  assert.equal((await context.request('/api/backups/restore', 'POST', { name: backup.body.name, confirmed: true })).status, 400);
  assert.equal(context.app.store.db.prepare('SELECT count(*) AS n FROM token_vault').get().n, 1);
  await context.restart();
  const state = (await context.request('/api/state')).body;
  assert.equal(state.settings.tokenSet, true);
  assert.equal(state.settings.jiraUrl, 'https://current.example');
});
