import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { inspectTaskFiles } from '../lib/infrastructure/task-files.mjs';
import { createApp } from '../server.mjs';

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'atelier-files-')), repo = join(dir, 'repo');
  await mkdir(repo);
  t.after(() => rm(dir, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8', windowsHide: true }).trim();
  git('init', '-b', 'task'); git('config', 'user.name', 'Test'); git('config', 'user.email', 'test@example.invalid'); git('config', 'core.autocrlf', 'false');
  await writeFile(join(repo, 'changed.txt'), 'before\n');
  await writeFile(join(repo, 'deleted.txt'), 'deleted\n');
  await writeFile(join(repo, '.gitignore'), 'ignored.txt\n');
  git('add', '.'); git('commit', '-qm', 'initial');
  const card = { id: 'task', title: 'Task', status: 'review', repo, worktree: { path: repo, source: repo, branch: 'task', baseCommit: git('rev-parse', 'HEAD') } };
  return { dir, repo, card, git };
}

test('task diff includes committed, staged, unstaged, deleted and untracked files without changing Git', async t => {
  const { repo, card, git } = await fixture(t);
  await writeFile(join(repo, 'committed.txt'), 'committed\n'); git('add', '.'); git('commit', '-qm', 'task commit');
  await writeFile(join(repo, 'staged.txt'), 'staged\n'); git('add', '.');
  await writeFile(join(repo, 'changed.txt'), 'after\n');
  await rm(join(repo, 'deleted.txt'));
  await writeFile(join(repo, 'new file é.txt'), '<script>new</script>\nlast');
  await writeFile(join(repo, 'ignored.txt'), 'secret');
  const before = git('status', '--porcelain=v1');
  const result = await inspectTaskFiles(card, 'changes');
  assert.deepEqual(Object.fromEntries(result.files.map(file => [file.path, file.status])), { 'changed.txt': 'M', 'committed.txt': 'A', 'deleted.txt': 'D', 'new file é.txt': 'A', 'staged.txt': 'A' });
  assert.equal(result.base, card.worktree.baseCommit);
  for (const path of ['committed.txt', 'staged.txt', 'new file é.txt']) assert.match((await inspectTaskFiles(card, 'file-diff', { path })).patch, /@@ -0,0 \+1/);
  const changed = await inspectTaskFiles(card, 'file-diff', { path: 'changed.txt' });
  assert.match(changed.patch, /-before\n\+after/);
  assert.match((await inspectTaskFiles(card, 'file-diff', { path: 'deleted.txt' })).patch, /-deleted/);
  const files = await inspectTaskFiles(card, 'files');
  assert.ok(files.files.includes('committed.txt'));
  assert.ok(!files.files.includes('ignored.txt'));
  assert.equal((await inspectTaskFiles(card, 'file', { path: 'new file é.txt' })).content, '<script>new</script>\nlast');
  assert.equal(git('status', '--porcelain=v1'), before);
});

test('file reads reject traversal, metadata, ignored files and directory links and bound previews', async t => {
  const { dir, repo, card } = await fixture(t);
  await mkdir(join(dir, 'outside'));
  await writeFile(join(dir, 'outside', 'secret.txt'), 'outside secret');
  await symlink(join(dir, 'outside'), join(repo, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  for (const path of ['../outside/secret.txt', '/etc/passwd', 'C:/secret', '.git/config', 'linked/secret.txt', 'a\\b', 'changed.txt:stream', ':(glob)*']) await assert.rejects(inspectTaskFiles(card, 'file', { path }));
  await writeFile(join(repo, 'ignored.txt'), 'ignored secret');
  await assert.rejects(inspectTaskFiles(card, 'file', { path: 'ignored.txt' }), /ignoré/);
  await writeFile(join(repo, 'large.txt'), 'a'.repeat(512 * 1024 + 1));
  await writeFile(join(repo, 'binary.bin'), Buffer.from([1, 0, 2]));
  await writeFile(join(repo, 'legacy.txt'), Buffer.from([255, 254]));
  assert.match((await inspectTaskFiles(card, 'file', { path: 'large.txt' })).unavailable, /volumineux/);
  assert.match((await inspectTaskFiles(card, 'file', { path: 'binary.bin' })).unavailable, /binaire/);
  assert.match((await inspectTaskFiles(card, 'file-diff', { path: 'binary.bin' })).unavailable, /binaire/);
  assert.match((await inspectTaskFiles(card, 'file', { path: 'legacy.txt' })).unavailable, /UTF-8/);
  await assert.rejects(inspectTaskFiles(card, 'file-diff', { path: 'changed.txt' }), /Actualise/);
});

test('source files remain readable without a worktree, but task diff needs its baseline', async t => {
  const { card } = await fixture(t);
  delete card.worktree;
  assert.equal((await inspectTaskFiles(card, 'files')).worktree, false);
  assert.equal((await inspectTaskFiles(card, 'file', { path: 'changed.txt' })).content, 'before\n');
  await assert.rejects(inspectTaskFiles(card, 'changes'), /Prépare le worktree/);
});

test('file preview rejects a file replaced between validation and opening', async t => {
  const { repo, card } = await fixture(t);
  const target = join(repo, 'changed.txt'), originalOpen = fs.promises.open;
  let replaced = false;
  const mock = t.mock.method(fs.promises, 'open', async (path, ...args) => {
    if (path === target && !replaced) {
      replaced = true;
      await fs.promises.rename(target, join(repo, 'before.txt'));
      await writeFile(target, 'unexpected contents');
    }
    return originalOpen(path, ...args);
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(inspectTaskFiles(card, 'file', { path: 'changed.txt' }), /changé/);
    assert.equal(replaced, true);
  } finally { mock.mock.restore(); syncBuiltinESMExports(); }
});

test('staged deletion and an untracked replacement have distinct diff previews', async t => {
  const { repo, card, git } = await fixture(t);
  git('rm', 'changed.txt');
  await writeFile(join(repo, 'changed.txt'), 'replacement\n');
  const entries = (await inspectTaskFiles(card, 'changes')).files.filter(file => file.path === 'changed.txt');
  assert.deepEqual(entries, [{ status: 'D', path: 'changed.txt' }, { status: 'A', path: 'changed.txt', untracked: true }]);
  const removed = await inspectTaskFiles(card, 'file-diff', { path: 'changed.txt' });
  const added = await inspectTaskFiles(card, 'file-diff', { path: 'changed.txt', untracked: 'true' });
  assert.match(removed.patch, /-before/);
  assert.match(added.patch, /\+replacement/);
  assert.doesNotMatch(added.patch, /-before/);
  git('add', 'changed.txt');
  await assert.rejects(inspectTaskFiles(card, 'file-diff', { path: 'changed.txt', untracked: 'true' }), /Actualise/);
});

test('inspection rejects a changed branch, detached HEAD and a different source repository', async t => {
  const { dir, repo, card, git } = await fixture(t);
  git('checkout', '-qb', 'other');
  for (const action of ['changes', 'file-diff', 'files', 'file']) {
    await assert.rejects(inspectTaskFiles(card, action, { path: 'changed.txt' }), /branche/);
  }
  git('checkout', '--detach', '-q');
  await assert.rejects(inspectTaskFiles(card, 'changes'), /branche/);
  git('checkout', '-q', 'task');
  const other = join(dir, 'other');
  execFileSync('git', ['clone', '--quiet', repo, other], { windowsHide: true });
  await assert.rejects(inspectTaskFiles({ ...card, repo: other, worktree: { ...card.worktree, source: other } }, 'files'), /dépôt source/);
  await assert.rejects(inspectTaskFiles({ ...card, repo: other }, 'files'), /dépôt source/);
  const linked = join(dir, 'linked');
  git('worktree', 'add', '-qb', 'linked-task', linked);
  const linkedCard = { ...card, worktree: { ...card.worktree, path: linked, branch: 'linked-task' } };
  assert.ok((await inspectTaskFiles(linkedCard, 'files')).files.includes('changed.txt'));
  assert.ok((await inspectTaskFiles({ ...linkedCard, repo: '' }, 'files')).files.includes('changed.txt'));
});

test('inspection disables configured Git programs without rewriting repository configuration', async t => {
  const { repo, card, git } = await fixture(t);
  const marker = 'inspection-program-ran';
  const program = `echo invoked > ${marker}; cat`;
  await writeFile(join(repo, '.gitattributes'), '*.txt filter=review diff=review\n');
  await writeFile(join(repo, 'changed.txt'), 'after\n');
  git('config', 'filter.review.clean', program);
  git('config', 'filter.review.smudge', program);
  git('config', 'filter.review.required', 'true');
  git('config', 'diff.review.command', program);
  git('config', 'diff.review.textconv', program);
  git('config', 'core.fsmonitor', `echo invoked > ${marker}`);
  for (const processFilter of [false, true]) {
    if (processFilter) git('config', 'filter.review.process', program);
    const config = await readFile(join(repo, '.git', 'config'));
    const index = await readFile(join(repo, '.git', 'index'));
    assert.ok((await inspectTaskFiles(card, 'changes')).files.some(file => file.path === 'changed.txt'));
    assert.match((await inspectTaskFiles(card, 'file-diff', { path: 'changed.txt' })).patch, /\+after/);
    assert.ok((await inspectTaskFiles(card, 'files')).files.includes('changed.txt'));
    assert.equal((await inspectTaskFiles(card, 'file', { path: 'changed.txt' })).content, 'after\n');
    await assert.rejects(access(join(repo, marker)), { code: 'ENOENT' });
    assert.deepEqual(await readFile(join(repo, '.git', 'config')), config);
    assert.deepEqual(await readFile(join(repo, '.git', 'index')), index);
  }
});

test('file HTTP routes require a session, forward exact paths and serve the workspace module', async t => {
  const { dir, repo, card, git } = await fixture(t);
  await writeFile(join(repo, 'é # [x].txt'), 'special path');
  const app = createApp({ dataDir: join(dir, 'data') });
  try {
  app.store.putCard(card);
  await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
  const url = 'http://127.0.0.1:' + app.server.address().port;
  const path = '/api/cards/task/file?path=' + encodeURIComponent('é # [x].txt');
  assert.equal((await fetch(url + path)).status, 401);
  const response = await fetch(url + path, { headers: { Authorization: 'Bearer ' + app.authToken } });
  assert.equal(response.status, 200);
  assert.equal((await response.json()).content, 'special path');
  git('rm', 'changed.txt');
  await writeFile(join(repo, 'changed.txt'), 'replacement\n');
  const added = await fetch(url + '/api/cards/task/file-diff?path=changed.txt&untracked=true', { headers: { Authorization: 'Bearer ' + app.authToken } });
  assert.equal(added.status, 200);
  assert.match((await added.json()).patch, /\+replacement/);
  assert.equal((await fetch(url + '/views/task-workspace.mjs')).status, 200);
  assert.equal((await fetch(url + '/api/cards/missing/files', { headers: { Authorization: 'Bearer ' + app.authToken } })).status, 404);
  } finally { await app.close(); }
});
