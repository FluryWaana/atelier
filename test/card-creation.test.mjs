import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

test('creation uses the selected column without starting an agent', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-creation-'));
  let launches = 0;
  const app = createApp({ dataDir, launch: () => { launches++; } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = `http://127.0.0.1:${app.server.address().port}/api/cards`;
    const create = body => fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify(body) });
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
