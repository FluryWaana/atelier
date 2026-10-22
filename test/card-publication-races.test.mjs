import test from 'node:test';
import assert from 'node:assert/strict';
import { openStore } from '../lib/store.mjs';
import { createCardWorkflows } from '../lib/application/card-workflows.mjs';

for (const action of ['pull-request', 'link-pr']) {
  for (const fails of [false, true]) test(`${action} preserves ticket updates while linking ${fails ? 'fails' : 'succeeds'}`, async t => {
    const store = openStore(':memory:');
    t.after(() => store.db.close());
    const publishing = new Set();
    const pr = { number: 7, url: 'https://forge.example/pull/7', ticketError: 'Ancienne erreur' };
    store.putCard({ id: 'card', title: 'Ancien titre', description: 'Ancienne description', status: 'review', ticket: { key: 'TEST-1', status: 'To Do' }, ...(action === 'link-pr' ? { pullRequest: pr } : {}) });
    let started, release;
    const linking = new Promise(resolve => { started = resolve; });
    const service = createCardWorkflows({ store, publishing, busy: id => publishing.has(id), changed() {}, invalidateReviews() {},
      publishPR: async () => pr,
      linkTicketPullRequest: () => { started(); return new Promise((resolve, reject) => { release = () => fails ? reject(new Error('Jira indisponible')) : resolve(true); }); },
    });
    const pending = service.execute('card', action, {});
    await linking;
    await assert.rejects(service.execute('card', 'link-pr', {}), { code: 'conflict' });
    const updated = store.card('card');
    updated.title = 'Titre actualisé par Jira'; updated.description = 'Nouveau besoin'; updated.ticket.status = 'Review';
    updated.ticketOutOfScope = true;
    store.putCard(updated);
    release();
    const result = await pending;
    assert.equal(result.title, updated.title);
    assert.equal(result.description, updated.description);
    assert.deepEqual(result.ticket, updated.ticket);
    assert.equal(result.ticketOutOfScope, true);
    assert.equal(result.pullRequest.number, 7);
    assert.equal(result.pullRequest.ticketLinked, !fails);
    assert.equal(result.pullRequest.ticketError, fails ? 'Jira indisponible' : undefined);
    assert.deepEqual(store.card('card'), result);
    assert.equal(publishing.size, 0);
  });
}
