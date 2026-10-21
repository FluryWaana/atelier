import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createApp } from '../server.mjs';

test('archive and deletion stay local for Jira cards in every column', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'kanban-actions-'));
  let externalCalls = 0;
  const app = createApp({ dataDir, requestJira: async () => { externalCalls++; throw new Error('Appel externe interdit'); } });
  try {
    await new Promise(resolve => app.server.listen(0, '127.0.0.1', resolve));
    const url = 'http://127.0.0.1:' + app.server.address().port;
    const request = (path, method, body) => fetch(url + path, { method, headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: JSON.stringify(body) });
    for (const status of ['todo', 'doing', 'review', 'done', 'standby']) {
      const card = { id: status, title: 'Carte Jira', status, agent: 'codex', ticket: { key: 'TEST-1', base: 'https://jira.example' } };
      app.store.putCard(card);
      const path = '/api/cards/' + card.id;
      assert.equal((await request(path + '/archive', 'POST', { archived: true })).status, 200);
      assert.equal(app.store.card(card.id).archived, true);
      assert.equal(app.store.card(card.id).status, status);
      assert.equal((await request(path + '/archive', 'POST', { archived: false })).status, 200);
      assert.equal((await request(path, 'DELETE', {})).status, 200);
      assert.equal(app.store.card(card.id), null);
    }
    assert.equal(externalCalls, 0);
  } finally {
    await app.close();
    rmSync(dataDir, { recursive: true, force: true });
  }
});
