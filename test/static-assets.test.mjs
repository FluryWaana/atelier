import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';

test('the browser entry point and every imported module are served with strict CSP', async t => {
  const dataDir = mkdtempSync(join(tmpdir(), 'atelier-assets-'));
  const app = createApp({ dataDir });
  t.after(async () => { await app.close(); rmSync(dataDir, { recursive: true, force: true }); });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  const paths = new Set(['/app.js']);
  for (const path of paths) {
    const req = Readable.from([]), headers = {};
    Object.assign(req, { method: 'GET', url: path, headers: { host: '127.0.0.1:4317' } });
    const response = await new Promise(resolve => {
      let status;
      app.server.emit('request', req, {
        setHeader(name, value) { headers[name] = value; },
        writeHead(value, values) { status = value; Object.assign(headers, values); },
        end(body) { resolve({ status, body: String(body) }); },
      });
    });
    assert.equal(response.status, 200, path);
    assert.match(headers['Content-Type'], /^text\/javascript/, path);
    assert.match(headers['Content-Security-Policy'], /script-src 'self';/);
    const source = path.startsWith('/lib/domain/') ? '..' + path : '../public' + path;
    assert.equal(response.body, readFileSync(new URL(source, import.meta.url), 'utf8'));
    for (const match of response.body.matchAll(/^(?:import|export)\s[^\r\n;]*?from\s*['"]([^'"]+)['"]/gm)) {
      paths.add(new URL(match[1], 'http://127.0.0.1:4317' + path).pathname);
    }
  }
  assert.ok(paths.has('/agent-usage.mjs'));
});

