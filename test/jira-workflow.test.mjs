import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

test('Jira sync sorts by configured status IDs and names without starting agents', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-jira-sort-'));
  const jiraUrl = 'https://example.atlassian.net';
  const issues = [
    ['todo', '10000', 'To Do', 'new'], ['doing', '10001', 'In Progress', 'indeterminate'],
    ['review', '10234', 'Review', 'indeterminate'], ['done', '10002', 'Done', 'done'], ['standby', '10135', 'Stand by', 'new'],
    ['unmapped', 'other', 'Other', 'indeterminate'], ['off', '10001', 'In Progress', 'indeterminate'],
    ['custom', 'custom-id', 'Custom', 'indeterminate'],
  ].map(([id, statusId, name, category]) => ({ id, key: id, fields: { summary: id, status: { id: statusId, name, statusCategory: { key: category } } } }));
  const app = createApp({ dataDir, launch: () => assert.fail('Import must not launch an agent'), requestJira: async (_, __, path) => {
    assert.match(path, /\/search/);
    return { issues, isLast: true };
  } });
  try {
    app.store.saveSettings({ jiraUrl, jiraAutoTransition: true, jiraStatusMap: { todo: '10000', doing: '10001', review: '10234', done: 'done', standby: '10135' } });
    const id = key => `jira-${Buffer.from(jiraUrl).toString('base64url')}-${key}`;
    app.store.putProject({ id: 'off', repo: 'off-project', jiraMode: 'off' });
    app.store.putProject({ id: 'custom', repo: 'custom-project', jiraMode: 'custom', jiraStatusMap: { standby: 'custom-id' } });
    for (const key of ['unmapped', 'off', 'custom']) app.store.putCard({ id: id(key), status: 'todo', agent: 'codex', repo: key + '-project' });
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const sync = async () => {
      const response = await fetch(`http://127.0.0.1:${app.server.address().port}/api/jira/sync`, {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: '{}',
      });
      assert.equal(response.status, 200);
    };
    await sync();
    for (const column of ['todo', 'doing', 'review', 'done', 'standby']) assert.equal(app.store.card(id(column)).status, column);
    assert.equal(app.store.card(id('unmapped')).status, 'todo');
    assert.equal(app.store.card(id('off')).status, 'todo');
    assert.equal(app.store.card(id('custom')).status, 'standby');
    issues[1].fields.status = { id: '10000', name: 'To Do' };
    await sync();
    assert.equal(app.store.card(id('doing')).status, 'todo');
    app.store.saveSettings({ jiraUrl, jiraAutoTransition: false, jiraStatusMap: { doing: '10001' } });
    issues[1].fields.status = { id: '10001', name: 'In Progress' };
    await sync();
    assert.equal(app.store.card(id('doing')).status, 'todo');
    assert.equal(app.store.runs().length, 0);
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});

for (const [column, target, name] of [['done', '10002', 'Done'], ['standby', '10135', 'Stand by']]) test(`${column} synchronizes its Jira status ID, keeps local progress on failure and allows retry`, async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-jira-workflow-'));
  const jiraUrl = 'https://example.atlassian.net';
  let status = 'In Progress', unavailable = false;
  const calls = [];
  const app = createApp({ dataDir, requestJira: async (_, __, path, body) => {
    calls.push({ path, body });
    if (unavailable) throw new Error('Jira indisponible');
    if (body) {
      assert.deepEqual(body, { transition: { id: '31' } });
      status = name;
      return {};
    }
    if (path.includes('/transitions')) return { transitions: [{ id: '31', to: { id: target, name } }] };
    return { fields: { status: { id: status === name ? target : '10001', name: status } } };
  } });
  try {
    app.store.saveSettings({ jiraUrl, jiraAutoTransition: true, jiraStatusMap: { [column]: target } });
    app.store.putCard({ id: 'ticket', title: 'Ticket', agent: 'codex', status: 'review', ticket: { base: jiraUrl, key: 'TEST-1', status } });
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const request = async (action, method, body) => {
      const response = await fetch(`http://127.0.0.1:${app.server.address().port}/api/cards/ticket${action}`, {
        method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify(body),
      });
      assert.equal(response.status, 200);
      return response.json();
    };
    unavailable = true;
    const failed = await request('', 'PATCH', { status: column });
    assert.equal(failed.status, column);
    assert.equal(failed.ticket.status, 'In Progress');
    assert.equal(failed.ticketSync.state, 'error');
    assert.equal(failed.ticketSync.message, 'Jira indisponible');
    unavailable = false;
    const synced = await request('/sync-status', 'POST', {});
    assert.equal(synced.ticket.status, name);
    assert.equal(synced.ticketSync.state, 'synced');
    assert.equal(synced.ticketSync.localStatus, column);
    assert.equal(calls.filter(call => call.body).length, 1);
    await request('/sync-status', 'POST', {});
    assert.equal(calls.filter(call => call.body).length, 1);
    const count = calls.length;
    await request('', 'PATCH', { status: 'review' });
    assert.equal(calls.length, count);
    await request('', 'PATCH', { status: column });
    assert.equal(calls.filter(call => call.body).length, 1);
    assert.equal(app.store.card('ticket').ticketSync.state, 'synced');
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});
