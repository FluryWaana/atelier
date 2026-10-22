import test from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { mkdtempSync, rmSync } from 'node:fs';
import { agentUpdateInfo, updateAgent } from '../lib/agent-updates.mjs';
import { createAgentProviders } from '../lib/application/agent-providers.mjs';
import { createRuns } from '../lib/application/runs.mjs';
import { providerDetailMarkup } from '../public/views/agent-providers.mjs';
import { setLocale } from '../public/i18n.mjs';
import { createApp } from '../server.mjs';

test('native CLI updates use fixed arguments, filtered environment and sanitized failures', async () => {
  const home = join(tmpdir(), 'agent-update-home'), binary = join(home, '.local', 'bin', 'claude.exe');
  const env = { USERPROFILE: home, HOME: home, PATH: 'bin', CODEX_API_KEY: 'secret', ANTHROPIC_API_KEY: 'secret', NODE_OPTIONS: '--require=evil', DISABLE_UPDATES: '1' };
  const provider = { type: 'claude', configDir: join(home, 'profile') };
  assert.equal(agentUpdateInfo(provider, () => binary, env).canUpdate, true);
  for (const path of [join(home, 'node_modules', 'claude.exe'), join(home, '.local', 'bin-other', 'claude.exe')]) assert.equal(agentUpdateInfo(provider, () => path, env).canUpdate, false);
  for (const profile of [{ ...provider, command: binary }, { type: 'command' }]) await assert.rejects(updateAgent(profile, () => assert.fail('Must not execute'), () => binary, env));
  await updateAgent(provider, async (command, args, options) => {
    assert.equal(command, binary); assert.deepEqual(args, ['update']);
    assert.equal(options.shell, false); assert.equal(options.timeout, 180000);
    assert.equal(options.env.CLAUDE_CONFIG_DIR, provider.configDir); assert.equal(options.env.DISABLE_UPDATES, '1');
    for (const key of ['CODEX_API_KEY', 'ANTHROPIC_API_KEY', 'NODE_OPTIONS']) assert.equal(options.env[key], undefined);
  }, () => binary, env);
  await assert.rejects(updateAgent(provider, async () => { throw new Error('private output'); }, () => binary, env), error => !error.message.includes('private output'));
  let resolutions = 0;
  await updateAgent(provider, async command => assert.equal(command, binary), () => ++resolutions === 1 ? binary : join(home, 'unverified.exe'), env);
  assert.equal(resolutions, 1);
});

function fixture(overrides = {}) {
  const phases = []; let calls = 0;
  const service = createAgentProviders({ settings: { config: () => ({}), token: () => '' }, store: {}, busy: () => false, invalidate() {},
    updateInfo: () => ({ canUpdate: true }), install: async () => {},
    probe: async () => ++calls === 1 ? { installed: true, version: '1.0.0', latestVersion: '1.1.0', updateState: 'outdated' } : { installed: true, version: '1.1.0' },
    changed: () => phases.push(service.updateState()), ...overrides });
  return { service, phases };
}

test('update phases reflect actual work and hold the lock until verification finishes', async () => {
  let release, entered;
  const installing = new Promise(resolve => entered = resolve);
  const { service, phases } = fixture({ install: () => { entered(); return new Promise(resolve => release = resolve); } });
  const result = service.update('claude', '1.1.0'); await installing;
  assert.deepEqual(service.updateState(), { providerId: 'claude', phase: 'installing', updating: true });
  await assert.rejects(service.update('codex', '1.1.0'), { code: 'conflict' });
  assert.throws(() => service.save({ id: 'claude', name: 'Claude', type: 'claude' }), { code: 'conflict' });
  release(); assert.equal((await result).outcome, 'updated');
  assert.deepEqual(phases.map(state => state.phase), ['checking', 'installing', 'verifying', 'updated', 'updated']);
  assert.equal(service.updating(), false);
});

test('updates reject stale targets and busy state; failures release the lock without false success', async () => {
  const blocked = fixture({ install: () => assert.fail('Must not install') }).service;
  await assert.rejects(blocked.update('claude', '1.2.0'), { code: 'conflict' });
  assert.equal(blocked.updateState().phase, 'failed'); assert.equal(blocked.updating(), false);
  await assert.rejects(blocked.update('claude', '1.1.0;evil'));
  await assert.rejects(fixture({ busy: () => true }).service.update('claude', '1.1.0'), { code: 'conflict' });
  const failed = fixture({ install: async () => { throw new Error('update failed'); } }).service;
  await assert.rejects(failed.update('claude', '1.1.0'), /update failed/); assert.equal(failed.updating(), false);
  const unchanged = fixture({ probe: async () => ({ installed: true, version: '1.0.0', latestVersion: '1.1.0', updateState: 'outdated' }) }).service;
  assert.equal((await unchanged.update('claude', '1.1.0')).outcome, 'unchanged');
  assert.equal(unchanged.updateState().phase, 'unchanged');
});

