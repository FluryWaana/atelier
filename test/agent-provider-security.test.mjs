import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { agentEnvironment, agentStatus, resolveExecutable, startAgent, eventProgress, eventText } from '../lib/agents.mjs';
import { providerSettings } from '../lib/domain/agent-options.mjs';
import { createApp } from '../server.mjs';
import { readAgentCredentials } from '../lib/agent-credentials.mjs';

test('agents inherit only allowed system variables and their own credentials', () => {
  const environment = { Path: dirname(process.execPath), HOME: '/home/test', SystemRoot: 'C:\\Windows', TEMP: '/tmp',
    AWS_SECRET_ACCESS_KEY: 'aws-secret', GITHUB_TOKEN: 'github-secret', DATABASE_URL: 'database-secret',
    NODE_OPTIONS: '--require=malicious.js', LD_PRELOAD: '/malicious.so', PYTHONPATH: '/malicious',
    NODE_TLS_REJECT_UNAUTHORIZED: '0', HTTPS_PROXY: 'https://attacker.invalid',
    CODEX_API_KEY: 'codex-secret', ANTHROPIC_API_KEY: 'claude-secret', JIRA_TOKEN: 'jira-secret', CLAUDECODE: 'nested',
  };
  const clean = { Path: dirname(process.execPath), HOME: '/home/test', SystemRoot: 'C:\\Windows', TEMP: '/tmp' };
  assert.deepEqual(agentEnvironment({ type: 'command' }, '', environment), clean);
  assert.deepEqual(agentEnvironment({ type: 'codex', auth: 'api' }, 'selected-key', environment), { ...clean, CODEX_API_KEY: 'selected-key' });
  assert.deepEqual(agentEnvironment({ type: 'claude', auth: 'cli' }, '', environment), { ...clean, ANTHROPIC_API_KEY: 'claude-secret' });
});

test('Claude task sessions inherit MCP and plugin environment without parent session or runtime injection', () => {
  const environment = { PATH: '/bin', HOME: '/home/test', CLAUDE_CONFIG_DIR: '/personal/claude',
    MCP_SERVICE_TOKEN: 'mcp-secret', RTK_CONFIG_DIR: '/personal/rtk', HTTPS_PROXY: 'https://proxy.invalid',
    ANTHROPIC_API_KEY: 'claude-secret', CLAUDECODE: '1', NODE_OPTIONS: '--require=inject.js',
    NODE_TLS_REJECT_UNAUTHORIZED: '0', PYTHONPATH: '/inject', LD_PRELOAD: '/inject.so' };
  const env = agentEnvironment({ type: 'claude' }, '', environment, true);
  assert.deepEqual(env, { PATH: '/bin', HOME: '/home/test', CLAUDE_CONFIG_DIR: '/personal/claude',
    MCP_SERVICE_TOKEN: 'mcp-secret', RTK_CONFIG_DIR: '/personal/rtk', HTTPS_PROXY: 'https://proxy.invalid', ANTHROPIC_API_KEY: 'claude-secret' });
  const other = agentEnvironment({ type: 'claude', configDir: '/other/profile' }, '', environment, true);
  assert.equal(other.CLAUDE_CONFIG_DIR, '/other/profile');
  assert.equal(other.ANTHROPIC_API_KEY, undefined);
  assert.equal(environment.CLAUDECODE, '1');
});

test('Claude launches in the task directory and redacts inherited MCP credentials', () => {
  const previous = process.env.ATELIER_TEST_MCP_TOKEN;
  process.env.ATELIER_TEST_MCP_TOKEN = 'private-mcp-credential';
  try {
    const child = new EventEmitter(); Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
    let launched; const lines = [];
    startAgent({ agent: 'claude', provider: { type: 'claude', command: process.execPath }, mode: 'edit', repo: '/task/worktree', prompt: 'Test' },
      (_, line) => lines.push(line), () => {}, (_, args, options) => { launched = { args, options }; return child; });
    child.stdout.write(JSON.stringify({ type: 'result', permission_denials: [{ tool_name: 'MCP', tool_input: { token: 'private-mcp-credential' } }] }) + '\n');
    child.emit('close', 0);
    assert.equal(launched.options.cwd, '/task/worktree');
    assert.equal(launched.options.env.ATELIER_TEST_MCP_TOKEN, 'private-mcp-credential');
    assert.ok(!JSON.stringify(launched.args).includes('private-mcp-credential'));
    assert.match(eventText(lines[0]), /\[secret\]/);
    assert.ok(!lines[0].includes('private-mcp-credential'));
  } finally {
    if (previous === undefined) delete process.env.ATELIER_TEST_MCP_TOKEN;
    else process.env.ATELIER_TEST_MCP_TOKEN = previous;
  }
});

