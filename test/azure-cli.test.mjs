import test from 'node:test';
import assert from 'node:assert/strict';
import { azureRequest } from '../lib/pull-requests.mjs';

const repository = { api: 'https://dev.azure.com/org/project/_apis/git/repositories/repo' };
const tenant = '7d1dc204-93dc-4569-98be-84ddd3e7dd9f';
const discovery = () => new Response(null, { status: 401, headers: { 'x-vss-resourcetenant': tenant } });

test('Azure utilise la CLI sans PAT et conserve la priorité du PAT', async t => {
  const headers = [];
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    assert.equal(options.redirect, 'manual');
    assert.equal(options.headers['X-TFS-FedAuthRedirect'], 'Suppress');
    if (!options.headers.Authorization) return discovery();
    headers.push(options.headers.Authorization);
    assert.equal(url.searchParams.get('api-version'), '7.1');
    return { ok: true, json: async () => ({ id: 'repo' }) };
  });
  let calls = 0;
  const run = async (command, args) => {
    calls++;
    assert.equal(command, process.platform === 'win32' ? 'cmd.exe' : 'az');
    assert.match(args.join(' '), /account get-access-token --resource 499b84ac-1321-427f-aa17-267ca6975798/);
    assert.match(args.join(' '), new RegExp('--tenant ' + tenant));
    return 'temporary-token\r\n';
  };
  assert.deepEqual(await azureRequest(repository, '', '', undefined, run), { id: 'repo' });
  await azureRequest(repository, 'pat', '', undefined, run);
  assert.equal(calls, 1);
  assert.deepEqual(headers, ['Bearer temporary-token', `Basic ${Buffer.from(':pat').toString('base64')}`]);
});

test('Azure explique comment se connecter si la CLI échoue ou renvoie un jeton vide', async t => {
  t.mock.method(globalThis, 'fetch', async () => discovery());
  for (const run of [async () => { throw new Error('secret stderr'); }, async () => '  ']) {
    await assert.rejects(azureRequest(repository, '', '', undefined, run), error => {
      assert.match(error.message, /az login/);
      assert.ok(error.message.includes('--tenant ' + tenant));
      assert.ok(!error.message.includes('secret stderr'));
      return true;
    });
  }
});

test('Azure refuse un tenant injecté avant de lancer la CLI', async t => {
  t.mock.method(globalThis, 'fetch', async () => new Response(null, { headers: { 'x-vss-resourcetenant': 'tenant & echo secret' } }));
  await assert.rejects(azureRequest(repository, '', '', undefined, async () => assert.fail('CLI appelée')), /tenant invalide/);
});

test('Azure précise les erreurs réseau sans exposer les secrets', async t => {
  t.mock.method(globalThis, 'fetch', async () => {
    throw new TypeError('fetch failed', { cause: Object.assign(new Error('secret-url'), { code: 'UNABLE_TO_GET_ISSUER_CERT_LOCALLY' }) });
  });
  for (const token of ['', 'secret-pat']) {
    await assert.rejects(azureRequest(repository, token), error => {
      assert.match(error.message, /UNABLE_TO_GET_ISSUER_CERT_LOCALLY/);
      assert.ok(!error.message.includes('secret'));
      return true;
    });
  }
});

test('Azure explique un refus HTTP sans suivre la page de connexion', async t => {
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    assert.equal(options.redirect, 'manual');
    return new Response(null, { status: 302, headers: { Location: 'https://login.example/' } });
  });
  await assert.rejects(azureRequest(repository, 'pat'), /HTTP 302.*accès du compte/);
});