test('provider details show indeterminate progress with phase labels and disable competing actions', () => {
  setLocale('fr-FR');
  const provider = { id: 'claude', type: 'claude', name: 'Claude', canUpdate: true };
  const status = { updateState: 'outdated', version: '1.0.0', latestVersion: '1.1.0' };
  for (const [phase, label] of [['checking', 'Vérification'], ['installing', 'Installation de'], ['verifying', 'Contrôle']]) {
    const html = providerDetailMarkup(provider, status, false, true, phase);
    assert.match(html, /<progress aria-label="Mise à jour du CLI"><\/progress>/);
    assert.match(html, new RegExp('data-update-phase>' + label));
    assert.match(html, /data-update-provider disabled/); assert.match(html, /data-edit-provider="claude" disabled/);
  }
  assert.doesNotMatch(providerDetailMarkup(provider, status), /<progress/);
  assert.doesNotMatch(providerDetailMarkup({ ...provider, canUpdate: false }, status), /data-update-provider/);
  assert.doesNotMatch(providerDetailMarkup(provider, { updateState: 'current' }), /data-update-provider/);
});

test('run starts are refused before accessing a card or preparing its worktree during an update', async () => {
  const runs = createRuns({ assertCanStart() { throw Object.assign(new Error('CLI update running'), { code: 'conflict' }); } });
  await assert.rejects(runs.start({}, {}), { code: 'conflict' });
});

test('HTTP update requires local authorization and exposes progress without invoking an installer on GET', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-update-'));
  let release, entered, installed = false;
  const installing = new Promise(resolve => entered = resolve);
  const app = createApp({ dataDir, protectTokens: value => value, getAgentUpdateInfo: () => ({ canUpdate: true }),
    probe: async () => ({ installed: true, version: installed ? '1.1.0' : '1.0.0', latestVersion: '1.1.0', updateState: installed ? 'current' : 'outdated' }),
    installAgentUpdate: async () => { entered(); await new Promise(resolve => release = resolve); installed = true; } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const base = 'http://127.0.0.1:' + app.server.address().port;
    const headers = { Authorization: 'Bearer ' + app.authToken, 'X-Local-Request': '1', 'Content-Type': 'application/json' };
    const request = (path, method = 'GET', body, extra = {}) => fetch(base + path, { method, headers: { ...headers, ...extra }, ...(body ? { body: JSON.stringify(body) } : {}) });
    const path = '/api/agent-providers/claude/update';
    const backup = await (await request('/api/backups', 'POST', {})).json();
    assert.ok(backup.name);
    assert.equal((await request(path)).status, 200); assert.equal(installed, false);
    assert.equal((await request(path, 'POST', { version: '1.1.0' }, { Authorization: '' })).status, 401);
    assert.equal((await request(path, 'POST', { version: '1.1.0' }, { 'X-Local-Request': '' })).status, 403);
    assert.equal((await request(path, 'POST', { version: '1.1.0' }, { Origin: 'https://evil.example' })).status, 403);
    assert.equal((await request(path, 'POST', { version: '1.1.0;evil' })).status, 400);
    const pending = request(path, 'POST', { version: '1.1.0' }); await installing;
    assert.equal((await (await request(path)).json()).phase, 'installing');
    assert.equal((await request(path, 'POST', { version: '1.1.0' })).status, 409);
    assert.equal((await request('/api/agents/claude/account', 'POST', {})).status, 409);
    assert.equal((await request('/api/backups/restore', 'POST', { confirmed: true, name: backup.name })).status, 409);
    assert.equal(app.store.settings().credentialsReset, undefined);
    assert.equal((await request('/api/agent-providers', 'POST', { id: 'claude', name: 'Claude', type: 'claude' })).status, 409);
    release(); const response = await pending;
    assert.equal(response.status, 200); assert.equal((await response.json()).outcome, 'updated');
    assert.equal((await (await request(path)).json()).updating, false);
  } finally { release?.(); await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});
