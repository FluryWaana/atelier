import test from 'node:test';
import assert from 'node:assert/strict';
import { createRuns } from '../lib/application/runs.mjs';

test('failed worktree preparation releases the card and allows another attempt without launching an agent', async () => {
  const publishing = new Set(), active = new Map();
  let attempts = 0;
  const service = createRuns({ active, publishing,
    automationBusy: () => false, checkDependencies() {}, configFor: () => ({}), changed() {},
    prepareWorktree: async () => { attempts++; throw new Error('Git : Filename too long'); },
    launch: () => assert.fail('must not launch without a worktree'),
  });
  for (let i = 0; i < 2; i++) {
    await assert.rejects(service.start({ id: 'a' }, { agent: 'codex', mode: 'edit' }), /Filename too long/);
    assert.equal(publishing.size, 0); assert.equal(active.size, 0);
  }
  assert.equal(attempts, 2);
  await service.close();
});

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
