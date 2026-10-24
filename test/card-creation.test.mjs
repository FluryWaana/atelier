import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

test('complete creation inherits templates and projects, resolves local directories and preserves UTF-16', async () => {
  const root = mkdtempSync(join(tmpdir(), 'kanban-creation-contract-'));
  const repo = join(root, 'dépôt 🔧'), file = join(root, 'file');
  mkdirSync(repo); writeFileSync(file, 'synthetic');
  let launches = 0;
  const app = createApp({ dataDir: join(root, 'data'), protectTokens: value => value, launch: () => { launches++; } });
  try {
    app.store.saveSettings({ repo, agent: 'codex', unknown: '\ud800', taskTemplates: [{ id: 'template', description: '\ud800', instructions: ' inherited ', mode: 'analyze', checklist: [{ text: ' \udc00 ', done: false }] }] });
    app.store.putProject({ id: 'project', repo, agent: 'claude' });
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const create = (path, body) => fetch(url + path, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
    const response = await create('/api/cards', { title: 'Inherited', templateId: 'template', status: 'doing', unknown: 'discard' });
    assert.equal(response.status, 200);
    const card = await response.json();
    assert.equal(card.agent, 'claude'); assert.equal(card.status, 'doing'); assert.equal(card.description, '\ud800');
    assert.deepEqual(card.checklist, [{ text: '\udc00', done: false }]); assert.equal(card.unknown, undefined);
    const explicit = await (await create('/api/cards', { title: 'Explicit', repo, agent: 'codex', model: ' model/v1 ', effort: 'high' })).json();
    assert.equal(explicit.repo, realpathSync(repo)); assert.equal(explicit.model, 'model/v1');
    const idea = await (await create('/api/brainstorms', { title: 'Idea', templateId: 'template', mode: 'invalid', status: 'invalid', description: '', checklist: [] })).json();
    assert.equal(idea.repo, ''); assert.equal(idea.status, 'todo'); assert.equal(idea.mode, 'analyze'); assert.equal(idea.brainstormSession, true);
    for (const body of [{ repo: file }, { repo: join(repo, 'missing') }, { repo: 'x\0y' }, { templateId: 'missing' }, { model: false }]) {
      assert.equal((await create('/api/cards', { title: 'Bad', ...body })).status, 400);
    }
    assert.equal(app.store.cards().length, 3); assert.equal(launches, 0);
  } finally { await app.close(); rmSync(root, { recursive: true, force: true }); }
});

test('creation uses the selected column without starting an agent', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-creation-'));
  let launches = 0;
  const app = createApp({ dataDir, launch: () => { launches++; } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}/api/cards`;
    const create = body => fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
    for (const status of [undefined, 'todo', 'doing', 'review']) {
      const response = await create({ title: 'Une tâche', status });
      assert.equal(response.status, 200);
      assert.equal((await response.json()).status, status ?? 'todo');
    }
    for (const status of ['done', 'invalid']) {
      assert.equal((await create({ title: 'Une tâche', status })).status, 400);
    }
    assert.equal(launches, 0);
    assert.equal(app.store.cards().length, 4);
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});
