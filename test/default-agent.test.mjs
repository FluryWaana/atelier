import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';
import { settingsForProject } from '../lib/project-settings.mjs';

test('projects inherit the global agent unless they override it', () => {
  const projects = [{ repo: 'project', agent: 'codex' }];
  assert.equal(settingsForProject({}, [], '').agent, 'codex');
  assert.equal(settingsForProject({ agent: 'claude' }, [], '').agent, 'claude');
  assert.equal(settingsForProject({ agent: 'claude' }, projects, 'PROJECT').agent, process.platform === 'win32' ? 'codex' : 'claude');
  assert.equal(settingsForProject({ agent: 'claude' }, [{ repo: 'project', agent: '' }], 'project').agent, 'claude');
});

test('global agent is validated, saved and used for new cards', async t => {
  // Keep this settings test independent of Windows DPAPI availability.
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-agent-'));
  const app = createApp({ protectTokens: value => value, dataDir });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify(body) });
    const settings = (await (await fetch(url + '/api/state')).json()).settings;
    assert.equal(settings.agent, 'codex');
    const original = await (await request('/api/cards', 'POST', { title: 'Avant' })).json();
    assert.equal((await request('/api/settings', 'PUT', { ...settings, agent: 'invalid' })).status, 400);
    const saved = await request('/api/settings', 'PUT', { ...settings, agent: 'claude' });
    assert.equal(saved.status, 200);
    assert.equal((await saved.json()).agent, 'claude');
    assert.equal(app.store.settings().agent, 'claude');
    assert.equal((await (await request('/api/cards', 'POST', { title: 'Après' })).json()).agent, 'claude');
    assert.equal((await (await request('/api/cards', 'POST', { title: 'Choix explicite', agent: 'codex' })).json()).agent, 'codex');
    assert.equal(app.store.card(original.id).agent, 'codex');
    const { agent, ...legacy } = settings;
    assert.equal((await request('/api/settings', 'PUT', legacy)).status, 200);
    assert.equal(app.store.settings().agent, 'claude');
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
    
    
  }
});

test('Jira sync updates untouched tickets without a repository and preserves manual or started agents', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-jira-agent-'));
  const issues = ['untouched', 'manual', 'model', 'effort', 'started', 'repository', 'worktree', 'new'].map(id => ({ id, key: id, fields: { summary: id, status: {} } }));
  const app = createApp({ protectTokens: value => value, dataDir, requestJira: async () => ({ issues, isLast: true }) });
  try {
    const jiraUrl = 'https://example.atlassian.net';
    app.store.saveSettings({ jiraUrl, agent: 'claude' });
    const id = key => `jira-${Buffer.from(jiraUrl).toString('base64url')}-${key}`;
    for (const key of ['untouched', 'manual', 'model', 'effort', 'started', 'repository', 'worktree']) {
      app.store.putCard({ id: id(key), title: key, agent: 'codex', repo: key === 'repository' ? dataDir : '', status: 'todo', ticket: { key }, ...(key === 'worktree' ? { worktree: { path: dataDir } } : {}) });
    }
    app.store.putRun({ id: 'run', cardId: id('started'), status: 'done' });
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}`;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify(body) });
    for (const agent of ['claude', 'codex']) assert.equal((await request('/api/cards/' + id('manual'), 'PATCH', { agent })).status, 200);
    assert.equal((await request('/api/cards/' + id('model'), 'PATCH', { model: 'custom-codex' })).status, 200);
    assert.equal((await request('/api/cards/' + id('effort'), 'PATCH', { effort: 'xhigh' })).status, 200);
    assert.equal((await request('/api/jira/sync', 'POST', {})).status, 200);
    for (const key of ['untouched', 'new']) assert.equal(app.store.card(id(key)).agent, 'claude');
    for (const key of ['manual', 'model', 'effort', 'started', 'repository', 'worktree']) assert.equal(app.store.card(id(key)).agent, 'codex');
    assert.equal((await request('/api/jira/sync', 'POST', {})).status, 200);
    assert.equal(app.store.card(id('manual')).agent, 'codex');
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});
