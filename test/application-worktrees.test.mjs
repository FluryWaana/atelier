import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { tmpdir } from 'node:os';
import { Readable } from 'node:stream';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { EventEmitter } from 'node:events';
import { createApp } from '../server.mjs';

function request(app, path, method = 'GET', body = {}) {
  return new Promise(resolve => {
    const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
    Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1', authorization: 'Bearer ' + app.authToken } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}

for (const primary of ['front', 'back']) test(`a principal ${primary} creates and reuses the other repository worktree on the same branch`, async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-app-worktrees-'));
  for (const name of ['front', 'back', 'primary-worktree']) mkdirSync(join(root, name));
  const sources = Object.fromEntries(['front', 'back'].map(role => [role, realpathSync(join(root, role))]));
  const peer = primary === 'front' ? 'back' : 'front', mainPath = realpathSync(join(root, 'primary-worktree'));
  const branch = 'feature/same-name';
  const worktrees = new Map(Object.values(sources).map(source => [source, [{ path: source, branch: 'main' }]]));
  worktrees.get(sources[primary]).push({ path: mainPath, branch });
  const commands = [], spawns = [];
  let dirty = false;
  const git = t.mock.method(childProcess, 'execFileSync', (command, args) => {
    assert.equal(command, 'git');
    const repo = args[1], options = args.slice(2); commands.push({ repo, options });
    if (options.includes('--show-toplevel')) return repo;
    if (options.includes('--git-common-dir')) return join(repo, '.git');
    if (options.includes('check-ref-format') || options[0] === 'fetch' || options[0] === 'for-each-ref' || options[0] === 'ls-files') return '';
    if (options.includes('FETCH_HEAD^{commit}')) return 'a'.repeat(40);
    if (options[0] === 'worktree' && options[1] === 'list') return worktrees.get(repo).map(item => `worktree ${item.path}\0branch refs/heads/${item.branch}\0\0`).join('');
    if (options[0] === 'worktree' && options[1] === 'add') {
      const path = options[options.indexOf('--') + 1]; mkdirSync(path, { recursive: true });
      worktrees.get(repo).push({ path: realpathSync(path), branch: options[options.indexOf('-b') + 1] });
      return '';
    }
    if (options[0] === 'worktree' && options[1] === 'remove') {
      if (dirty && !options.includes('--force')) throw new Error('Modifications locales');
      const path = options[options.indexOf('--') + 1];
      worktrees.set(repo, worktrees.get(repo).filter(item => item.path !== path)); return '';
    }
    throw new Error('Commande Git inattendue : ' + options.join(' '));
  });
  const spawn = t.mock.method(childProcess, 'spawn', (...args) => {
    spawns.push(args);
    const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
    child.stdout.setEncoding = child.stderr.setEncoding = () => {};
    queueMicrotask(() => child.emit('close', 0));
    return child;
  });
  syncBuiltinESMExports();
  const app = createApp({ dataDir: join(root, 'data') });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  try {
    app.store.putProject({ id: 'p', name: 'Projet', repo: sources[primary], [peer + 'Repo']: sources[peer], frontCommand: 'npm run dev', backCommand: 'dotnet run' });
    app.store.putCard({ id: 'card', title: 'Même tâche', repo: sources[primary], branch, status: 'todo', worktree: { source: sources[primary], path: mainPath, branch } });
    const start = await request(app, '/api/cards/card/dev-servers', 'POST', { standalone: true });
    assert.equal(start.status, 200, JSON.stringify(start.body));
    const card = app.store.card('card'), companion = card.serviceWorktrees[peer];
    assert.equal(companion.branch, branch);
    assert.equal(companion.source, sources[peer]);
    assert.notEqual(companion.path, sources[peer]);
    assert.match(basename(companion.path), /^feature-same-name-/);
    const launchedPaths = Object.fromEntries(start.body.services.map(service => [service.name.toLowerCase(), service.repo]));
    assert.equal(launchedPaths[primary], mainPath);
    assert.equal(launchedPaths[peer], companion.path);
    assert.equal(spawns.length, 2);
    assert.equal((await request(app, '/api/cards/card/dev-servers', 'POST', { standalone: true })).status, 200);
    assert.equal(commands.filter(call => call.options[0] === 'worktree' && call.options[1] === 'add').length, 1);
    const change = await request(app, '/api/projects/p', 'PATCH', { [peer + 'Repo']: sources[primary] });
    assert.equal(change.status, 409, 'cannot abandon an existing companion worktree');
    dirty = true;
    assert.equal((await request(app, '/api/cards/card/worktree', 'DELETE')).status, 400);
    assert.equal(app.store.card('card').serviceWorktrees[peer].path, companion.path);
    assert.equal(app.store.card('card').worktree.path, mainPath);
    assert.equal((await request(app, '/api/cards/card/worktree', 'DELETE', { discardChanges: true })).status, 200);
    assert.equal(app.store.card('card').worktree, undefined);
    assert.deepEqual(app.store.card('card').serviceWorktrees, {});
  } finally {
    await app.close(); git.mock.restore(); spawn.mock.restore(); syncBuiltinESMExports();
    rmSync(root, { recursive: true, force: true });
  }
});

test('detection reads each service repository and never offers the backend project as the frontend', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-detect-repos-'));
  for (const name of ['front', 'back']) mkdirSync(join(root, name));
  writeFileSync(join(root, 'front/package.json'), JSON.stringify({ scripts: { dev: 'vite' } }));
  writeFileSync(join(root, 'back/Api.csproj'), '<Project Sdk="Microsoft.NET.Sdk.Web"/>');
  const app = createApp({ dataDir: join(root, 'data') });
  t.mock.method(app.server, 'address', () => ({ port: 4317 }));
  try {
    app.store.putProject({ id: 'p', name: 'Back principal', repo: realpathSync(join(root, 'back')) });
    const detection = await request(app, '/api/projects/p/detect-launch', 'POST', { frontRepo: join(root, 'front') });
    assert.equal(detection.status, 200);
    assert.deepEqual(detection.body.candidates.map(candidate => [candidate.role, candidate.technology]), [['front', 'Vite'], ['back', 'ASP.NET Core']]);
    assert.notEqual(detection.body.candidates[0].sourceRepo, detection.body.candidates[1].sourceRepo);
    assert.equal(app.store.projects()[0].frontRepo, undefined, 'detection does not save configuration');
    const onlyFront = await request(app, '/api/projects/p/detect-launch', 'POST', { frontRepo: join(root, 'front'), backRepo: join(root, 'missing-back'), role: 'front' });
    assert.equal(onlyFront.status, 200, 'Front detection must not depend on the Back path');
    assert.equal(onlyFront.body.candidates.length, 1);
    assert.equal(onlyFront.body.candidates[0].role, 'front');
  } finally { await app.close(); rmSync(root, { recursive: true, force: true }); }
});
