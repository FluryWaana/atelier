import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { agentAccount, quotaWindows } from '../lib/agent-account.mjs';
import { agentSidebarMarkup } from '../public/agent-options.mjs';
import { createAgentProvidersView, providerCardMarkup, providerDetailMarkup } from '../public/views/agent-providers.mjs';
import { setLocale } from '../public/i18n.mjs';

function mockServer(respond) {
  const requests = [], child = new EventEmitter();
  Object.assign(child, { stdout: new PassThrough(), stderr: new PassThrough(), killed: false, kill() { this.killed = true; } });
  child.stdin = new Writable({ write(chunk, encoding, callback) {
    const message = JSON.parse(chunk); requests.push(message); callback();
    queueMicrotask(() => { const response = respond(message); if (response) child.stdout.write(JSON.stringify(response) + '\n'); });
  } });
  return { child, requests };
}
const provider = { type: 'codex', auth: 'cli', command: process.execPath };

test('Codex account reads use only the read-only protocol and return public fields', async () => {
  const { child, requests } = mockServer(message => message.id === 1 ? { id: 1, result: {} }
    : message.id === 2 ? { id: 2, result: { account: { type: 'chatgpt', email: 'test@example.com', planType: 'pro', accessToken: 'hidden' } } }
      : message.id === 3 ? { id: 3, result: { rateLimits: { limitId: 'codex', primary: { usedPercent: 35, windowDurationMins: 300, resetsAt: 2000000000 }, secondary: { usedPercent: 90, windowDurationMins: 10080 } }, token: 'hidden' } } : null);
  const result = await agentAccount(provider, (binary, args, options) => {
    assert.equal(binary, process.execPath); assert.deepEqual(args, ['app-server']); assert.equal(options.shell, false); return child;
  });
  assert.equal(result.subscription, 'pro'); assert.equal(result.account, 'test@example.com');
  assert.equal(result.quotaState, 'available'); assert.equal(result.quotas[0].remainingPercent, 65);
  assert.equal(result.quotas[1].remainingPercent, 10); assert.equal(child.killed, true);
  assert.ok(!JSON.stringify(result).includes('hidden'));
  assert.deepEqual(requests.map(value => value.method), ['initialize', 'initialized', 'account/read', 'account/rateLimits/read']);
  assert.equal(requests[2].params.refreshToken, false);
});

test('account probes handle missing authentication, errors, output bounds and timeouts', async () => {
  for (const mode of ['disconnected', 'error', 'oversized', 'invalid', 'timeout']) {
    const { child } = mockServer(message => {
      if (mode === 'oversized') { child.stdout.write('x'.repeat(1024 * 1024 + 1)); return; }
      if (mode === 'invalid') { child.stdout.write('not-json\n'); return; }
      if (mode === 'timeout') return;
      if (message.id === 1) return { id: 1, result: {} };
      if (message.id === 2) return mode === 'error' ? { id: 2, error: { message: 'secret failure' } } : { id: 2, result: { account: null } };
    });
    const result = await agentAccount(provider, () => child, 15);
    assert.equal(result.quotaState, mode === 'disconnected' ? 'disconnected' : 'unavailable');
    assert.deepEqual(result.quotas, []); assert.ok(child.killed); assert.ok(!JSON.stringify(result).includes('secret'));
  }
  const noSpawn = () => { throw new Error('Must not spawn'); };
  assert.equal((await agentAccount({ type: 'codex', auth: 'api' }, noSpawn)).quotaState, 'api');
  assert.equal((await agentAccount({ type: 'command' }, noSpawn)).quotaState, 'unsupported');
});

test('quota normalization handles multiple buckets and rejects malformed percentages', () => {
  const result = quotaWindows({ rateLimitsByLimitId: {
    a: { limitName: 'codex', primary: { usedPercent: 0, windowDurationMins: 300, resetsAt: 0 } },
    b: { limitId: 'review', secondary: { usedPercent: 100, windowDurationMins: -1, resetsAt: 1e100 } },
    bad: { primary: { usedPercent: '25' }, secondary: { usedPercent: -1 } },
  } });
  assert.equal(result.length, 2); assert.equal(result[0].remainingPercent, 100); assert.equal(result[1].remainingPercent, 0);
  assert.equal(result[1].resetsAt, null); assert.equal(result[1].windowMinutes, null);
});

test('provider views expose accessible account navigation, honest unknown states and escaped content', () => {
  setLocale('fr-FR');
  const profile = { id: 'codex', type: 'codex', name: '<img src=x>', auth: 'cli' };
  const status = { connected: true, subscription: 'pro', account: '<script>bad</script>', version: 'codex-cli 1.2.3' };
  const sidebar = agentSidebarMarkup({ codex: { ...profile, ...status } });
  assert.match(sidebar, /<button type="button"/); assert.match(sidebar, /data-agent-account="codex"/);
  assert.doesNotMatch(sidebar, /codex-cli/);
  const card = providerCardMarkup(profile, status); assert.match(card, /data-open-provider="codex"/);
  const detail = providerDetailMarkup(profile, { ...status, quotas: [{ name: 'Codex', remainingPercent: 65, windowMinutes: 300, resetsAt: '2099-01-01T00:00:00Z' }] });
  assert.match(providerDetailMarkup(profile, status, true), /data-edit-provider="codex" disabled/);
  assert.match(detail, /value="65"/); assert.match(detail, /5 heures/); assert.match(detail, /noopener noreferrer/);
  for (const html of [sidebar, card, detail]) { assert.doesNotMatch(html, /<script\b|<img\b/i); }
  const unknown = providerDetailMarkup({ ...profile, type: 'claude' }, status);
  assert.match(unknown, /Quota non communiqué/); assert.ok(unknown.includes('href="https://claude.ai/settings/usage"')); assert.doesNotMatch(unknown, /<progress/);
  const apiAccount = providerDetailMarkup({ ...profile, type: 'claude' }, { authMethod: 'api_key' });
  assert.match(apiAccount, /Facturation à l’usage/); assert.ok(apiAccount.includes('href="https://platform.claude.com/usage"'));
  const expired = providerDetailMarkup(profile, { quotas: [{ remainingPercent: 65, windowMinutes: 300, resetsAt: '2000-01-01T00:00:00Z' }] });
  assert.match(expired, /À actualiser/); assert.doesNotMatch(expired, /<progress/);
});

test('provider editor renders supported types and ignores injected type markup', async () => {
  const options = {}, add = {}, cancel = {};
  const form = {
    elements: { type: { value: 'codex' }, name: { focus() {} } },
    querySelector: selector => ({ '[data-provider-options]': options, '[data-cancel-provider]': cancel })[selector] || null,
  };
  const editor = { querySelector: () => form };
  const host = {
    querySelector: selector => ({ '[data-add-provider]': add, '[data-provider-editor]': editor })[selector] || null,
    querySelectorAll: () => [],
  };
  const view = createAgentProvidersView({ document: { querySelector: () => host }, api: async () => [] });
  await view.open();
  add.onclick();
  for (const [type, placeholder] of [['codex', 'codex'], ['claude', 'claude'], ['command', '/chemin/agent'], ['"><img src=x onerror=alert(1)>', 'codex']]) {
    form.elements.type.value = type;
    form.elements.type.onchange();
    assert.ok(options.innerHTML.includes(`placeholder="${placeholder}"`));
    assert.doesNotMatch(options.innerHTML, /<img\b/i);
    assert.equal(options.innerHTML.includes('name="analyzeArgs"'), type === 'command');
  }
});