test('executable lookup ignores the current directory, relative PATH entries and Windows wrappers', () => {
  assert.throws(() => resolveExecutable(basename(process.execPath), { PATH: '.;relative' }), /introuvable/);
  assert.equal(resolveExecutable(basename(process.execPath), { PATH: dirname(process.execPath) }), process.execPath);
  assert.equal(resolveExecutable(process.execPath, {}), process.execPath);
  if (process.platform === 'win32') {
    for (const command of ['C:\\test.cmd', 'C:\\test.bat', '\\\\server\\share\\agent.exe', '\\\\?\\C:\\agent.exe']) assert.throws(() => resolveExecutable(command, {}), /introuvable/);
  }
  for (const path of ['\\\\server\\share', '//server/share', '\\\\?\\C:\\device']) {
    assert.throws(() => providerSettings({ id: 'test', name: 'Test', type: 'codex', command: path }));
    assert.throws(() => providerSettings({ id: 'test', name: 'Test', type: 'codex', configDir: path }));
  }
  for (const path of ['/\\server/share', '\\/server/share']) {
    assert.throws(() => providerSettings({ id: 'test', name: 'Test', type: 'codex', configDir: path }));
    assert.throws(() => providerSettings({ id: 'test', name: 'Test', type: 'codex', command: path }));
  }
});

test('malformed CLI events cannot crash progress or logging', () => {
  for (const event of [null, 12, [], { message: { content: {} } }, { message: { content: [null] } }, { type: 'result', result: {} }, { type: 'error', message: {} }, { item: { type: 'file_change', changes: [null] } }]) {
    const line = JSON.stringify(event);
    assert.doesNotThrow(() => eventProgress(line));
    assert.equal(typeof eventText(line), 'string');
  }
});

test('Claude result logs preserve the summary and identify denied tools and inputs', () => {
  const event = { type: 'result', result: 'Fichier créé.', permission_denials: [
    { tool_name: 'Bash', tool_input: { command: 'git status --short' } },
    { tool_name: 'Write', tool_input: { file_path: '/protected/file' } },
  ] };
  assert.equal(eventText(JSON.stringify(event)), 'Fichier créé.\nPermission refusée : Bash · {"command":"git status --short"}\nPermission refusée : Write · {"file_path":"/protected/file"}');
  assert.match(eventText(JSON.stringify({ ...event, result: undefined })), /^Permission refusée : Bash/);
  assert.equal(eventText(JSON.stringify({ ...event, permission_denials: [] })), 'Fichier créé.');
  assert.equal(eventText(JSON.stringify({ type: 'result', errors: ['Échec'] })), 'Échec');
  assert.match(eventText(JSON.stringify({ type: 'result', permission_denials: [null] })), /outil inconnu/);
});

