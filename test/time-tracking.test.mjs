import test from 'node:test';
import assert from 'node:assert/strict';
import { openStore } from '../lib/store.mjs';
import { createTimeTracking } from '../lib/time-tracking.mjs';

test('local timers remain independent and Tempo preserves its uncertain-send protections', async () => {
  const store = openStore(':memory:');
  let now = Date.parse('2026-10-22T09:00:00Z'), release, sent = 0;
  const waiting = new Promise(resolve => { release = resolve; });
  store.putCard({ id: 'card', ticket: { key: 'TEST-1', base: 'https://jira.example' } });
  const service = createTimeTracking({ store, now: () => now, newId: () => 'entry', changed() {},
    config: () => ({ jiraType: 'cloud', jiraUrl: 'https://jira.example' }), jiraToken: () => 'jira', tempoToken: () => 'tempo',
    requestJira: async (_, __, path) => path.endsWith('/myself') ? { accountId: 'user' } : { id: '42' },
    requestTempo: async () => { sent++; await waiting; throw new Error('Response lost'); },
    findTempoEntry: async () => ({ tempoWorklogId: 99 }),
  });
  try {
    const fields = { cardId: 'card', seconds: 60, date: '2026-10-22', startTime: '09:00:00', description: 'Work', start: true };
    await service.execute('create', null, fields);
    now += 65000;
    assert.equal((await service.execute('stop', 'entry')).seconds, 65);
    assert.equal(sent, 0);
    await assert.rejects(service.execute('export', 'entry', {}), /Valide/);
    const exporting = service.execute('export', 'entry', { confirmed: true });
    assert.equal(service.busy(), true);
    await assert.rejects(service.execute('remove', 'entry'), /cours/);
    release();
    assert.equal((await exporting).exportState, 'uncertain');
    await assert.rejects(service.execute('export', 'entry', { confirmed: true }), /Valide/);
    assert.equal((await service.execute('reconcile', 'entry', {})).tempoId, 99);
    assert.equal(store.time('entry').exportState, 'exported');
    assert.equal(sent, 1);
    assert.equal(service.busy(), false);
  } finally { service.close(); store.db.close(); }
});
