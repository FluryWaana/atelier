import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { agentProviders, providerSettings, resolveAgentOptions } from '../lib/domain/agent-options.mjs';
import { agentStatus, createVersionChecker, versionState, startAgent, agentEnvironment } from '../lib/agents.mjs';
import { createApp } from '../server.mjs';
import { agentName, agentSelect, agentStatusText } from '../public/agent-options.mjs';

test('profiles preserve legacy defaults and validate configurable commands', () => {
  assert.equal(agentProviders({ codexModel: 'custom' })[0].model, 'custom');
  const provider = providerSettings({ id: 'work', name: 'Travail', type: 'claude', model: 'opus', effort: 'max' });
  assert.deepEqual(resolveAgentOptions({ agentProviders: [provider] }, {}, { agent: 'work' }), { model: 'opus', effort: 'max' });
  for (const body of [{ id: '../escape' }, { type: 'unknown' }, { auth: 'api' }, { configDir: '../relative' }, { command: './relative' }, { effort: 'minimal' }]) assert.throws(() => providerSettings({ ...provider, ...body }));
  const custom = { id: 'other', name: 'Autre', type: 'command', command: 'other' };
  for (const body of [{ analyzeArgs: '--unsafe' }, { editArgs: [null] }, { statusArgs: ['\0'] }, { npmPackage: '../private' }]) assert.throws(() => providerSettings({ ...custom, ...body }));
  assert.equal(agentName({ agentProviders: [provider] }, 'work'), 'Travail');
  assert.equal(agentName({}, 'removed', { agentName: 'Historic' }), 'Historic');
  assert.match(agentSelect({ agentProviders: [{ ...provider, name: '<script>' }] }, 'work'), /&lt;script&gt;/);
});

test('version comparison handles older, equal, newer and prerelease installations', () => {
  for (const [installed, latest, expected] of [['codex-cli 0.9.0', '0.10.0', 'outdated'], ['2.1.3 (Claude Code)', '2.1.3', 'current'], ['0.11.0', '0.10.0', 'ahead'], ['0.10.0-beta.1', '0.10.0', 'outdated'], ['0.11.0-beta.1', '0.10.0', 'ahead'], ['unknown', '1.0.0', 'unknown'], ['1.0.0', '', 'unknown']]) assert.equal(versionState(installed, latest), expected);
});

test('published versions are fetched without secrets, cached and tolerate offline registries', async () => {
  const requests = [];
  const latest = createVersionChecker(async (url, options) => { requests.push({ url, options }); return Response.json({ version: '2.3.4' }); });
  assert.deepEqual(await Promise.all([latest({ type: 'codex' }), latest({ type: 'codex' })]), ['2.3.4', '2.3.4']);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, 'https://registry.npmjs.org/%40openai%2Fcodex/latest');
  assert.equal(requests[0].options.headers, undefined);
  assert.equal(await latest({ type: 'command' }), '');
  assert.equal(await createVersionChecker(async () => { throw new Error('offline'); })({ type: 'claude' }), '');
  assert.equal(await createVersionChecker(async () => new Response(' '.repeat(65537) + '{"version":"2.3.4"}'))({ type: 'claude' }), '');
});

test('status exposes only account identity and reports configured API keys without claiming validation', async () => {
  const provider = providerSettings({ id: 'work', name: 'Travail', type: 'claude', command: process.execPath });
  const calls = [];
  const execute = async (binary, args) => { calls.push([binary, args]); return { stdout: args[0] === '--version' ? '2.0.0 (Claude Code)' : JSON.stringify({ loggedIn: true, email: 'test@example.com', authMethod: 'oauth', subscriptionType: 'max', accessToken: 'do-not-expose' }), stderr: '' }; };
  const status = await agentStatus('work', provider, '', execute, async () => '2.1.0');
  assert.equal(status.subscription, 'max'); assert.equal(status.account, 'test@example.com'); assert.equal(status.connected, true); assert.equal(status.updateState, 'outdated');
  assert.ok(!JSON.stringify(status).includes('do-not-expose')); assert.equal(calls[0][0], process.execPath);
  const api = await agentStatus('api', { type: 'codex', auth: 'api', command: process.execPath }, 'secret', execute, async () => '2.1.0');
  assert.equal(api.connected, false); assert.equal(api.configured, true); assert.equal(api.account, ''); assert.equal(calls.length, 3);
  assert.ok(!JSON.stringify(api).includes('secret'));
  assert.equal((await agentStatus('work', provider, '', async () => { throw new Error('missing'); }, async () => '')).installed, false);
  assert.ok(agentStatusText(status).includes('2.1.0'));
});

