import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

test('creating then syncing a Jira ticket preserves its local card and history across instances', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-jira-identity-'));
  const jiraUrl = 'https://example.atlassian.net';
  const issue = { id: '42', key: 'TEST-42', fields: { summary: 'Titre distant', description: '', status: { name: 'To Do' } } };
  const app = createApp({ dataDir, protectTokens: value => value, requestJira: async (_, __, path) => path.includes('/search') ? { issues: [issue], isLast: true } : issue });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const request = async (path, body, method = 'POST', status = 200) => {
      const response = await fetch(`http://127.0.0.1:${app.server.address().port}${path}`, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body) });
      const result = await response.json(); assert.equal(response.status, status, JSON.stringify(result)); return result;
    };
    await request('/api/settings', { jiraUrl, token: 'fake' }, 'PUT');
    const card = await request('/api/cards', { title: 'Local', checklist: [{ text: 'Conserver', done: false }] });
    await request(`/api/cards/${card.id}/create-ticket`, { project: 'TEST', issueType: 'Task' });
    app.store.putRun({ id: 'history', cardId: card.id, status: 'completed' });
    app.store.putTime({ id: 'time', cardId: card.id, seconds: 60, exportState: 'draft' });
    for (const path of ['/api/tickets/sync', '/api/tickets/reset']) await request(path, {});
    assert.equal(app.store.cards().length, 1);
    assert.equal(app.store.card(card.id).title, issue.fields.summary);
    assert.deepEqual(app.store.card(card.id).checklist, card.checklist);
    assert.equal(app.store.run('history').cardId, card.id);
    assert.equal(app.store.time('time').cardId, card.id);
    const other = await request('/api/cards', { title: 'Autre' });
    await request(`/api/cards/${other.id}/create-ticket`, { existingKey: issue.key }, 'POST', 409);
    assert.equal(app.store.card(other.id).ticket, undefined);
    await request('/api/settings', { jiraUrl: 'https://other.atlassian.net', token: 'fake' }, 'PUT');
    await request('/api/tickets/sync', {});
    assert.equal(app.store.cards().filter(card => card.ticket).length, 2);
    assert.equal(app.store.card(card.id).ticket.base, jiraUrl);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

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
        method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: '{}',
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
        method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1', Authorization: 'Bearer ' + app.authToken }, body: JSON.stringify(body),
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
