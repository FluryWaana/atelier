import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { createAgentProtocol } from '../lib/agent-interactions.mjs';
import { startAgent } from '../lib/agents.mjs';
import { createRuns } from '../lib/application/runs.mjs';
import { runRoute } from '../lib/http/run-routes.mjs';
import { agentState } from '../public/view-helpers.mjs';
import { Readable } from 'node:stream';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

function protocol(type = 'codex', options = {}) {
  const sent = [], events = [], pauses = [], completed = [];
  const adapter = createAgentProtocol({ type, mode: 'edit', repo: '/repo', prompt: 'Task', send: message => sent.push(message), emit: event => events.push(event), complete: code => completed.push(code), waiting: value => pauses.push(value), ...options });
  return { adapter, sent, events, pauses, completed, request: () => events.findLast(event => event.type === 'interaction').requests.at(-1) };
}

test('Codex routes multiple pending questions and permissions without broadening grants', () => {
  const { adapter, sent, request, events } = protocol();
  adapter.receive({ id: 7, method: 'item/tool/requestUserInput', params: { questions: [{ id: 'choice', question: 'Which?', options: [{ label: 'A' }] }] } });
  const question = request();
  adapter.receive({ id: 8, method: 'item/permissions/requestApproval', params: { permissions: { network: { enabled: true } } } });
  const approval = request();
  adapter.reply(question.id, { decision: 'answer', answers: { 0: ['Other'] } });
  assert.deepEqual(sent.at(-1), { id: 7, result: { answers: { choice: { answers: ['Other'] } } } });
  assert.equal(events.at(-1).requests.length, 1);
  adapter.reply(approval.id, { decision: 'allow' });
  assert.deepEqual(sent.at(-1), { id: 8, result: { permissions: { network: { enabled: true } }, scope: 'turn' } });
  assert.equal(events.at(-1).requests.length, 0);
  assert.throws(() => adapter.reply(question.id, { decision: 'allow' }), /attente/);
});

test('Codex cancellation invalidates a request and command approvals preserve details', () => {
  const { adapter, sent, request } = protocol();
  adapter.receive({ method: 'item/started', params: { item: { id: 'file', type: 'fileChange', changes: [{ path: '/repo/a', diff: '+new' }] } } });
  adapter.receive({ id: 'file-approval', method: 'item/fileChange/requestApproval', params: { itemId: 'file', reason: 'Write file' } });
  assert.match(request().detail, /\+new/);
  const id = request().id;
  adapter.receive({ method: 'serverRequest/resolved', params: { requestId: 'file-approval' } });
  assert.throws(() => adapter.reply(id, { decision: 'allow' }), /attente/);
  adapter.receive({ id: 'cmd', method: 'item/commandExecution/requestApproval', params: { command: 'git status', availableDecisions: ['decline'] } });
  assert.equal(request().canAllow, false);
  adapter.reply(request().id, { decision: 'deny' });
  assert.deepEqual(sent.at(-1), { id: 'cmd', result: { decision: 'decline' } });
});

test('Codex preserves summaries, progress, usage and terminal errors', () => {
  const { adapter, events, completed } = protocol();
  adapter.receive({ method: 'item/completed', params: { item: { type: 'agentMessage', text: 'Done' } } });
  adapter.receive({ method: 'thread/tokenUsage/updated', params: { tokenUsage: { total: { inputTokens: 100, cachedInputTokens: 80, outputTokens: 20 } } } });
  adapter.receive({ method: 'turn/completed', params: { turn: { status: 'completed' } } });
  assert.equal(events[0].item.text, 'Done');
  assert.equal(events[0].item.type, 'agent_message');
  assert.deepEqual(events.at(-1).usage, { input_tokens: 100, cached_input_tokens: 80, output_tokens: 20 });
  assert.deepEqual(completed, [0]);
  adapter.receive({ method: 'turn/completed', params: { turn: { status: 'failed', error: { message: 'Offline' } } } });
  assert.equal(events.at(-1).error.message, 'Offline');
  assert.deepEqual(completed, [0, 1]);
});

test('Codex API profiles log in on the private channel before starting a thread', () => {
  const { adapter, sent } = protocol('codex', { apiKey: 'test-private-key' });
  adapter.start();
  adapter.receive({ id: 'initialize', result: {} });
  assert.deepEqual(sent.at(-1), { id: 'login', method: 'account/login/start', params: { type: 'apiKey', apiKey: 'test-private-key' } });
  assert.ok(!sent.some(message => message.method === 'thread/start'));
  adapter.receive({ id: 'login', result: { type: 'apiKey' } });
  assert.equal(sent.at(-1).method, 'thread/start');
});