test('custom CLI receives separate arguments and stdin; Codex API key is scoped and redacted', async () => {
  const provider = providerSettings({ id: 'other', name: 'Autre', type: 'command', command: process.execPath, analyzeArgs: ['--read-only', '{model}', '{effort}'], editArgs: ['--write'], effort: 'deep' });
  const child = new EventEmitter(); Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  let launched, result;
  const lines = [];
  startAgent({ agent: provider.id, provider, mode: 'analyze', repo: '.', prompt: 'A task', model: 'model', effort: 'deep' }, (stream, line) => lines.push(line), code => result = code, (binary, args, options) => { launched = { binary, args, options }; return child; });
  assert.deepEqual(launched.args, ['--read-only', 'model', 'deep']); assert.equal(launched.options.shell, false); assert.equal(child.stdin.read().toString(), 'A task');
  child.stdout.end('Final answer'); await new Promise(resolve => child.stdout.once('end', resolve)); child.emit('close', 0);
  assert.equal(result, 0); assert.deepEqual(JSON.parse(lines.at(-1)), { type: 'result', result: 'Final answer' });
  const apiChild = new EventEmitter(); Object.assign(apiChild, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  startAgent({ agent: 'api', provider: { type: 'codex', auth: 'api', command: process.execPath }, apiKey: 'private-key', mode: 'edit', repo: '.', prompt: 'test' }, (_, line) => lines.push(line), () => {}, (_, args, options) => { launched = { args, options }; return apiChild; });
  assert.equal(launched.options.env.CODEX_API_KEY, 'private-key'); assert.ok(!launched.args.includes('private-key'));
  assert.ok(launched.args.includes('cli_auth_credentials_store="ephemeral"'));
  apiChild.stderr.write('error private-key\n'); apiChild.emit('close', 1);
  assert.equal(lines.at(-1), 'error [secret]');
  assert.equal(agentEnvironment({ type: 'claude' }).CODEX_API_KEY, undefined);
  const environment = { CODEX_API_KEY: 'codex-key', ANTHROPIC_API_KEY: 'claude-key', JIRA_TOKEN: 'jira-key', PATH: 'bin' };
  assert.deepEqual(agentEnvironment({ type: 'claude', auth: 'cli' }, '', environment), { PATH: 'bin', ANTHROPIC_API_KEY: 'claude-key' });
  assert.deepEqual(agentEnvironment({ type: 'codex', auth: 'api' }, 'selected-key', environment), { PATH: 'bin', CODEX_API_KEY: 'selected-key' });
  assert.deepEqual(agentEnvironment({ type: 'codex', configDir: '/other' }, '', environment), { PATH: 'bin', CODEX_HOME: '/other' });
  assert.throws(() => startAgent({ agent: 'api', provider: { type: 'codex', auth: 'api', command: process.execPath }, mode: 'edit' }, () => {}, () => {}), /clé API/);
});

test('provider CRUD persists secrets separately and works through tasks, projects, runs and restart', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'kanban-providers-')), dataDir = join(directory, 'data'), repo = join(directory, 'repo');
  mkdirSync(repo);
  const git = args => execFileSync('git', ['-C', repo, ...args], { windowsHide: true, stdio: 'pipe' });
  git(['init']); git(['-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '--allow-empty', '-m', 'Initial']); git(['remote', 'add', 'origin', repo]);
  const launches = [], probes = [];
  const options = { dataDir, protectTokens: value => value, probe: async (id, provider) => { probes.push(provider); return { installed: true, connected: true, version: '1.0.0' }; }, launch: (options, line, end) => { launches.push(options); line('stdout', JSON.stringify({ type: 'result', result: 'Done', total_cost_usd: 0.01, usage: { input_tokens: 1, output_tokens: 1 } })); end(0); return { stop() {} }; } };
  let app = createApp(options);
  async function serve() { await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve)); }
  const request = async (path, method = 'GET', body) => fetch(`http://127.0.0.1:${app.server.address().port}` + path, { method, headers: { Authorization: 'Bearer ' + app.authToken, 'X-Local-Request': '1', 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  try {
    await serve();
    assert.equal((await request('/views/agent-providers.mjs')).status, 200);
    const profile = { id: 'work', type: 'claude', name: 'Travail', model: 'opus', effort: 'max' };
    assert.equal((await request('/api/agent-providers', 'POST', profile)).status, 200);
    assert.equal((await request('/api/agent-providers', 'POST', { ...profile, clearApiKey: 'false' })).status, 400);
    assert.equal((await request('/api/agent-providers', 'POST', { ...profile, apiKey: 'wrong-auth' })).status, 400);
    assert.equal((await request('/api/agent-providers', 'POST', { id: 'api', name: 'Codex API', type: 'codex', auth: 'api', apiKey: 'private-key' })).status, 200);
    assert.ok(!JSON.stringify(app.store.settings()).includes('private-key'));
    const catalog = await (await request('/api/agent-providers')).json(); assert.equal(catalog.find(p => p.id === 'api').apiKeySet, true);
    assert.ok(!JSON.stringify(catalog).includes('private-key'));
    assert.equal((await request('/api/settings', 'PUT', { agent: 'work' })).status, 200);
    const card = await (await request('/api/cards', 'POST', { title: 'Test', repo })).json(); assert.equal(card.agent, 'work');
    const project = await (await request('/api/projects', 'POST', { repo, name: 'Test' })).json();
    assert.equal((await request('/api/projects/' + project.id, 'PATCH', { agent: 'api' })).status, 200);
    assert.equal((await request('/api/cards/' + card.id + '/run', 'POST', {})).status, 200);
    assert.equal(launches[0].provider.id, 'work'); assert.equal(launches[0].model, 'opus');
    assert.equal(app.store.runs()[0].agentType, 'claude'); assert.equal(app.store.runs()[0].agentName, 'Travail'); assert.equal(app.store.runs()[0].usage.totalTokens, 2);
    assert.equal((await request('/api/agent-providers/work', 'DELETE')).status, 409);
    assert.equal((await request('/api/agent-providers', 'POST', { ...profile, type: 'codex', effort: '' })).status, 400);
    await request('/api/agents'); assert.equal(probes.length, 4);
    await request('/api/agent-providers', 'POST', { ...profile, model: 'sonnet' });
    assert.equal(app.store.settings().workModel, 'sonnet');
    await request('/api/settings', 'PUT', { baseBranch: 'main' });
    assert.equal(app.store.settings().agentProviders.find(p => p.id === 'work').model, 'sonnet');
    await app.close(); app = createApp(options); await serve();
    assert.equal((await (await request('/api/agent-providers')).json()).find(p => p.id === 'api').apiKeySet, true);
    assert.equal((await request('/api/cards/' + card.id + '/run', 'POST', { agent: 'api', effort: '' })).status, 200);
    assert.equal(launches.at(-1).apiKey, 'private-key');
    assert.ok(!JSON.stringify(await (await request('/api/state')).json()).includes('private-key'));
    await request('/api/agent-providers', 'POST', { id: 'api', name: 'Codex API', type: 'codex', auth: 'api', clearApiKey: true });
    assert.equal((await (await request('/api/agent-providers')).json()).find(p => p.id === 'api').apiKeySet, false);
    assert.equal((await request('/api/agent-providers/claude', 'DELETE')).status, 200);
  } finally { await app.close(); rmSync(directory, { recursive: true, force: true }); }
});
