import test from 'node:test';
import assert from 'node:assert/strict';
import { createProjectsView } from '../public/views/projects.mjs';

function fixture() {
  const nodes = new Map(), calls = [], selected = [];
  const remove = { dataset: { removeProject: 'project' } }, archive = { dataset: { archiveRepo: 'C:/orphan' } }, select = { dataset: { project: 'project' } };
  let confirmed = false;
  const document = {
    querySelector(key) { if (!nodes.has(key)) nodes.set(key, { value: '', innerHTML: '' }); return nodes.get(key); },
    querySelectorAll: key => key === '[data-remove-project]' ? [remove] : key === '[data-archive-repo]' ? [archive] : key === '[data-project]' ? [select] : [],
  };
  const view = createProjectsView({ document, location: { hash: '#project=project' }, api: async (...args) => calls.push(args), action: fn => fn(),
    confirmAction: async () => confirmed, async refresh() {}, toast() {}, onSelect: (...args) => selected.push(args), openProjectSettings() {},
  });
  const state = { projects: [{ id: 'project', name: '<Projet>', repo: 'C:/project' }], cards: [{ id: 'orphan', repo: 'C:/orphan', archived: false }] };
  view.render(state);
  return { view, state, nodes, calls, selected, remove, archive, select, confirm: () => { confirmed = true; } };
}
test('project view restores the filter, escapes names and shows orphan tasks', () => {
  const { nodes, select, selected } = fixture();
  assert.equal(nodes.get('#project-filter').value, 'project');
  assert.match(nodes.get('#project-list').innerHTML, /&lt;Projet&gt;/);
  assert.match(nodes.get('#project-list').innerHTML, /data-archive-repo="C:\/orphan"/);
  select.onclick(); assert.deepEqual(selected, [['project', true]]);
});
test('project and orphan archiving require confirmation and use the existing endpoints', async () => {
  const { calls, remove, archive, confirm } = fixture();
  await remove.onclick(); await archive.onclick(); assert.deepEqual(calls, []);
  confirm(); await remove.onclick(); await archive.onclick();
  assert.deepEqual(calls, [['/api/projects/project', 'DELETE', { archiveTasks: true }], ['/api/projects/archive-tasks', 'POST', { repo: 'C:/orphan' }]]);
});
test('invalidating the project view rebuilds unchanged data after a restoration', () => {
  const { view, nodes, state } = fixture();
  nodes.get('#project-list').innerHTML = 'stale'; view.render(state);
  assert.equal(nodes.get('#project-list').innerHTML, 'stale');
  view.invalidate(); view.render(state);
  assert.match(nodes.get('#project-list').innerHTML, /&lt;Projet&gt;/);
});
