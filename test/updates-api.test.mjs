import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';

async function request(app, path, method = 'GET', body = {}, headers = {}) {
  const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
  Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', ...(method !== 'GET' ? { 'content-type': 'application/json', 'x-local-request': '1' } : {}), ...headers } });
  return new Promise(resolve => {
    let status;
    app.server.emit('request', req, { setHeader() {}, writeHead(value) { status = value; }, end(body) { resolve({ status, body: JSON.parse(body) }); } });
  });
}
test('update API rejects foreign requests, blocks busy installations and locks other mutations during preparation', async t => {
  const dataDir = mkdtempSync(join(tmpdir(), 'atelier-updates-'));
  let unblock, began;
  const preparing = new Promise(resolve => { began = resolve; });
  const app = createApp({ dataDir, updateOptions: { repository: 'owner/repo', releases: { latest: async () => ({ version: '9.0.0' }) },
    installer: { prepare: () => { began(); return new Promise(resolve => { unblock = resolve; }); }, start: async () => {} } } });
  t.after(async () => { await app.close(); rmSync(dataDir, { recursive: true, force: true }); });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  assert.equal((await request(app, '/api/updates/check', 'POST', {}, { origin: 'https://foreign.example' })).status, 403);
  assert.equal((await request(app, '/api/updates/check', 'POST')).body.available.version, '9.0.0');
  app.store.putTime({ id: 'timer', runningSince: new Date().toISOString() });
  assert.equal((await request(app, '/api/updates/install', 'POST', { version: '9.0.0' })).status, 409);
  app.store.deleteTime('timer');
  const install = request(app, '/api/updates/install', 'POST', { version: '9.0.0' }); await preparing;
  assert.equal((await request(app, '/api/queue', 'POST', { paused: false })).status, 409);
  assert.equal((await request(app, '/api/updates/install', 'POST', { version: '9.0.0' })).status, 409);
  assert.equal((await request(app, '/api/health')).status, 200);
  unblock(); assert.equal((await install).status, 200);
});