test('a child thread cannot finish the main run, but its approvals can be resolved', () => {
  const { adapter, completed, request, events } = protocol();
  adapter.receive({ id: 'thread', result: { thread: { id: 'main' } } });
  adapter.receive({ method: 'turn/completed', params: { threadId: 'child', turn: { status: 'completed' } } });
  assert.deepEqual(completed, []);
  adapter.receive({ id: 1, method: 'item/commandExecution/requestApproval', params: { threadId: 'child', command: 'test' } });
  assert.ok(request());
  adapter.receive({ method: 'serverRequest/resolved', params: { threadId: 'child', requestId: 1 } });
  assert.equal(events.at(-1).requests.length, 0);
  adapter.receive({ method: 'turn/completed', params: { threadId: 'main', turn: { status: 'completed' } } });
  assert.deepEqual(completed, [0]);
});

test('file approvals show the matching thread changes and discard completed items', () => {
  const { adapter, request, events, sent, completed } = protocol();
  adapter.receive({ id: 'thread', result: { thread: { id: 'main' } } });
  for (const threadId of ['main', 'child']) adapter.receive({ method: 'item/started', params: { threadId, item: { id: 'same-id', type: 'fileChange', changes: [{ path: `/repo/${threadId}.txt`, diff: `-${threadId} contents` }] } } });
  assert.equal(events.filter(event => event.item).length, 1, 'child progress does not become the main result');
  for (const threadId of ['main', 'child']) {
    adapter.receive({ id: threadId, method: 'item/fileChange/requestApproval', params: { threadId, itemId: 'same-id', reason: 'Update file' } });
    const approval = request();
    assert.equal(approval.canAllow, true);
    assert.deepEqual(JSON.parse(approval.detail).changes, [{ path: `/repo/${threadId}.txt`, diff: `-${threadId} contents` }]);
    adapter.reply(approval.id, { decision: 'allow' });
    assert.deepEqual(sent.at(-1), { id: threadId, result: { decision: 'accept' } });
  }
  adapter.receive({ method: 'item/completed', params: { threadId: 'child', item: { id: 'same-id', type: 'fileChange' } } });
  adapter.receive({ id: 'missing-details', method: 'item/fileChange/requestApproval', params: { threadId: 'child', itemId: 'same-id' } });
  assert.equal(request().canAllow, false, 'missing details cannot authorize a file change');
  assert.deepEqual(completed, []);
});

test('Claude initializes streaming input and answers questions with original inputs', () => {
  const { adapter, sent, request, completed } = protocol('claude');
  adapter.start();
  assert.equal(sent[0].request.subtype, 'initialize');
  adapter.receive({ type: 'control_response', response: { request_id: 'initialize', subtype: 'success', response: {} } });
  assert.equal(sent.at(-1).message.content, 'Task');
  const input = { questions: [{ question: 'Which?', multiSelect: true, options: [{ label: 'A' }, { label: 'B' }] }] };
  adapter.receive({ type: 'control_request', request_id: 'q', request: { subtype: 'can_use_tool', tool_name: 'AskUserQuestion', input } });
  adapter.reply(request().id, { decision: 'answer', answers: { 0: ['A', 'B'] } });
  assert.deepEqual(sent.at(-1).response.response, { behavior: 'allow', updatedInput: { ...input, answers: { 'Which?': ['A', 'B'] } } });
  adapter.receive({ type: 'result', result: 'Done' });
  assert.deepEqual(completed, [0]);
});

test('Claude denial and cancellation do not approve a tool or leave a pending request', () => {
  const { adapter, sent, request } = protocol('claude', { mode: 'analyze' });
  adapter.receive({ type: 'control_request', request_id: 'a', request: { subtype: 'can_use_tool', tool_name: 'ExitPlanMode', input: {} } });
  assert.equal(request().canAllow, false);
  adapter.reply(request().id, { decision: 'deny' });
  assert.equal(sent.at(-1).response.response.behavior, 'deny');
  adapter.receive({ type: 'control_request', request_id: 'b', request: { subtype: 'can_use_tool', tool_name: 'Bash', input: { command: 'echo test' } } });
  const id = request().id;
  adapter.receive({ type: 'control_cancel_request', request_id: 'b' });
  assert.throws(() => adapter.reply(id, {}));
});

