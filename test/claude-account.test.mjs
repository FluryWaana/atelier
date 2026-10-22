import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { claudeCredential, claudeQuotaWindows, createClaudeAccountReader } from '../lib/claude-account.mjs';
import { providerDetailMarkup } from '../public/views/agent-providers.mjs';
import { setLocale } from '../public/i18n.mjs';

const payload = { five_hour: { utilization: 27.5, resets_at: '2099-01-01T12:00:00Z' }, seven_day: { utilization: 90, resets_at: '2099-01-07T12:00:00Z' }, seven_day_sonnet: { utilization: 0, resets_at: null }, seven_day_opus: null, extra_usage: { utilization: 10 }, accessToken: 'never-return' };

test('Claude quota normalization returns session, weekly and model limits without guessing missing data', () => {
  const quotas = claudeQuotaWindows(payload);
  assert.equal(quotas.length, 3); assert.deepEqual(quotas.map(q => q.remainingPercent), [72.5, 10, 100]);
  assert.deepEqual(quotas.map(q => q.windowMinutes), [300, 10080, 10080]);
  assert.equal(quotas[2].name, 'sonnet'); assert.equal(quotas[2].resetsAt, null);
  assert.ok(!JSON.stringify(quotas).includes('never-return'));
  assert.deepEqual(claudeQuotaWindows({ five_hour: { utilization: '10' }, seven_day: { utilization: -1 }, seven_day_opus: { utilization: 101 } }), []);
  assert.deepEqual(claudeQuotaWindows(null), []);
});

test('Claude credentials stay in the selected profile and are never rewritten or refreshed', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'atelier-claude-quota-'));
  const path = join(directory, '.credentials.json');
  const raw = JSON.stringify({ claudeAiOauth: { accessToken: 'local-access', refreshToken: 'never-use-refresh', expiresAt: 2000000000000 } });
  try {
    writeFileSync(path, raw);
    const profile = { type: 'claude', configDir: directory };
    const credential = await claudeCredential(profile, { CLAUDE_CODE_OAUTH_TOKEN: 'wrong-profile', ANTHROPIC_API_KEY: 'wrong-api' });
    assert.deepEqual(credential, { token: 'local-access', expiresAt: 2000000000000 });
    assert.equal(readFileSync(path, 'utf8'), raw);
    assert.deepEqual(await claudeCredential({ type: 'claude' }, { CLAUDE_CODE_OAUTH_TOKEN: 'env-access' }), { token: 'env-access' });
    assert.equal((await claudeCredential({ type: 'claude' }, { ANTHROPIC_API_KEY: 'api-key' })).state, 'api');
    assert.equal((await claudeCredential({ type: 'claude', configDir: '\\\\server\\share' }, {})).state, 'unavailable');
    writeFileSync(path, 'x'.repeat(65537));
    assert.equal((await claudeCredential(profile, {})).state, 'unavailable');
    writeFileSync(path, JSON.stringify({ claudeAiOauth: { accessToken: 'bad\r\nheader' } }));
    assert.equal((await claudeCredential(profile, {})).state, 'disconnected');
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('Claude usage sends a read-only request to the fixed origin and deduplicates profiles by login', async () => {
  let calls = 0, time = 1000000;
  const read = createClaudeAccountReader({ now: () => time, readCredential: async () => ({ token: 'secret-access' }), request: async (url, options) => {
    calls++;
    assert.equal(url, 'https://api.anthropic.com/api/oauth/usage');
    assert.equal(options.method, 'GET'); assert.equal(options.redirect, 'error'); assert.ok(options.signal);
    assert.equal(options.headers.Authorization, 'Bearer secret-access'); assert.equal(options.headers['anthropic-beta'], 'oauth-2025-04-20');
    assert.equal(options.body, undefined);
    return Response.json(payload);
  } });
  const results = await Promise.all([read({ id: 'first' }), read({ id: 'alias' })]);
  assert.equal(calls, 1); assert.equal(results[0].quotaState, 'available'); assert.deepEqual(results[0], results[1]);
  assert.ok(!JSON.stringify(results).includes('secret-access')); assert.ok(!JSON.stringify(results).includes('never-return'));
  time += 299999; await read({}); assert.equal(calls, 1);
  time += 1; await read({}); assert.equal(calls, 2);
});

test('Claude reader respects Retry-After, expiry, failures and response bounds without leaking bodies', async () => {
  let time = 1000000, calls = 0;
  const read = createClaudeAccountReader({ now: () => time, readCredential: async () => ({ token: 'access' }), request: async () => { calls++; return new Response('secret-error', { status: 429, headers: { 'retry-after': '3600' } }); } });
  const result = await read({}); assert.equal(result.quotaState, 'rate-limited'); assert.equal(Date.parse(result.retryAt), time + 3600000);
  time += 3599999; await read({}); assert.equal(calls, 1);
  time++; await read({}); assert.equal(calls, 2);
  for (const credential of [{ token: 'access', expiresAt: time - 1 }, { token: 'bad\nheader' }, { state: 'api' }]) {
    const result = await createClaudeAccountReader({ now: () => time, readCredential: async () => credential, request: () => assert.fail('Must not call the network') })({});
    assert.equal(result.quotaState, credential.state || 'disconnected');
  }
  for (const response of [new Response('secret-error', { status: 401 }), new Response('secret-error', { status: 403 }), new Response('x'.repeat(65537)), new Response('malformed-json')]) {
    const result = await createClaudeAccountReader({ readCredential: async () => ({ token: 'access' }), request: async () => response })({});
    assert.deepEqual(result.quotas, []); assert.ok(!JSON.stringify(result).includes('secret-error'));
  }
  const failed = await createClaudeAccountReader({ readCredential: async () => ({ token: 'access' }), request: async () => { throw new Error('secret-access'); } })({});
  assert.deepEqual(failed, { quotas: [], quotaState: 'unavailable' });
});

test('Claude detail displays real quota timestamps and actionable cooldowns', () => {
  setLocale('fr-FR');
  const provider = { type: 'claude', id: 'claude', name: 'Claude' };
  const markup = providerDetailMarkup(provider, { quotas: claudeQuotaWindows(payload), quotaState: 'available', quotaCheckedAt: '2099-01-01T00:00:00Z', checkedAt: '2099-01-02T00:00:00Z' });
  assert.match(markup, /value="72.5"/); assert.match(markup, /value="10"/); assert.match(markup, /sonnet/);
  assert.match(markup, /01\/01\/2099/); assert.doesNotMatch(markup, /02\/01\/2099/);
  const limited = providerDetailMarkup(provider, { quotaState: 'rate-limited', retryAt: '2099-01-01T12:00:00Z' });
  assert.match(limited, /Anthropic limite les vérifications/); assert.doesNotMatch(limited, /<progress/);
});
