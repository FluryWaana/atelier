import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';

test('health returns the package version', async t => {
  const dataDir = mkdtempSync(join(tmpdir(), 'atelier-version-'));
  const app = createApp({ dataDir });
  t.after(async () => { await app.close(); rmSync(dataDir, { recursive: true, force: true }); });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  const req = Readable.from([]);
  Object.assign(req, { method: 'GET', url: '/api/health', headers: { host: '127.0.0.1:4317' } });
  const result = await new Promise(resolve => {
    app.server.emit('request', req, { setHeader() {}, writeHead(status) { assert.equal(status, 200); }, end(value) { resolve(JSON.parse(value)); } });
  });
  const { version } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  assert.equal(result.version, version);
});
