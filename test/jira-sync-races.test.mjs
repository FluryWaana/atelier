import test from 'node:test';
import assert from 'node:assert/strict';
import { openStore } from '../lib/store.mjs';
import { createSyncRepository } from '../lib/infrastructure/sync-repository.mjs';
import { createJiraSync } from '../lib/application/jira-sync.mjs';

function fixture(t) {
  const store = openStore(':memory:');
  t.after(() => store.db.close());
  const settings = { jiraUrl: 'https://jira.example', repo: '', agent: 'codex', jiraAutoTransition: true, jiraStatusMap: { todo: 'To Do', review: 'Review', done: 'Done' } };
  const busy = new Set();
  let resolveFetch, rejectFetch;
  const service = createJiraSync({ store: createSyncRepository(store), config: () => settings, configFor: () => settings,
    token: () => '', fetchIssues: () => new Promise((resolve, reject) => { resolveFetch = resolve; rejectFetch = reject; }),
    busy: id => busy.has(id), plainText: text => text || '', timestamp: () => 'now', invalidateReviews() {},
    issueId: (base, id) => 'card-' + id,
  });
  const card = { id: 'local', title: 'Ticket', status: 'todo', agent: 'codex', ticket: { connector: 'jira', base: settings.jiraUrl, id: '1', key: 'TEST-1' } };
  store.putCard(card);
  return { store, service, busy, card,
    complete: (status = 'To Do') => resolveFetch([{ id: '1', key: 'TEST-1', fields: { summary: 'Titre distant actualisé', status: { name: status } } }]),
    fail: () => rejectFetch(new Error('Jira indisponible')),
  };
}

test('a slow Jira sync preserves a column changed locally while fetching', async t => {
  const { store, service, card, complete } = fixture(t);
  const pending = service.sync();
  store.putCard({ ...card, status: 'done' });
  complete(); await pending;
  assert.equal(store.card(card.id).status, 'done');
  assert.equal(store.card(card.id).title, 'Titre distant actualisé');
  const next = service.sync();
  complete('Review'); await next;
  assert.equal(store.card(card.id).status, 'review', 'a later sync still applies remote changes');
});

test('Jira sync preserves a busy column and releases its lock after a network failure', async t => {
  const { store, service, busy, card, complete, fail } = fixture(t);
  const failed = service.sync();
  const rejection = assert.rejects(failed, /Jira indisponible/);
  await assert.rejects(service.sync(), { code: 'conflict' });
  fail(); await rejection;
  assert.equal(service.busy(), false);
  busy.add(card.id);
  const retry = service.sync();
  complete('Done'); await retry;
  assert.equal(store.card(card.id).status, 'todo');
  assert.equal(service.busy(), false);
});
