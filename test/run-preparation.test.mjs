import test from 'node:test';
import assert from 'node:assert/strict';
import { createRuns } from '../lib/application/runs.mjs';

test('shutdown and cancellation drain worktree preparation without launching an agent', async () => {
  for (const stopping of ['shutdown', 'cancel']) {
    let release, saved;
    const worktree = new Promise(resolve => { release = resolve; });
    const publishing = new Set(), signal = new AbortController();
    const service = createRuns({ store: { putCard: card => { saved = card; } }, active: new Map(), publishing,
      automationBusy: () => false, checkDependencies() {}, configFor: () => ({}), changed() {},
      prepareWorktree: () => worktree, launch: () => assert.fail('must not launch after cancellation or shutdown'),
    });
    const card = { id: 'a', title: 'Task', repo: 'repo' };
    const starting = service.start(card, { agent: 'codex', mode: 'edit', signal: signal.signal });
    assert.equal(publishing.has(card.id), true);
    await assert.rejects(service.start(card, { agent: 'codex', mode: 'edit' }), /active/);
    const rejected = assert.rejects(starting, stopping === 'shutdown' ? /arrêt/ : /abort/i);
    const closing = stopping === 'shutdown' ? service.close() : null;
    if (stopping === 'cancel') signal.abort();
    release({ path: 'worktree', branch: 'task' });
    await rejected; await closing;
    assert.equal(saved.worktree.path, 'worktree', 'keep the created worktree associated for later recovery');
    assert.equal(publishing.size, 0);
  }
});
