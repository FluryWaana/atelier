import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { openStore } from '../lib/store.mjs';
import { createApp } from '../server.mjs';

test('restart persists the last known agent activity instead of counting offline time', async () => {
  const root = new URL('../.test-tmp/', import.meta.url);
  mkdirSync(root, { recursive: true });
  const dataDir = mkdtempSync(new URL('time-restart-', root));
  const store = openStore(join(dataDir, 'kanban.sqlite'));
  store.putCard({ id: 'a', title: 'Tâche', status: 'doing' });
  const startedAt = '2026-10-20T10:00:00Z', lastEventAt = '2026-10-20T10:01:00Z';
  const usage = { inputTokens: 1000, cacheReadTokens: 800, outputTokens: 100, totalTokens: 1100, costUsd: 0.0022 };
  store.putRun({ id: 'run', cardId: 'a', status: 'running', startedAt, lastEventAt, usage, interactions: [{ id: 'pending', kind: 'question' }] });
  store.putRun({ id: 'silent', cardId: 'a', status: 'running', startedAt });
  store.db.close();
  const app = createApp({ dataDir });
  try {
    assert.equal(app.store.run('run').status, 'interrupted');
    assert.equal(app.store.run('run').interactions, undefined);
    assert.equal(app.store.run('run').endedAt, lastEventAt);
    assert.deepEqual(app.store.run('run').usage, usage);
    assert.equal(app.store.run('silent').endedAt, startedAt);
    assert.equal(app.store.card('a').status, 'todo');
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});
