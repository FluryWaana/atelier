import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';
import { parseBrainstorm, brainstormPrompt } from '../lib/brainstorm.mjs';

const ticket = { title: 'Exporter', description: 'Exporter les tâches', criteria: ['Le CSV est téléchargeable'], questions: ['Quel séparateur ?'] };
function request(app, path, body = {}, method = 'POST') {
  return new Promise(resolve => {
    const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
    Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1', authorization: 'Bearer ' + app.authToken } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}

test('brainstorm validates bounded proposals and keeps criteria unapproved', () => {
  const result = parseBrainstorm('```json\n' + JSON.stringify({ tickets: [ticket] }) + '\n```');
  assert.deepEqual(result.proposals[0].checklist, [{ text: ticket.criteria[0], done: false }]);
  for (const tickets of [null, [], Array(9).fill(ticket), [null], [{ ...ticket, title: '' }], [{ ...ticket, title: 'x'.repeat(301) }], [{ ...ticket, description: 'x'.repeat(12001) }], [{ ...ticket, criteria: [] }], [{ ...ticket, criteria: Array(21).fill('x') }], [{ ...ticket, questions: [null] }]]) assert.throws(() => parseBrainstorm(JSON.stringify({ tickets })));
  assert.match(brainstormPrompt({ title: 'Idée' }), /lecture seule/);
});

test('brainstorm accepts a conversation without tickets and validates messages and questions', () => {
  const response = { message: ' Explorons les options. ', questions: [' Qui utilisera cet export ? '], tickets: [] };
  assert.deepEqual(parseBrainstorm(JSON.stringify(response)), { message: 'Explorons les options.', questions: ['Qui utilisera cet export ?'], proposals: [] });
  for (const change of [{ message: '' }, { message: 'x'.repeat(12001) }, { questions: null }, { questions: Array(6).fill('Question ?') }, { questions: ['x'.repeat(501)] }, { questions: [null] }, { tickets: null }]) assert.throws(() => parseBrainstorm(JSON.stringify({ ...response, ...change })));
});

test('concurrent sessions reject the same repository with a clear conflict and allow another', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-concurrent-'));
  mkdirSync(join(dataDir, 'other'));
  const app = createApp({ dataDir, launch(input, line, finish) { return { stop() { finish(1); } }; } });
  app.server.address = () => ({ port: 4317 });
  try {
    const first = await request(app, '/api/brainstorms', { title: 'Première', agent: 'codex' });
    const second = await request(app, '/api/brainstorms', { title: 'Deuxième', agent: 'codex' });
    assert.equal((await request(app, `/api/cards/${first.body.id}/brainstorm`)).status, 200);
    const conflict = await request(app, `/api/cards/${second.body.id}/brainstorm`);
    assert.equal(conflict.status, 409); assert.match(conflict.body.error, /Un agent travaille déjà/);
    await request(app, `/api/cards/${second.body.id}`, { repo: realpathSync(join(dataDir, 'other')) }, 'PATCH');
    assert.equal((await request(app, `/api/cards/${second.body.id}/brainstorm`)).status, 200);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('conversation answers survive restart, feed later turns and can become task proposals', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-conversation-'));
  let options, line, finish;
  const launch = (input, onLine, onEnd) => { options = input; line = onLine; finish = onEnd; return { stop() { finish(1); } }; };
  let app = createApp({ dataDir, launch }); app.server.address = () => ({ port: 4317 });
  try {
    const session = await request(app, '/api/brainstorms', { title: 'Export', description: 'Usage réservé aux gestionnaires.', agent: 'claude' });
    const path = `/api/cards/${session.body.id}/brainstorm`;
    const first = await request(app, path);
    line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ message: 'Un export simple suffit.', questions: ['Pour quel public ?'], tickets: [] }) })); finish(0);
    assert.deepEqual(app.store.run(first.body.id).proposals, []);
    assert.equal((await request(app, `/api/cards/${session.body.id}/brainstorm-draft`, { runId: first.body.id, index: 0 })).status, 400);
    await app.close(); app = createApp({ dataDir, launch }); app.server.address = () => ({ port: 4317 });
    assert.equal(app.store.run(first.body.id).brainstorm.message, 'Un export simple suffit.');
    await request(app, `/api/cards/${session.body.id}`, { description: 'Usage ouvert aux comptables.' }, 'PATCH');
    const next = await request(app, path, { reply: 'Pour les comptables. Propose les tâches.' });
    assert.match(options.prompt, /Un export simple suffit/); assert.match(options.prompt, /Pour quel public/); assert.match(options.prompt, /comptables/);
    assert.match(options.prompt, /réservé aux gestionnaires/); assert.match(options.prompt, /ouvert aux comptables/);
    line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ message: 'Voici le découpage.', questions: [], tickets: [ticket] }) })); finish(0);
    assert.equal((await request(app, `/api/cards/${session.body.id}/brainstorm-draft`, { runId: next.body.id, index: 0 })).status, 200);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('invalid answers remain readable, cannot create drafts and recover on the next turn', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-invalid-'));
  let line, finish;
  const app = createApp({ dataDir, launch(input, onLine, onEnd) { line = onLine; finish = onEnd; return { stop() { finish(1); } }; } });
  app.server.address = () => ({ port: 4317 });
  try {
    const session = await request(app, '/api/brainstorms', { title: 'Export', agent: 'codex' });
    const path = `/api/cards/${session.body.id}/brainstorm`;
    const first = await request(app, path);
    line('stdout', JSON.stringify({ type: 'result', result: 'Une réponse utile mais sans JSON.' })); finish(0);
    const invalid = app.store.run(first.body.id);
    assert.match(invalid.attention, /format attendu/); assert.equal(invalid.summary, 'Une réponse utile mais sans JSON.'); assert.equal(invalid.proposals, undefined);
    assert.equal((await request(app, path + '-draft', { runId: first.body.id, index: 0 })).status, 400);
    const retry = await request(app, path, { reply: 'Reprends avec le format attendu.' });
    line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ message: 'Explorons cette idée.', questions: [], tickets: [] }) })); finish(0);
    assert.equal(app.store.run(retry.body.id).attention, undefined);
    assert.equal(app.store.run(retry.body.id).brainstorm.message, 'Explorons cette idée.');
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('proposals cannot silently create a task in a different project', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-project-'));
  let line, finish;
  const app = createApp({ dataDir, launch(input, onLine, onEnd) { line = onLine; finish = onEnd; return { stop() { finish(1); } }; } });
  app.server.address = () => ({ port: 4317 });
  try {
    const session = await request(app, '/api/brainstorms', { title: 'Export', agent: 'codex' });
    const path = `/api/cards/${session.body.id}`;
    const run = await request(app, path + '/brainstorm');
    line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ tickets: [ticket] }) })); finish(0);
    await request(app, path, { repo: realpathSync(dataDir) }, 'PATCH');
    const draft = { runId: run.body.id, index: 0 };
    const conflict = await request(app, path + '/brainstorm-draft', draft);
    assert.equal(conflict.status, 409); assert.match(conflict.body.error, /projet a changé/);
    await request(app, path, { repo: '' }, 'PATCH');
    const created = await request(app, path + '/brainstorm-draft', draft);
    assert.equal(created.status, 200); assert.equal(created.body.repo, '');
    await request(app, path, { repo: realpathSync(dataDir) }, 'PATCH');
    assert.equal((await request(app, path + '/brainstorm-draft', draft)).body.id, created.body.id);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('stopping a turn retains the reply and allows continuing the same conversation', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-stop-'));
  let prompt;
  const app = createApp({ dataDir, launch(input, line, finish) { prompt = input.prompt; return { stop() { finish(1); } }; } });
  app.server.address = () => ({ port: 4317 });
  try {
    const session = await request(app, '/api/brainstorms', { title: 'Export', agent: 'codex' });
    const path = `/api/cards/${session.body.id}/brainstorm`;
    const run = await request(app, path, { reply: 'Utiliser UTF-8.' });
    assert.equal((await request(app, `/api/runs/${run.body.id}/cancel`)).status, 200);
    assert.equal(app.store.run(run.body.id).status, 'cancelled');
    assert.equal(app.store.run(run.body.id).instructions, 'Utiliser UTF-8.');
    assert.equal((await request(app, path, { reply: 'Et un séparateur point-virgule.' })).status, 200);
    assert.match(prompt, /Utiliser UTF-8/); assert.match(prompt, /point-virgule/);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('brainstorm runs read-only and creates one editable local draft per proposal', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-'));
  let options, line, finish;
  const app = createApp({ dataDir, launch(input, onLine, onEnd) { options = input; line = onLine; finish = onEnd; return { stop() { finish(1); } }; }, requestJira: () => assert.fail('Aucun appel Jira pendant le brainstorming') });
  app.server.address = () => ({ port: 4317 });
  try {
    app.store.putCard({ id: 'idea', title: 'Export', status: 'todo', repo: realpathSync(dataDir), agent: 'codex' });
    const run = await request(app, '/api/cards/idea/brainstorm');
    assert.equal(run.status, 200); assert.equal(run.body.kind, 'brainstorm');
    assert.equal(options.mode, 'analyze'); assert.equal(app.store.card('idea').status, 'todo');
    assert.equal(app.store.card('idea').worktree, undefined);
    assert.equal((await request(app, '/api/cards/idea/brainstorm')).status, 409);
    line('stdout', JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: JSON.stringify({ tickets: [ticket] }) } })); finish(0);
    const body = { runId: run.body.id, index: 0 };
    const draft = await request(app, '/api/cards/idea/brainstorm-draft', body);
    assert.equal(draft.status, 200); assert.equal(draft.body.title, ticket.title);
    assert.equal(draft.body.ticket, undefined); assert.match(draft.body.instructions, /séparateur/);
    assert.equal((await request(app, '/api/cards/idea/brainstorm-draft', body)).body.id, draft.body.id);
    assert.equal((await request(app, '/api/cards/idea/brainstorm-draft', { ...body, index: -1 })).status, 400);
    assert.equal((await request(app, '/api/cards/' + draft.body.id, { title: 'Titre relu' }, 'PATCH')).body.title, 'Titre relu');
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('standalone sessions need neither a ticket nor a repository and produce ordinary local tasks', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-session-'));
  let options, line, finish;
  let app = createApp({ dataDir, launch(input, onLine, onEnd) { options = input; line = onLine; finish = onEnd; return { stop() { finish(1); } }; }, requestJira: () => assert.fail('Jira est facultatif') });
  app.server.address = () => ({ port: 4317 });
  try {
    app.store.saveSettings({ repo: realpathSync(dataDir) });
    const session = await request(app, '/api/brainstorms', { title: 'Accueil client', description: 'Réduire les étapes', instructions: 'Accessibilité', agent: 'codex' });
    assert.equal(session.status, 200); assert.equal(session.body.brainstormSession, true);
    assert.equal(session.body.repo, ''); assert.equal(session.body.mode, 'analyze');
    const run = await request(app, `/api/cards/${session.body.id}/brainstorm`);
    assert.equal(run.status, 200); assert.equal(options.mode, 'analyze');
    assert.equal(options.repo, realpathSync(join(dataDir, 'brainstorm')));
    assert.match(options.prompt, /Accessibilité/);
    line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ tickets: [ticket] }) })); finish(0);
    const draft = await request(app, `/api/cards/${session.body.id}/brainstorm-draft`, { runId: run.body.id, index: 0 });
    assert.equal(draft.body.brainstormSession, undefined); assert.equal(draft.body.ticket, undefined); assert.equal(draft.body.repo, '');
    const state = await request(app, '/api/state', {}, 'GET');
    assert.equal(state.body.cards.length, 1); assert.equal(state.body.cards[0].id, draft.body.id);
    assert.equal(state.body.brainstorms.length, 1); assert.equal(state.body.brainstorms[0].id, session.body.id);
    await app.close();
    app = createApp({ dataDir }); app.server.address = () => ({ port: 4317 });
    assert.equal(app.store.card(session.body.id).brainstormSession, true);
    assert.equal(app.store.run(run.body.id).proposals[0].title, ticket.title);
    const direct = await request(app, '/api/cards', { title: 'Tâche directe', description: 'Sans agent', agent: 'codex' });
    assert.equal(direct.body.brainstormSession, undefined); assert.equal(app.store.runs().length, 1);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('brainstorm replies retain questions and previous decisions over multiple turns and restart', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-chat-'));
  let options, line, finish;
  const launch = (input, onLine, onEnd) => { options = input; line = onLine; finish = onEnd; return { stop() { finish(1); } }; };
  let app = createApp({ dataDir, launch }); app.server.address = () => ({ port: 4317 });
  const complete = questions => { line('stdout', JSON.stringify({ type: 'result', result: JSON.stringify({ tickets: [{ ...ticket, questions }] }) })); finish(0); };
  try {
    const session = await request(app, '/api/brainstorms', { title: 'Exporter', agent: 'codex' });
    const path = `/api/cards/${session.body.id}/brainstorm`;
    await request(app, path); complete(['Quel séparateur ?']);
    const second = await request(app, path, { reply: 'Utiliser le point-virgule.' });
    assert.equal(second.status, 200); assert.equal(second.body.instructions, 'Utiliser le point-virgule.');
    assert.match(options.prompt, /Quel séparateur/); assert.match(options.prompt, /point-virgule/);
    assert.equal(options.mode, 'analyze');
    assert.equal((await request(app, path, { reply: 'Doublon' })).status, 409);
    complete(['Quel encodage ?']);
    await app.close(); app = createApp({ dataDir, launch }); app.server.address = () => ({ port: 4317 });
    const invalid = await request(app, path, { reply: 'x'.repeat(20001) });
    assert.equal(invalid.status, 400); assert.equal(app.store.runs().length, 2);
    const third = await request(app, path, { reply: 'UTF-8 avec BOM.' });
    assert.equal(third.status, 200);
    assert.match(options.prompt, /point-virgule/); assert.match(options.prompt, /Quel encodage/); assert.match(options.prompt, /UTF-8 avec BOM/);
    assert.ok(options.prompt.indexOf('Quel séparateur') < options.prompt.indexOf('Quel encodage'));
    complete([]);
    assert.equal(app.store.run(third.body.id).proposals[0].questions.length, 0);
    const state = await request(app, '/api/state', {}, 'GET');
    assert.equal(state.body.brainstorms.length, 1); assert.equal(state.body.cards.length, 0);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

for (const jiraType of ['cloud', 'server']) test(`Jira ${jiraType} creation includes reviewed content, prevents duplicates and allows recovery`, async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'brainstorm-jira-'));
  const calls = []; let reject = false, release;
  const savedToken = process.env.JIRA_TOKEN;
  process.env.JIRA_TOKEN = 'fake-test-token';
  const app = createApp({ dataDir, requestJira: async (_, __, path, body) => {
    calls.push({ path, body });
    if (reject) throw new Error('Connexion perdue');
    if (release) await new Promise(resolve => { release.resolve = resolve; });
    return { id: '100', key: 'TEST-1', fields: { status: { name: 'Ouvert' } } };
  } });
  if (savedToken === undefined) delete process.env.JIRA_TOKEN; else process.env.JIRA_TOKEN = savedToken;
  app.server.address = () => ({ port: 4317 });
  const path = '/api/cards/draft/create-jira', body = { project: 'TEST', issueType: 'Story' };
  try {
    app.store.saveSettings({ jiraUrl: 'https://jira.example', jiraType });
    const card = { id: 'draft', title: 'Titre relu', description: 'Description relue', instructions: 'Question ?', checklist: [{ text: 'Critère', done: false }], status: 'todo', agent: 'codex' };
    app.store.putCard(card);
    assert.equal((await request(app, path, { ...body, project: '../bad' })).status, 400); assert.equal(calls.length, 0);
    release = {};
    const first = request(app, path, body);
    while (!release.resolve) await new Promise(resolve => setImmediate(resolve));
    assert.equal((await request(app, path, body)).status, 409);
    release.resolve(); release = null;
    assert.equal((await first).body.ticket.key, 'TEST-1');
    assert.equal((await request(app, path, body)).body.ticket.key, 'TEST-1'); assert.equal(calls.length, 1);
    assert.equal(calls[0].path, `/rest/api/${jiraType === 'server' ? '2' : '3'}/issue`);
    const fields = calls[0].body.fields;
    assert.equal(fields.summary, card.title); assert.equal(fields.issuetype.name, 'Story');
    assert.match(JSON.stringify(fields.description), /Critère/); assert.match(JSON.stringify(fields.description), /Question/);
    assert.equal(typeof fields.description, jiraType === 'server' ? 'string' : 'object');
    app.store.putCard(card); reject = true;
    assert.equal((await request(app, path, body)).status, 400);
    assert.equal(app.store.card('draft').ticketCreation.state, 'uncertain');
    const count = calls.length;
    assert.equal((await request(app, path, body)).status, 409); assert.equal(calls.length, count);
    reject = false;
    const recovered = await request(app, path, { existingKey: 'TEST-1' });
    assert.equal(recovered.body.ticket.key, 'TEST-1'); assert.equal(calls.at(-1).body, undefined);
    assert.equal(app.store.card('draft').ticketCreation, undefined);
    app.store.putCard({ ...card, archived: true });
    assert.equal((await request(app, path, body)).status, 409);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});