test('waiting pauses the work timeout, redacts tool input, and keeps the input channel open', t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'] });
  const child = new EventEmitter();
  let exit, stops = 0;
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill() { stops++; child.emit('close', 1); } });
  const lines = [];
  const handle = startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath, auth: 'api' }, apiKey: 'private-credential', mode: 'edit', repo: '.', prompt: 'Task' }, (_, line) => lines.push(line), code => { exit = code; }, () => child);
  child.stdin.read();
  t.mock.timers.tick(30 * 60 * 1000);
  const message = JSON.stringify({ id: 1, method: 'item/commandExecution/requestApproval', params: { command: 'echo private-credential' } });
  child.stdout.write(message.slice(0, -8));
  assert.equal(lines.length, 0);
  child.stdout.write(message.slice(-8) + '\n');
  const request = JSON.parse(lines.at(-1)).requests[0];
  assert.ok(!JSON.stringify(lines).includes('private-credential'));
  assert.match(request.detail, /\[secret\]/);
  assert.equal(child.stdin.writableEnded, false);
  t.mock.timers.tick(2 * 60 * 60 * 1000);
  assert.equal(stops, 0);
  handle.reply(request.id, { decision: 'allow' });
  assert.deepEqual(JSON.parse(child.stdin.read().toString()), { id: 1, result: { decision: 'accept' } });
  t.mock.timers.tick(30 * 60 * 1000);
  assert.equal(stops, 1); assert.equal(exit, 1);
});

test('optional questions do not pause the work deadline, including after a blocking request resolves', t => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'] });
  const child = new EventEmitter(); let exit, stops = 0;
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill() { stops++; child.emit('close', 1); } });
  const events = [];
  const handle = startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath }, mode: 'edit', repo: '.', prompt: 'Task' }, (_, line) => { try { events.push(JSON.parse(line)); } catch {} }, code => { exit = code; }, () => child);
  t.after(() => child.emit('close', 1));
  const receive = event => child.stdout.write(JSON.stringify(event) + '\n');
  receive({ id: 'optional', method: 'item/tool/requestUserInput', params: { isBlocking: false, questions: [{ id: 'q', question: 'Optional?' }] } });
  assert.equal(events.at(-1).requests.length, 1, 'optional questions remain answerable');
  t.mock.timers.tick(30 * 60 * 1000);
  receive({ id: 'blocking', method: 'item/tool/requestUserInput', params: { isBlocking: true, questions: [{ id: 'q', question: 'Required?' }] } });
  const blocking = events.at(-1).requests.at(-1);
  t.mock.timers.tick(2 * 60 * 60 * 1000);
  assert.equal(stops, 0);
  handle.reply(blocking.id, { decision: 'answer', answers: { 0: ['Continue'] } });
  assert.equal(events.at(-1).requests.length, 1);
  t.mock.timers.tick(30 * 60 * 1000);
  assert.equal(stops, 1); assert.equal(exit, 1);
});

test('a CLI exiting successfully without a final result is not considered complete', () => {
  const child = new EventEmitter();
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  let exit;
  startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath }, mode: 'analyze', repo: '.', prompt: 'Task' }, () => {}, code => { exit = code; }, () => child);
  child.emit('close', 0);
  assert.equal(exit, 1);
});

test('secret answers are masked if the agent echoes them later', () => {
  const child = new EventEmitter();
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  const lines = [];
  const handle = startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath }, mode: 'analyze', repo: '.', prompt: 'Task' }, (_, line) => lines.push(line), () => {}, () => child);
  child.stdout.write(JSON.stringify({ id: 1, method: 'item/tool/requestUserInput', params: { questions: [{ id: 'secret', question: 'Secret?', isSecret: true }] } }) + '\n');
  const request = JSON.parse(lines.at(-1)).requests[0];
  handle.reply(request.id, { decision: 'answer', answers: { 0: ['secret-answer'] } });
  child.stdout.write(JSON.stringify({ method: 'item/completed', params: { item: { type: 'agentMessage', text: 'Echo secret-answer' } } }) + '\n');
  child.emit('close', 1);
  assert.ok(!lines.join('\n').includes('secret-answer'));
  assert.match(lines.at(-1), /\[secret\]/);
});

