import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { Readable } from 'node:stream';
import { createApp } from '../server.mjs';

function request(app, body, headers = {}) {
  return new Promise(resolve => {
    const req = Readable.from([JSON.stringify(body)]);
    Object.assign(req, { method: 'PATCH', url: '/api/projects/project', headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1', authorization: 'Bearer ' + app.authToken, ...headers } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'atelier-project-location-'));
  for (const name of ['old', 'new', 'other']) mkdirSync(join(root, name));
  const old = realpathSync(join(root, 'old')), destination = realpathSync(join(root, 'new'));
  writeFileSync(join(old, 'keep.txt'), 'local files');
  const app = createApp({ dataDir: join(root, 'data') });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  app.store.putProject({ id: 'project', name: 'Projet', repo: old, frontCommand: 'npm start' });
  app.store.putCard({ id: 'card', title: 'Carte', repo: old, status: 'todo' });
  app.store.putCard({ id: 'archive', title: 'Archive', repo: old, archived: true, status: 'done' });
  t.after(async () => { await app.close(); rmSync(root, { recursive: true, force: true }); });
  return { app, root, old, destination };
}

test('changing a project location backs up and updates its cards without moving files or history', async t => {
  const { app, root, old, destination } = fixture(t);
  app.store.putRun({ id: 'review', cardId: 'card', kind: 'pr-review', status: 'completed', repo: old });
  const result = await request(app, { repo: destination });
  assert.equal(result.status, 200);
  assert.equal(result.body.repo, destination);
  assert.equal(result.body.frontCommand, 'npm start');
  assert.equal(app.store.card('card').repo, destination);
  assert.equal(app.store.card('archive').repo, destination);
  assert.equal(app.store.run('review').repo, old);
  assert.match(app.store.run('review').reviewStaleReason, /Emplacement/);
  assert.equal(readdirSync(join(root, 'data/backups')).length, 1);
  assert.equal(existsSync(join(old, 'keep.txt')), true);
  assert.equal(existsSync(join(destination, 'keep.txt')), false);
});

test('location changes reject existing worktrees, duplicate and invalid directories without partial updates', async t => {
  const { app, root, old, destination } = fixture(t);
  app.store.putProject({ id: 'other', name: 'Autre', repo: realpathSync(join(root, 'other')) });
  assert.equal((await request(app, { repo: join(root, 'other') })).status, 409);
  assert.equal((await request(app, { repo: join(root, 'missing') })).status, 400);
  assert.equal((await request(app, { repo: join(old, 'keep.txt') })).status, 400);
  app.store.putCard({ ...app.store.card('archive'), worktree: { path: 'existing-worktree' } });
  const response = await request(app, { repo: destination, frontCommand: 'changed' });
  assert.equal(response.status, 409);
  assert.match(response.body.error, /worktrees/);
  assert.equal(app.store.projects().find(project => project.id === 'project').repo, old);
  assert.equal(app.store.card('card').repo, old);
  assert.equal(readdirSync(join(root, 'data/backups')).length, 0);
  assert.equal((await request(app, { repo: old, frontCommand: 'changed' })).status, 200, 'normal settings can still be saved with existing worktrees');
});

test('a failed card update rolls back the project location and every card', async t => {
  const { app, old, destination } = fixture(t);
  t.mock.method(app.store, 'putCard', () => { throw new Error('storage failure'); });
  assert.equal((await request(app, { repo: destination })).status, 400);
  assert.equal(app.store.projects()[0].repo, old);
  assert.equal(app.store.card('card').repo, old);
  assert.equal(app.store.card('archive').repo, old);
});

test('project rules can be saved, preserved by partial updates and removed', async t => {
  const { app } = fixture(t);
  const projectRules = 'Préserve les accents.\nExécute les tests.';
  const saved = await request(app, { projectRules });
  assert.equal(saved.status, 200);
  assert.equal(saved.body.projectRules, projectRules);
  assert.equal(app.store.projects()[0].projectRules, projectRules);
  assert.equal((await request(app, { baseBranch: 'main' })).body.projectRules, projectRules);
  assert.equal((await request(app, { projectRules: [] })).status, 400);
  assert.equal(app.store.projects()[0].projectRules, projectRules);
  assert.equal((await request(app, { projectRules: '   ' })).body.projectRules, '');
  assert.equal(app.store.projects()[0].projectRules, '');
});

test('project rules reject unauthorized, foreign-origin and oversized updates without changing stored data', async t => {
  const { app } = fixture(t);
  await request(app, { projectRules: 'Règles conservées' });
  for (const [headers, status] of [[{ authorization: '' }, 401], [{ origin: 'https://example.com' }, 403], [{ 'x-local-request': '' }, 403]]) {
    assert.equal((await request(app, { projectRules: 'Remplacement' }, headers)).status, status);
    assert.equal(app.store.projects()[0].projectRules, 'Règles conservées');
  }
  assert.equal((await request(app, { projectRules: 'x'.repeat(12001) })).status, 400);
  assert.equal(app.store.projects()[0].projectRules, 'Règles conservées');
});
