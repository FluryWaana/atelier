import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { Readable } from 'node:stream';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { createApp } from '../server.mjs';

// Exercise the real HTTP handler without network access or external commands.
function request(app, path, method = 'GET', body = {}) {
  return new Promise(resolve => {
    const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
    Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1' } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}
async function fixture(t, enabled = true, assessment = 'covered') {
  const root = mkdtempSync(join(tmpdir(), 'automation-api-'));
  const repo = join(root, 'repo'), worktree = join(root, 'worktree'); mkdirSync(repo); mkdirSync(worktree);
  const source = realpathSync(repo), destination = realpathSync(worktree);
  const git = t.mock.method(childProcess, 'execFileSync', (command, args) => {
    assert.equal(command, 'git');
    if (args.includes('--show-toplevel')) return source;
    if (args.includes('list')) return `worktree ${destination}\0branch refs/heads/feature/test\0\0`;
    if (args.includes('check-ref-format')) return '';
    throw new Error('Commande Git inattendue : ' + args.join(' '));
  });
  syncBuiltinESMExports();
  const launches = [], publications = [];
  const app = createApp({ dataDir: join(root, 'data'),
    launch: (options, line, finish) => {
      launches.push(options);
      const context = options.mode === 'analyze';
      const timer = setTimeout(() => {
        const criteria = (app.store.card('ticket').checklist || []).map((item, index) => ({ index, status: assessment, evidence: 'src/app.js:12 et test/feature.test.mjs : comportement analysé, tests locaux fournis.' }));
        line('stdout', JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: context ? JSON.stringify({ summary: 'OK', findings: [], criteria }) : 'Développé' } })); finish(0);
      }, 5);
      return { stop() { clearTimeout(timer); finish(1); } };
    },
    previewPR: async () => ({ fingerprint: 'stable', title: 'Titre', body: 'Description' }),
    publishPR: async (card, options) => { publications.push(options); return { number: 1, url: 'https://example.invalid/pr/1', repository: 'org/project/repo', draft: true }; },
    reviewPR: async () => ({ source: 'a'.repeat(40), target: 'b'.repeat(40), base: 'c'.repeat(40), number: 1 }),
    verify: async (command, cwd, { onOutput }) => { assert.equal(cwd, destination); onOutput('Checks OK'); },
  });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  app.store.putProject({ id: 'project', name: 'Projet', repo: source, automationEnabled: enabled, autoFix: true, testCommand: 'npm test', prTarget: 'main' });
  app.store.putCard({ id: 'ticket', title: 'Titre', description: '', repo: source, agent: 'codex', mode: 'edit', status: 'todo', branch: 'feature/test', worktree: { path: destination, source, branch: 'feature/test' } });
  t.after(async () => { await app.close(); git.mock.restore(); syncBuiltinESMExports(); rmSync(root, { recursive: true, force: true }); });
  const wait = async () => {
    for (let i = 0; i < 100 && (app.store.card('ticket').automation?.state === 'running' || app.store.runs().some(r => r.status === 'running')); i++) await new Promise(resolve => setTimeout(resolve, 5));
    return app.store.card('ticket');
  };
  return { app, launches, publications, wait };
}

test('moving a card into doing runs the complete workflow through the real API', async t => {
  const { app, launches, publications, wait } = await fixture(t);
  const response = await request(app, '/api/cards/ticket', 'PATCH', { status: 'doing' });
  assert.equal(response.status, 200);
  assert.equal((await request(app, '/api/cards/ticket/archive', 'POST', { archived: true })).status, 409);
  assert.equal((await request(app, '/api/projects/project', 'PATCH', {})).status, 409);
  const card = await wait();
  assert.equal(card.status, 'review'); assert.equal(card.automation.state, 'ready');
  assert.equal(launches.length, 2); assert.equal(publications.length, 1);
  assert.equal(launches[1].mode, 'analyze'); assert.match(launches[1].prompt, /review de la PR/);
  assert.equal(app.store.runs().filter(r => r.review).length, 1);
  assert.equal((await request(app, '/api/state')).body.cards[0].publishing, false);
});

test('manual projects keep their existing single-run behavior', async t => {
  const { app, launches, publications, wait } = await fixture(t, false);
  const response = await request(app, '/api/cards/ticket/run', 'POST', {});
  assert.equal(response.status, 200); assert.ok(response.body.id);
  const card = await wait();
  assert.equal(card.status, 'review'); assert.equal(card.automation, undefined);
  assert.equal(launches.length, 1); assert.equal(publications.length, 0);
});