test('redaction preserves event metadata and request IDs even for short or structural secrets', t => {
  for (const secret of ['type', 'id', 'question', 'interaction', '0', 'e', '"']) {
    const child = new EventEmitter();
    Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
    const events = [];
    const handle = startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath }, mode: 'analyze', repo: '.', prompt: 'Task' }, (_, line) => events.push(JSON.parse(line)), () => {}, () => child);
    t.after(() => child.emit('close', 1));
    const receive = event => child.stdout.write(JSON.stringify(event) + '\n');
    receive({ id: 1, method: 'item/tool/requestUserInput', params: { questions: [{ id: 'secret', question: 'Secret?', isSecret: true }] } });
    handle.reply(events.at(-1).requests[0].id, { decision: 'answer', answers: { 0: [secret] } });
    assert.deepEqual(events.at(-1), { type: 'interaction', requests: [] }, secret);
    receive({ id: 2, method: 'item/tool/requestUserInput', params: { questions: [{ id: 'next', question: secret, isSecret: true, options: [{ label: secret, description: secret }] }] } });
    const question = events.at(-1).requests[0];
    assert.equal(question.kind, 'question');
    assert.equal(question.questions[0].id, '0');
    assert.equal(question.questions[0].question, '[secret]');
    assert.equal(question.questions[0].options[0].label, '[secret]');
    assert.equal(question.questions[0].isSecret, true);
    assert.doesNotThrow(() => handle.reply(question.id, { decision: 'deny' }));
    receive({ method: 'item/completed', params: { item: { id: 'id-0', type: 'agentMessage', text: secret } } });
    assert.equal(events.at(-1).type, 'item.completed');
    assert.deepEqual(events.at(-1).item, { id: 'id-0', type: 'agent_message', text: '[secret]' });
    receive({ method: 'item/completed', params: { item: { id: 'id-0', type: 'fileChange', changes: [{ path: secret, kind: { type: 'update', move_path: secret }, diff: secret }] } } });
    assert.deepEqual(events.at(-1).item.changes, [{ path: '[secret]', kind: { type: 'update', move_path: '[secret]' }, diff: '[secret]' }]);
    child.emit('close', 1);
  }
});

test('Claude tool payloads redact keys and values without corrupting result or message metadata', t => {
  const child = new EventEmitter();
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  const events = [];
  startAgent({ agent: 'claude', provider: { type: 'claude', command: process.execPath }, apiKey: 'type', mode: 'analyze', repo: '.', prompt: 'Task' }, (_, line) => events.push(JSON.parse(line)), () => {}, () => child);
  t.after(() => child.emit('close', 1));
  child.stdout.write(JSON.stringify({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'tool_use', id: 'type', name: 'Read', input: { type: 'type', id: 'type' } }] } }) + '\n');
  assert.deepEqual(events.at(-1).message.content[0], { type: 'tool_use', id: 'type', name: 'Read', input: { '[secret]': '[secret]', id: '[secret]' } });
  child.stdout.write(JSON.stringify({ type: 'result', result: 'type', permission_denials: [{ tool_name: 'Read', tool_input: { type: 'type' } }] }) + '\n');
  assert.equal(events.at(-1).type, 'result');
  assert.equal(events.at(-1).result, '[secret]');
  assert.deepEqual(events.at(-1).permission_denials[0].tool_input, { '[secret]': '[secret]' });
  child.emit('close', 0);
});

test('invalid protocol output fails without leaking a JSON parser excerpt', () => {
  const child = new EventEmitter();
  Object.assign(child, { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill() { child.emit('close', 1); } });
  const lines = []; let exit;
  startAgent({ agent: 'codex', provider: { type: 'codex', command: process.execPath, auth: 'api' }, apiKey: 'long-private-credential-value', mode: 'analyze', repo: '.', prompt: 'Task' }, (_, line) => lines.push(line), code => { exit = code; }, () => child);
  child.stdout.write('invalid long-private-credential-value\n');
  assert.equal(exit, 1);
  assert.deepEqual(lines, ['Sortie de l’agent invalide : contenu masqué.']);
});

