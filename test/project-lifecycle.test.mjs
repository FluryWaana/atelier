import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, renameSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';
import { spawnSync } from 'node:child_process';

test('empty repository paths resolve the synthetic process working directory for projects and tasks', () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-empty-repo-'));
  try {
    const output = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import { realpathSync } from 'node:fs';
      const { createApp } = await import(${JSON.stringify(import.meta.resolve('../server.mjs'))});
      const app = createApp({ dataDir: 'data', protectTokens: value => value });
      try {
        await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
        const url = 'http://127.0.0.1:' + app.server.address().port;
        const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
        const project = await request('/api/projects', 'POST', { name: 'Synthetic cwd', repo: '' });
        assert.equal(project.status, 200); assert.equal((await project.json()).repo, realpathSync('.'));
        const created = await request('/api/cards', 'POST', { title: 'Synthetic cwd', repo: '   ' });
        assert.equal(created.status, 200); const card = await created.json(); assert.equal(card.repo, realpathSync('.'));
        const edited = await request('/api/cards/' + card.id, 'PATCH', { repo: '   ' });
        assert.equal(edited.status, 200); assert.equal((await edited.json()).repo, realpathSync('.'));
      } finally { await app.close(); }
    `], { cwd: root, encoding: 'utf8', windowsHide: true, timeout: 30000 });
    assert.equal(output.status, 0, output.stdout + output.stderr);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('project HTTP lifecycle rejects duplicates and active timers while retaining archived cards and time history', async () => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-project-contract-'));
  const repo = join(root, 'project'); mkdirSync(repo);
  const app = createApp({ dataDir: join(root, 'data'), protectTokens: value => value });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    const responses = await Promise.all([request('/api/projects', 'POST', { name: '\ud800', repo }), request('/api/projects', 'POST', { name: '\ud800', repo })]);
    assert.deepEqual(responses.map(response => response.status).sort(), [200, 409]);
    const project = await responses.find(response => response.status === 200).json(); assert.equal(project.name, '\ud800');
    const cards = [];
    for (const title of ['First', 'Second']) cards.push(await (await request('/api/cards', 'POST', { title, repo })).json());
    const timer = await (await request('/api/time', 'POST', { cardId: cards[0].id, seconds: 60, date: '2026-10-24', startTime: '09:00:00', description: 'synthetic', start: true })).json();
    assert.equal((await request('/api/projects/' + project.id, 'DELETE', { archiveTasks: true })).status, 409);
    assert.equal((await request('/api/projects/archive-tasks', 'POST', { repo: project.repo })).status, 409);
    assert.equal((await request('/api/time/' + timer.id + '/stop', 'POST', {})).status, 200);
    const removed = await request('/api/projects/' + project.id, 'DELETE', { archiveTasks: true });
    assert.equal(removed.status, 200); assert.deepEqual(await removed.json(), { ok: true, archived: 2 });
    renameSync(repo, join(root, 'moved-synthetic-directory'));
    const orphaned = await request('/api/projects/archive-tasks', 'POST', { repo: project.repo });
    assert.equal(orphaned.status, 200); assert.deepEqual(await orphaned.json(), { count: 2 });
    const state = await (await request('/api/state', 'GET')).json();
    assert.equal(state.projects.length, 0); assert.equal(state.cards.length, 2); assert.equal(state.times.length, 1);
    assert.ok(state.cards.every(card => card.archived));
  } finally { await app.close(); rmSync(root, { recursive: true, force: true }); }
});
