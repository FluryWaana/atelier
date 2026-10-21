import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { localRequest } from '../lib/infrastructure/local-http.mjs';
import { authenticatedHealth } from '../lib/infrastructure/local-session.mjs';

test('local control requests avoid fetch and preserve errors, authentication, bounds and cancellation', async t => {
  t.mock.method(globalThis, 'fetch', () => { throw new Error('The local control channel must not use fetch'); });
  const received = [];
  const server = createServer(async (req, res) => {
    if (req.url === '/redirect') { res.writeHead(302, { Location: 'https://example.invalid' }); res.end(); return; }
    if (req.url === '/large') { res.end('x'.repeat(65537)); return; }
    if (req.url === '/hang') return;
    if (req.url.startsWith('/api/health')) { req.socket.destroy(); return; }
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    received.push({ method: req.method, authorization: req.headers.authorization, body: Buffer.concat(chunks).toString() });
    res.writeHead(409, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Agent actif' }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => { server.closeAllConnections(); return new Promise(resolve => server.close(resolve)); });
  const url = `http://127.0.0.1:${server.address().port}`;
  const response = await localRequest(url + '/api/shutdown', { method: 'POST', headers: { Authorization: 'Bearer test-session' }, body: '{}' });
  assert.equal(response.ok, false);
  assert.equal(response.status, 409);
  assert.deepEqual(await response.json(), { error: 'Agent actif' });
  assert.deepEqual(received, [{ method: 'POST', authorization: 'Bearer test-session', body: '{}' }]);
  await assert.rejects(localRequest(url + '/redirect'), /Redirection/);
  await assert.rejects(localRequest(url + '/large'), /volumineuse/);
  await assert.rejects(localRequest(url + '/hang', { signal: AbortSignal.timeout(30) }), { name: 'AbortError' });
  assert.equal(await authenticatedHealth(url, () => assert.fail('Reset sockets must not request credentials')), null);
  for (const address of ['https://127.0.0.1:4317', 'http://example.invalid', 'http://user:secret@127.0.0.1:4317', url + '/#session']) {
    await assert.rejects(localRequest(address), /Adresse du serveur local invalide/);
  }
});