test('run replies validate each answer, reject stale requests and keep the card occupied', async () => {
  const active = new Map(), saved = new Map(), card = { id: 'card', title: 'Task', agent: 'codex' };
  let line, end;
  const replies = [], logs = [];
  const store = { runs: () => [...saved.values()], run: id => saved.get(id), putRun: run => saved.set(run.id, structuredClone(run)), putCard() {}, card: () => card, log: (...args) => logs.push(args) };
  const runs = createRuns({ store, active, publishing: new Set(), automationBusy: () => false, checkDependencies() {}, configFor: () => ({}), samePath: (a, b) => a === b,
    prepareWorktree: async () => ({ path: '/repo', branch: 'task' }), newId: () => 'run', timestamp: () => new Date().toISOString(), changed() {}, invalidateReviews() {}, checklistText: () => '', recordAgentUsage() {}, eventProgress: () => null, eventText: () => '',
    launch(options, onLine, onEnd) { line = onLine; end = onEnd; return { stop() {}, reply(id, answer) { replies.push({ id, answer }); line('stdout', JSON.stringify({ type: 'interaction', requests: [] })); } }; },
  });
  await runs.start(card, { agent: 'codex', mode: 'edit' });
  const request = { id: 'q', kind: 'question', questions: [{ id: '0', question: 'Which?' }] };
  line('stdout', JSON.stringify({ type: 'interaction', requests: [request] }));
  assert.equal(agentState(store.run('run')), 'En attente de votre réponse');
  await assert.rejects(runs.start(card, { agent: 'codex', mode: 'edit' }), /active/);
  for (const answers of [null, {}, { 0: [] }, { 0: [''] }, { 0: ['A', 'B'] }, { 0: [5] }]) assert.throws(() => runs.reply('run', { requestId: 'q', decision: 'answer', answers }));
  assert.throws(() => runs.reply('run', { requestId: 'q', decision: 'allow' }));
  assert.equal(runRoute(runs, 'GET', '/api/runs/run/reply', {}), undefined);
  assert.deepEqual(runRoute(runs, 'POST', '/api/runs/run/reply', { requestId: 'q', decision: 'answer', answers: { 0: ['A'] } }), { ok: true });
  assert.equal(store.run('run').activity, 'Travail en cours');
  assert.equal(replies.length, 1);
  assert.throws(() => runs.reply('run', { requestId: 'q', decision: 'answer', answers: { 0: ['A'] } }), /attente/);
  line('stdout', JSON.stringify({ type: 'interaction', requests: [{ id: 'permission', kind: 'approval', canAllow: false }] }));
  assert.throws(() => runs.reply('run', { requestId: 'permission', decision: 'allow' }), /autorisation/);
  active.get('run').cancelled = true;
  assert.throws(() => runs.reply('run', { requestId: 'permission', decision: 'deny' }), /arrêtée/);
  end(0);
  assert.equal(store.run('run').status, 'cancelled');
  assert.equal(store.run('run').interactions, undefined);
  assert.equal(active.size, 0);
  assert.ok(!JSON.stringify(logs).includes('"A"'), 'reply content is not stored in logs');
});

test('authenticated HTTP replies resume the same run and reject duplicate or cancelled answers', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'atelier-replies-'));
  let line, finish, launches = 0;
  const received = [];
  const app = createApp({ dataDir, launch(options, onLine, onEnd) {
    launches++; line = onLine; finish = onEnd;
    return { stop() { finish(1); }, reply(id, answer) { received.push({ id, answer }); line('stdout', JSON.stringify({ type: 'interaction', requests: [] })); } };
  } });
  app.server.address = () => ({ port: 4317 });
  const request = (path, body = {}, headers = {}) => new Promise(resolve => {
    const req = Readable.from([JSON.stringify(body)]);
    Object.assign(req, { method: 'POST', url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1', authorization: 'Bearer ' + app.authToken, ...headers } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
  try {
    const card = await request('/api/brainstorms', { title: 'Test interactif', agent: 'codex' });
    const started = await request(`/api/cards/${card.body.id}/brainstorm`);
    assert.equal(started.status, 200);
    const path = `/api/runs/${started.body.id}/reply`, body = { requestId: 'q', decision: 'answer', answers: { 0: ['Test'] } };
    const waiting = () => line('stdout', JSON.stringify({ type: 'interaction', requests: [{ id: 'q', kind: 'question', questions: [{ id: '0', question: 'Choix ?' }] }] }));
    waiting();
    assert.equal((await request(path, body, { authorization: '' })).status, 401);
    assert.equal((await request(path, body, { 'x-local-request': '' })).status, 403);
    assert.equal(received.length, 0);
    assert.equal((await request(path, body)).status, 200);
    assert.equal(launches, 1);
    assert.equal(app.store.run(started.body.id).status, 'running');
    assert.equal((await request(path, body)).status, 409);
    assert.equal(received.length, 1);
    waiting();
    assert.equal((await request(`/api/runs/${started.body.id}/cancel`)).status, 200);
    assert.equal((await request(path, body)).status, 409);
    assert.equal(app.store.run(started.body.id).interactions, undefined);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});