test('the queue completes automatic workflows sequentially and blocks competing manual launches', async t => {
  const { app, launches, publications } = await fixture(t);
  app.store.putCard({ ...app.store.card('ticket'), id: 'second', title: 'Seconde tâche' });
  for (const id of ['ticket', 'second']) assert.equal((await request(app, `/api/cards/${id}/queue`, 'POST')).status, 200);
  await request(app, '/api/queue', 'POST', { paused: false });
  assert.equal((await request(app, '/api/cards/second/run', 'POST')).status, 409);
  let queue;
  for (let i = 0; i < 150; i++) {
    queue = (await request(app, '/api/state')).body.queue;
    assert.ok(app.store.runs().filter(run => run.status === 'running').length <= 1);
    if (!queue.ids.length || queue.paused) break;
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.deepEqual(queue.ids, []); assert.equal(queue.current, null);
  assert.equal(launches.length, 4); assert.equal(publications.length, 2);
  assert.equal(app.store.card('ticket').automation.state, 'ready');
  assert.equal(app.store.card('second').automation.state, 'ready');
});

test('cancelling an automatic agent stops the entire workflow', async t => {
  const { app, publications, wait } = await fixture(t);
  const response = await request(app, '/api/cards/ticket/run', 'POST', {});
  const runId = response.body.automation.runId;
  assert.equal((await request(app, `/api/runs/${runId}/cancel`, 'POST')).status, 200);
  const card = await wait();
  assert.equal(card.automation.state, 'cancelled'); assert.equal(publications.length, 0);
});

test('the API stores criterion evidence and invalidates it when the requirement changes, not when a human checks it', async t => {
  const { app, launches, wait } = await fixture(t);
  await request(app, '/api/cards/ticket', 'PATCH', { description: 'Une session doit être ouverte.', checklist: [{ text: 'Refus sans mot de passe', done: false }] });
  assert.equal((await request(app, '/api/cards/ticket/run', 'POST')).status, 200);
  let card = await wait(); const report = app.store.runs().find(run => run.kind === 'pr-review');
  assert.equal(card.automation.state, 'ready'); assert.equal(card.automation.acceptance.covered, 1); assert.equal(card.checklist[0].done, false);
  assert.match(launches[1].prompt, /Une session doit être ouverte/); assert.match(launches[1].prompt, /Checks OK/);
  assert.equal(report.review.criteria[0].text, 'Refus sans mot de passe'); assert.match(report.review.criteria[0].evidence, /src\/app.js/);
  assert.equal(report.reviewContext.requirements.criteria[0], 'Refus sans mot de passe');
  await request(app, '/api/cards/ticket', 'PATCH', { checklist: [{ text: 'Refus sans mot de passe', done: true }] });
  assert.equal(app.store.card('ticket').automation.state, 'ready'); assert.equal(app.store.run(report.id).reviewStaleReason, undefined);
  await request(app, '/api/cards/ticket', 'PATCH', { checklist: [{ text: 'Autre comportement', done: true }] });
  card = app.store.card('ticket');
  assert.equal(card.automation.state, 'interrupted'); assert.match(app.store.run(report.id).reviewStaleReason, /critères ont changé/);
  await request(app, '/api/cards/ticket/check-review', 'POST', { runId: report.id });
  assert.match(app.store.run(report.id).reviewStaleReason, /critères ont changé/);
  assert.equal(app.store.run(report.id).review.criteria[0].text, 'Refus sans mot de passe');
});

test('an unverified criterion blocks automatic readiness despite passing checks and an empty findings list', async t => {
  const { app, wait } = await fixture(t, true, 'unverifiable');
  await request(app, '/api/cards/ticket', 'PATCH', { checklist: [{ text: 'Vérifier le rendu sur un vrai mobile', done: true }] });
  await request(app, '/api/cards/ticket/run', 'POST');
  const card = await wait();
  assert.equal(card.automation.tests.state, 'passed'); assert.equal(card.automation.counts.P1, 0);
  assert.equal(card.automation.acceptance.unverifiable, 1); assert.equal(card.automation.state, 'blocked');
  assert.equal(card.checklist[0].done, true);
});
