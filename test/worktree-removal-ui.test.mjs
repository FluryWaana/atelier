import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { removeWorktree } from '../public/views/workspace-workflows.mjs';

test('worktree removal requires explicit confirmation before discarding files', async () => {
    for (const accepted of [false, true]) {
      const calls = [], button = { dataset: { cleanWorktree: 'a' } };
      let warning = '';
      const context = {
        card: { id: 'a' }, $: () => button, document: { querySelectorAll: () => [button] },
        confirmAction: async message => { warning = message; return accepted; },
        action: fn => fn(), cardPath: id => '/api/cards/' + id,
        api: async (...args) => calls.push(args), async refresh() {}, renderDetail() {}, async updateRun() {}, toast() {}, worktreeChecks: [],
      };
      await removeWorktree('a', context);
      assert.match(warning, /modifications non commitées et les fichiers non suivis/);
      assert.match(warning, /irréversible/);
      assert.match(warning, /worktrees Front \/ Back associés/);
      assert.equal(JSON.stringify(calls), JSON.stringify(accepted ? [['/api/cards/a/worktree', 'DELETE', { discardChanges: true }]] : []));
    }
  });
