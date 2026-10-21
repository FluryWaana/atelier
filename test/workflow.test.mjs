import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';
import { openStore } from '../lib/store.mjs';
import { createBackups } from '../lib/backups.mjs';
import { createTaskQueue } from '../lib/task-queue.mjs';
import { refreshPullRequest } from '../lib/pull-requests.mjs';
import { blockers, nextAction, prLabel, cleanupCandidate, workReport } from '../public/workflow.mjs';
import { matchesFocus } from '../public/ui-helpers.mjs';

function request(app, path, method = 'GET', body = {}) {
  return new Promise(resolve => {
    const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
    Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1' } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}
function fixture(t, options = {}) {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-workflow-'));
  const app = createApp({ dataDir, protectTokens: value => value, ...options });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  t.after(async () => { await app.close(); rmSync(dataDir, { recursive: true, force: true }); });
  return { app, dataDir, request: (path, method, body) => request(app, path, method, body) };
}

test('dependencies reject invalid references, cycles and launches until prerequisites finish', async t => {
  let launches = 0;
  const { app, request } = fixture(t, { launch() { launches++; } });
  for (const id of ['a', 'b', 'c']) app.store.putCard({ id, title: id, status: 'todo', mode: 'edit', agent: 'codex' });
  for (const blockedBy of [['a'], ['missing'], ['b', 'b'], 'b', null]) assert.equal((await request('/api/cards/a', 'PATCH', { blockedBy })).status, 400);
  assert.equal((await request('/api/cards/a', 'PATCH', { blockedBy: ['b'] })).status, 200);
  assert.equal((await request('/api/cards/b', 'PATCH', { blockedBy: ['c'] })).status, 200);
  assert.equal((await request('/api/cards/c', 'PATCH', { blockedBy: ['a'] })).status, 400);
  for (const action of ['/run', '/review', '/automation', '/resume-automation']) assert.equal((await request('/api/cards/a' + action, 'POST')).status, 409);
  assert.equal((await request('/api/cards/a', 'PATCH', { status: 'doing' })).status, 409);
  assert.equal(app.store.card('a').status, 'todo'); assert.equal(launches, 0);
  assert.equal((await request('/api/cards/b', 'DELETE')).status, 409);
  await request('/api/cards/b', 'PATCH', { status: 'done' });
  assert.equal(blockers(app.store.card('a'), app.store.cards()).length, 0);
  await request('/api/cards/a', 'PATCH', { blockedBy: [] });
  assert.equal((await request('/api/cards/b', 'DELETE')).status, 200);
});

test('personal templates persist only reusable content and survive settings changes', async t => {
  const { app, request } = fixture(t);
  app.store.putCard({ id: 'source', title: 'Bug', description: 'Reproduire', instructions: 'Tester', mode: 'analyze', checklist: [{ text: 'Corrigé', done: true }], branch: 'feature/test', blockedBy: ['other'], worktree: { path: '/private' }, pullRequest: { url: 'https://example.invalid' }, ticket: { key: 'X-1' } });
  assert.equal((await request('/api/templates', 'POST', { name: '', cardId: 'source' })).status, 400);
  const saved = (await request('/api/templates', 'POST', { name: 'Régression', cardId: 'source' })).body;
  assert.equal(saved.instructions, 'Tester'); assert.deepEqual(saved.checklist, [{ text: 'Corrigé', done: false }]);
  for (const key of ['branch', 'worktree', 'pullRequest', 'jira', 'blockedBy', 'repo']) assert.equal(saved[key], undefined);
  const settings = { jiraType: 'cloud', jiraEmail: '', jql: '', jiraUrl: '' };
  assert.equal((await request('/api/settings', 'PUT', settings)).status, 200);
  assert.equal(app.store.settings().taskTemplates[0].id, saved.id);
  const card = (await request('/api/cards', 'POST', { title: 'Nouveau bug', templateId: saved.id })).body;
  assert.equal(card.description, 'Reproduire'); assert.equal(card.instructions, 'Tester'); assert.equal(card.mode, 'analyze');
  assert.deepEqual(card.checklist, saved.checklist);
  assert.equal((await request('/api/templates/' + saved.id, 'DELETE')).status, 200);
  assert.equal(app.store.card(card.id).description, 'Reproduire');
  assert.equal((await request('/api/cards', 'POST', { title: 'Bug', templateId: saved.id })).status, 400);
});

test('PR refresh preserves local work, reports errors and invalidates reviews on changed commits', async t => {
  let failed = false;
  const { app, request } = fixture(t, { refreshPR: async () => { if (failed) throw new Error('Connexion refusée'); return { status: 'completed', sourceCommit: 'new', checkedAt: new Date().toISOString() }; } });
  app.store.putCard({ id: 'a', title: 'A', status: 'review', pullRequest: { number: 1, ticketLinked: true, url: 'https://example.invalid' } });
  app.store.putRun({ id: 'review', cardId: 'a', kind: 'pr-review', reviewContext: { source: 'old' } });
  let card = (await request('/api/cards/a/refresh-pr', 'POST')).body;
  assert.equal(card.status, 'review'); assert.equal(card.pullRequest.status, 'completed'); assert.equal(card.pullRequest.ticketLinked, true);
  assert.match(app.store.run('review').reviewStaleReason, /commits distants/);
  failed = true;
  card = (await request('/api/cards/a/refresh-pr', 'POST')).body;
  assert.equal(card.pullRequest.status, 'completed'); assert.equal(card.pullRequest.refreshError, 'Connexion refusée');
  assert.match(prLabel(card.pullRequest), /échec/);
});

test('Azure PR tracking validates identity and keeps the latest status in each context', async () => {
  const card = { branch: 'feature/a', pullRequest: { url: 'https://dev.azure.com/org/project/_git/repo/pullrequest/7', number: 7, repository: 'org/project/repo' } };
  const pr = { pullRequestId: 7, sourceRefName: 'refs/heads/feature/a', status: 'active', isDraft: true, reviewers: [{ displayName: 'Alice', vote: -10 }] };
  const request = async (repo, token, path) => {
    assert.equal(repo.name, 'org/project/repo');
    if (path.endsWith('/iterations')) return { value: [{ id: 1 }, { id: 2 }] };
    return path.endsWith('/statuses') ? { value: [{ id: 3, context: { name: 'Tests', genre: 'ci' }, state: 'succeeded', iterationId: 2 }, { id: 1, context: { name: 'Tests', genre: 'ci' }, state: 'failed', iterationId: 2 }, { id: 5, context: { name: 'Ancien contrôle' }, state: 'failed', iterationId: 1 }] } : pr;
  };
  const status = await refreshPullRequest(card, '', request);
  assert.equal(status.checks.length, 1); assert.equal(status.checks[0].state, 'succeeded'); assert.match(prLabel(status), /échec/);
  const partial = await refreshPullRequest(card, '', async (repo, token, path) => { if (path.endsWith('/statuses')) throw new Error('HTTP 403'); return { ...pr, status: 'completed' }; });
  assert.equal(partial.status, 'completed'); assert.equal(partial.checksError, 'HTTP 403');
  await assert.rejects(refreshPullRequest({ ...card, pullRequest: { ...card.pullRequest, url: 'https://evil.invalid/_git/repo/pullrequest/7' } }, '', request));
  await assert.rejects(refreshPullRequest(card, '', async () => ({ ...pr, sourceRefName: 'refs/heads/other' })), /correspond/);
});

test('queue serializes tasks, pauses on failure and never resumes by itself after restart', () => {
  const store = openStore(':memory:');
  const started = []; let running = false, success = true;
  const options = { store, launch(id) { started.push(id); running = true; return { id }; }, busy: () => running, result: () => success, changed() {} };
  let queue = createTaskQueue(options);
  try {
    queue.add('a'); queue.add('a'); queue.add('b'); queue.tick(); assert.deepEqual(started, []);
    queue.pause(false); queue.tick(); queue.tick(); assert.deepEqual(started, ['a']);
    assert.throws(() => queue.remove('a'), /attends la fin/);
    queue.pause(true); running = false; queue.tick(); assert.deepEqual(queue.state().ids, ['b']);
    queue.pause(false); queue.tick(); assert.deepEqual(started, ['a', 'b']);
    success = false; running = false; queue.tick(); assert.equal(queue.state().paused, true); assert.deepEqual(queue.state().ids, ['b']);
    queue.pause(false); queue.close(); queue = createTaskQueue(options);
    assert.equal(queue.state().paused, true); queue.tick(); assert.deepEqual(started, ['a', 'b']);
    queue.remove('b'); queue.add('c'); queue.pause(false); queue.tick(); assert.deepEqual(started, ['a', 'b', 'c']);
  } finally { queue.close(); store.db.close(); }
});

test('queue API prevents manual launches and deletion, and fails safely on blocked tasks', async t => {
  const { app, request } = fixture(t);
  app.store.putCard({ id: 'a', title: 'A', status: 'todo', blockedBy: ['b'] });
  app.store.putCard({ id: 'b', title: 'B', status: 'todo' });
  await request('/api/cards/a/queue', 'POST');
  assert.equal((await request('/api/cards/a', 'DELETE')).status, 409);
  assert.equal((await request('/api/queue', 'POST', { paused: 'false' })).status, 400);
  await request('/api/queue', 'POST', { paused: false });
  assert.equal((await request('/api/cards/b/run', 'POST')).status, 409);
  await new Promise(resolve => setTimeout(resolve, 250));
  const state = (await request('/api/state')).body;
  assert.equal(state.queue.paused, true); assert.match(state.queue.message, /bloquée/); assert.equal(state.runs.length, 0);
  assert.equal((await request('/api/cards/a/queue', 'DELETE')).status, 200);
});

test('restoring templates and a previously active queue keeps that queue paused', async t => {
  const { app, dataDir, request } = fixture(t);
  app.store.putCard({ id: 'a', title: 'A', status: 'todo' });
  const settings = { taskTemplates: [{ id: 'template', name: 'Conserver' }], taskQueue: { ids: ['a'], paused: false } };
  app.store.saveSettings(settings);
  const backup = createBackups(app.store, dataDir).create();
  app.store.saveSettings({ taskTemplates: [], taskQueue: { ids: [], paused: true } });
  const response = await request('/api/backups/restore', 'POST', { name: backup.name, confirmed: true });
  assert.equal(response.status, 200);
  const state = (await request('/api/state')).body;
  assert.equal(state.settings.taskTemplates[0].name, 'Conserver');
  assert.equal(state.queue.paused, true); assert.deepEqual(state.queue.ids, ['a']);
});

test('next actions, cleanup candidates and reports distinguish recorded work from current blockers', () => {
  const cards = [{ id: 'a', title: 'Front', status: 'todo', blockedBy: ['b'] }, { id: 'b', title: 'Back', status: 'done' }];
  assert.equal(nextAction(cards[0], [], cards), '');
  assert.equal(matchesFocus(cards[0], [], 'attention', cards), false);
  assert.equal(matchesFocus(cards[0], [], 'attention', [{ ...cards[1], status: 'todo' }]), true);
  assert.match(nextAction({ ...cards[0], blockedBy: ['missing'] }, [], cards), /introuvable/);
  assert.match(nextAction({ id: 'x', automation: { state: 'error', step: 'tests' } }, [], cards), /vérifications/);
  assert.equal(cleanupCandidate({ worktree: {}, status: 'todo' }), false);
  assert.equal(cleanupCandidate({ worktree: {}, archived: true }), true);
  assert.equal(cleanupCandidate({ worktree: {}, pullRequest: { status: 'completed', refreshError: 'offline' } }), false);
  const date = new Date(2026, 9, 21, 12).toISOString();
  const report = workReport(cards, [{ cardId: 'b', endedAt: date, status: 'completed' }, { cardId: 'other', endedAt: date, status: 'failed' }], [{ cardId: 'b', date: '2026-10-21', seconds: 3660 }, { cardId: 'other', date: '2026-10-21', seconds: 99999 }], '2026-10-21');
  assert.match(report, /Back : réussie, à vérifier/); assert.match(report, /1 h 1 min/); assert.doesNotMatch(report, /other/);
});
