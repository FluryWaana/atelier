import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { commandArgs, startAgent } from '../lib/agents.mjs';
import { agentOptions, resolveAgentOptions } from '../public/agent-options.mjs';
import { createApp } from '../server.mjs';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';

test('CLI options preserve permissions and use each agent format', () => {
  const codex = commandArgs('codex', 'analyze', { model: 'custom/model', effort: 'xhigh' });
  assert.equal(codex[codex.indexOf('--model') + 1], 'custom/model');
  assert.equal(codex[codex.indexOf('-c') + 1], 'model_reasoning_effort="xhigh"');
  assert.equal(codex[codex.indexOf('-s') + 1], 'read-only');
  assert.equal(codex.at(-1), '-');
  const claude = commandArgs('claude', 'edit', { model: 'opus', effort: 'high' });
  assert.equal(claude[claude.indexOf('--model') + 1], 'opus');
  assert.equal(claude[claude.indexOf('--effort') + 1], 'high');
  assert.equal(claude[claude.indexOf('--permission-mode') + 1], 'acceptEdits');
  for (const mode of ['analyze', 'edit']) {
    const args = commandArgs('claude', mode);
    assert.equal(args[args.indexOf('--permission-mode') + 1], mode === 'edit' ? 'acceptEdits' : 'plan');
    assert.equal(args[args.indexOf('--permission-prompts') + 1], 'none');
    assert.deepEqual(args.slice(args.indexOf('--allowedTools') + 1), ['Bash(git status:*)', 'Bash(git branch --show-current)']);
  }
  for (const agent of ['codex', 'claude']) {
    assert.ok(!commandArgs(agent, 'edit').includes('--model'));
    for (const model of ['--unsafe', 'a\nb', 'a"b', 42, null]) assert.throws(() => agentOptions(agent, { model }));
  }
  assert.throws(() => agentOptions('claude', { effort: 'minimal' }));
  assert.throws(() => agentOptions('codex', { effort: 'max' }));
});

test('task and launch options override only the selected agent defaults', () => {
  const settings = { codexModel: 'codex-default', codexEffort: 'medium', claudeModel: 'sonnet', claudeEffort: 'low' };
  const card = { agent: 'codex', model: 'task-model', effort: 'high' };
  assert.deepEqual(resolveAgentOptions(settings, {}, { agent: 'codex' }), { model: 'codex-default', effort: 'medium' });
  assert.deepEqual(resolveAgentOptions(settings, card, { agent: 'codex' }), { model: 'task-model', effort: 'high' });
  assert.deepEqual(resolveAgentOptions(settings, card, { agent: 'claude' }), { model: 'sonnet', effort: 'low' });
  assert.deepEqual(resolveAgentOptions(settings, card, { agent: 'codex', model: 'one-run', effort: '' }), { model: 'one-run', effort: 'medium' });
});

test('startAgent forwards model and effort to the spawned process', () => {
  const child = new EventEmitter();
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  let args;
  startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath }, mode: 'edit', model: 'custom', effort: 'low', repo: '.', prompt: 'Test' }, () => {}, () => {}, (binary, argv) => { args = argv; return child; });
  child.emit('close', 0);
  assert.equal(args[args.indexOf('--model') + 1], 'custom');
  assert.ok(args.includes('model_reasoning_effort="low"'));
});

test('API persists defaults and task overrides through launches, copies and agent changes', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'kanban-options-')), repo = join(dir, 'repo');
  mkdirSync(repo);
  execFileSync('git', ['init', repo], { windowsHide: true });
  execFileSync('git', ['-C', repo, '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '--allow-empty', '-m', 'Initial'], { windowsHide: true });
  execFileSync('git', ['-C', repo, 'remote', 'add', 'origin', repo], { windowsHide: true });
  const launches = [];
  const app = createApp({ protectTokens: value => value, dataDir: join(dir, 'data'), launch: (options, line, end) => {
    launches.push(options);
    if (options.agent === 'codex') line('stdout', JSON.stringify({ type: 'turn.completed', usage: { input_tokens: 1000, cached_input_tokens: 800, output_tokens: 100 } }));
    else line('stdout', JSON.stringify({ type: 'result', total_cost_usd: 0.12, usage: { input_tokens: 100, output_tokens: 20 } }));
    end(0); return { stop() {} };
  } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
    assert.equal((await fetch(url + '/agent-options.mjs')).status, 200);
    const settings = (await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).settings;
    const defaults = { ...settings, codexModel: 'custom-codex', codexEffort: 'high', claudeModel: 'sonnet', claudeEffort: 'medium' };
    assert.equal((await request('/api/settings', 'PUT', defaults)).status, 200);
    assert.equal((await request('/api/settings', 'PUT', settings)).status, 200);
    assert.equal(app.store.settings().codexModel, 'custom-codex');
    assert.equal((await request('/api/settings', 'PUT', { ...defaults, claudeEffort: 'minimal' })).status, 400);
    assert.equal((await request('/api/cards', 'POST', { title: 'Invalid', model: '--unsafe' })).status, 400);
    const card = await (await request('/api/cards', 'POST', { title: 'Options', repo })).json();
    const path = '/api/cards/' + card.id;
    assert.equal((await request(path + '/run', 'POST', {})).status, 200);
    assert.equal(launches.at(-1).model, 'custom-codex');
    assert.equal(app.store.runs()[0].effort, 'high');
    assert.equal(app.store.runs()[0].usage.totalTokens, 1100);
    assert.equal(app.store.runs()[0].usage.costUsd, null);
    assert.equal((await (await fetch(url + '/api/state', { headers: { Authorization: 'Bearer ' + app.authToken } })).json()).runs[0].usage.pricingModel, 'custom-codex');
    assert.equal((await request(path, 'PATCH', { model: 'task-model', effort: 'low', status: 'doing' })).status, 200);
    assert.equal(launches.at(-1).model, 'task-model');
    assert.equal(launches.at(-1).effort, 'low');
    const copy = await (await request(path + '/duplicate', 'POST', {})).json();
    assert.equal(copy.model, 'task-model'); assert.equal(copy.effort, 'low');
    assert.equal((await request(path, 'PATCH', { agent: 'claude' })).status, 200);
    assert.equal(app.store.card(card.id).model, '');
    assert.equal((await request(path + '/run', 'POST', {})).status, 200);
    assert.equal(launches.at(-1).model, 'sonnet');
    assert.equal(launches.at(-1).effort, 'medium');
    assert.equal(app.store.runs()[0].usage.costUsd, 0.12);
    assert.equal((await request(path + '/run', 'POST', { effort: 'minimal' })).status, 400);
  } finally {
    await app.close();  
    rmSync(dir, { recursive: true, force: true });
  }
});