test('credential readers bound local files and Codex exposes only the selected identity', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'kanban-credential-bounds-'));
  try {
    for (const path of ['/\\server/share', '\\/server/share', '//server/share', 'relative']) assert.equal(await readAgentCredentials(path, 'auth.json'), null);
    mkdirSync(join(directory, 'special.json'));
    assert.equal(await readAgentCredentials(directory, 'special.json').catch(() => null), null);
    const authPath = join(directory, 'auth.json');
    const provider = { type: 'codex', auth: 'cli', command: process.execPath, configDir: directory };
    const probe = () => agentStatus('codex', provider, '', async (_, args) => ({ stdout: args[0] === '--version' ? '1.0.0' : 'Logged in using ChatGPT', stderr: '' }), async () => '1.0.0');
    const claim = Buffer.from(JSON.stringify({ email: 'test@example.com' })).toString('base64url');
    writeFileSync(authPath, JSON.stringify({ tokens: { id_token: 'header.' + claim + '.signature', access_token: 'private-token' } }));
    const status = await probe();
    assert.equal(status.account, 'test@example.com'); assert.ok(!JSON.stringify(status).includes('private-token'));
    writeFileSync(authPath, ' '.repeat(65537));
    assert.equal(await readAgentCredentials(directory, 'auth.json'), null);
    assert.equal((await probe()).account, '');
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('version checks do not receive API keys or leak echoed secrets', async () => {
  let calls = 0;
  const secret = 'private-credential';
  const status = await agentStatus('api', { type: 'codex', auth: 'api', command: process.execPath }, secret, async (binary, args, options) => {
    calls++;
    assert.equal(options.env.CODEX_API_KEY, undefined);
    assert.equal(options.env.OPENAI_API_KEY, undefined);
    assert.equal(options.cwd, dirname(process.execPath));
    return { stdout: '1.0.0 ' + secret, stderr: '' };
  }, async () => '1.1.0');
  assert.equal(calls, 1); assert.equal(status.configured, true);
  assert.ok(!JSON.stringify(status).includes(secret)); assert.equal(status.updateState, 'outdated');
});

test('account fields redact credentials before truncating display text', async () => {
  const previous = process.env.ANTHROPIC_API_KEY;
  process.env.ANTHROPIC_API_KEY = 'private-credential';
  try {
    const status = await agentStatus('claude', { type: 'claude', auth: 'cli', command: process.execPath }, '', async (binary, args) => ({
      stdout: args[0] === '--version' ? '1.0.0' : JSON.stringify({ loggedIn: true, email: 'x'.repeat(192) + 'private-credential', authMethod: 'x'.repeat(92) + 'private-credential' }), stderr: '',
    }), async () => '1.0.0');
    assert.ok(status.connected);
    assert.ok(!JSON.stringify(status).includes('private-'));
    assert.ok(status.account.endsWith('[secret]'));
    assert.ok(status.authMethod.endsWith('[secret]'));
  } finally {
    if (previous === undefined) delete process.env.ANTHROPIC_API_KEY;
    else process.env.ANTHROPIC_API_KEY = previous;
  }
});

test('split credentials and oversized output never reach logs or summaries in plaintext', () => {
  const child = new EventEmitter(); Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  const lines = [], secret = 'private-credential';
  startAgent({ agent: 'api', provider: { type: 'codex', auth: 'api', command: process.execPath }, apiKey: secret, mode: 'analyze', repo: '.', prompt: '' }, (stream, line) => lines.push({ stream, line }), () => {}, () => child);
  child.stdout.write('x'.repeat(65536) + 'private-');
  assert.equal(lines.length, 0);
  child.stdout.write('credential\n');
  assert.ok(lines[0].line.endsWith('[secret]'));
  child.stderr.write('x'.repeat(1024 * 1024) + 'private-');
  child.stderr.write('credential\nNext line\n');
  child.emit('close', 0);
  assert.ok(!JSON.stringify(lines).includes('private-'));
  assert.ok(!JSON.stringify(lines).includes('credential'));
  assert.equal(lines.at(-1).line, 'Next line');
  assert.equal(lines.filter(entry => entry.stream === 'system').length, 1);
});

test('custom probes require an authenticated explicit POST and invalidate cached identity after edits', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'kanban-provider-security-'));
  const calls = [], accountCalls = [];
  const app = createApp({ dataDir: directory, protectTokens: value => value,
    readAgentAccount: async provider => { accountCalls.push(provider.id); return { subscription: 'Pro', quotaState: 'available', quotas: [{ remainingPercent: 60 }] }; },
    probe: async (id, provider) => { calls.push(id); return { installed: true, connected: true, version: '1.0.0', account: provider.accountLabel }; } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = 'http://127.0.0.1:' + app.server.address().port;
    const headers = { Authorization: 'Bearer ' + app.authToken, 'X-Local-Request': '1', 'Content-Type': 'application/json' };
    const request = (path, method = 'GET', body) => fetch(url + path, { method, headers, ...(body ? { body: JSON.stringify(body) } : {}) });
    const profile = { id: 'custom', name: 'Custom', type: 'command', command: process.execPath, accountLabel: 'account-one' };
    assert.equal((await request('/api/agent-providers', 'POST', profile)).status, 200);
    assert.equal((await request('/api/agent-providers', 'POST', { id: 'override', name: 'Override', type: 'codex', command: process.execPath })).status, 200);
    const automatic = await (await request('/api/agents?refresh=1')).json();
    assert.equal(automatic.custom.manualCheckRequired, true); assert.equal(automatic.override.manualCheckRequired, true);
    assert.deepEqual(calls, ['codex', 'claude']);
    assert.equal((await request('/api/agents/custom/account')).status, 404);
    assert.equal((await fetch(url + '/api/agents/custom/account', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })).status, 401);
    assert.equal((await fetch(url + '/api/agents/custom/account', { method: 'POST', headers: { ...headers, Origin: 'https://attacker.invalid' }, body: '{}' })).status, 403);
    assert.deepEqual(accountCalls, []);
    assert.equal((await fetch(url + '/api/agents/check', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })).status, 401);
    assert.equal((await fetch(url + '/api/agents/check', { method: 'POST', headers: { ...headers, Origin: 'https://attacker.invalid' }, body: '{}' })).status, 403);
    assert.equal((await fetch(url + '/api/agents/check', { method: 'POST', headers: { Authorization: headers.Authorization, 'Content-Type': 'application/json' }, body: '{}' })).status, 403);
    assert.deepEqual(calls, ['codex', 'claude']);
    const checked = await (await request('/api/agents/check', 'POST', {})).json();
    assert.equal(checked.custom.account, 'account-one'); assert.ok(checked.custom.checkedAt);
    assert.ok(calls.includes('custom')); assert.ok(calls.includes('override'));
    assert.equal((await (await request('/api/agents')).json()).custom.version, '1.0.0');
    const account = await (await request('/api/agents/custom/account', 'POST', {})).json();
    assert.equal(account.subscription, 'Pro'); assert.equal(account.quotas[0].remainingPercent, 60);
    await request('/api/agents/custom/account', 'POST', {});
    assert.deepEqual(accountCalls, ['custom']);
    await request('/api/agent-providers', 'POST', { ...profile, accountLabel: 'account-two' });
    assert.equal((await (await request('/api/agents')).json()).custom.manualCheckRequired, true);
    assert.equal((await (await request('/api/agents/custom/account', 'POST', {})).json()).account, 'account-two');
    assert.deepEqual(accountCalls, ['custom', 'custom']);
  } finally { await app.close(); rmSync(directory, { recursive: true, force: true }); }
});

test('an account read prevents restoring credentials or editing its profile until it finishes', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-account-lock-'));
  let release, entered;
  const reading = new Promise(resolve => entered = resolve);
  const app = createApp({ dataDir, protectTokens: value => value, probe: async () => ({ installed: true }),
    readAgentAccount: async () => { entered(); await new Promise(resolve => release = resolve); return { quotas: [] }; } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const request = (path, body) => fetch('http://127.0.0.1:' + app.server.address().port + path, { method: 'POST', headers: { Authorization: 'Bearer ' + app.authToken, 'X-Local-Request': '1', 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    const backup = await (await request('/api/backups', {})).json();
    const pending = request('/api/agents/codex/account', {}); await reading;
    assert.equal((await request('/api/backups/restore', { confirmed: true, name: backup.name })).status, 409);
    assert.equal((await request('/api/agent-providers', { id: 'codex', type: 'codex', name: 'Other profile' })).status, 409);
    release(); assert.equal((await pending).status, 200);
    assert.equal(app.store.settings().credentialsReset, undefined);
  } finally { release?.(); await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});
