import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const lines = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8').split('\n');
for (const [name, marker] of [['card detail', "  $('#remove-worktree').onclick ="], ['worktrees list', "  document.querySelectorAll('[data-clean-worktree]').forEach"]]) {
  test(name + ' requires explicit confirmation before discarding worktree files', async () => {
    for (const accepted of [false, true]) {
      const calls = [], button = { dataset: { cleanWorktree: 'a' } };
      let warning = '';
      const context = vm.createContext({
        card: { id: 'a' }, $: () => button, document: { querySelectorAll: () => [button] },
        confirmAction: async message => { warning = message; return accepted; },
        action: fn => fn(), cardPath: id => '/api/cards/' + id,
        api: async (...args) => calls.push(args), async refresh() {}, renderDetail() {}, async updateRun() {}, toast() {}, worktreeChecks: [],
      });
      vm.runInContext(lines.find(line => line.startsWith(marker)), context);
      await button.onclick();
      assert.match(warning, /modifications non commitées et les fichiers non suivis/);
      assert.match(warning, /irréversible/);
      assert.match(warning, /worktrees Front \/ Back associés/);
      assert.equal(JSON.stringify(calls), JSON.stringify(accepted ? [['/api/cards/a/worktree', 'DELETE', { discardChanges: true }]] : []));
    }
  });
}
