import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createTaskEditor } from '../public/views/task-editor.mjs';
import { loadVersion } from '../public/ui-actions.mjs';
import { bindView } from './helpers/view-context.mjs';

test('creation and project selection target the repository disclosure among optional sections', () => {
  for (const repo of ['', 'C:/project']) {
    const directory = { open: false };
    const form = {
      elements: { repo: { value: '', closest(selector) { assert.equal(selector, 'details'); return directory; } }, agent: {}, title: { focus() {} } },
      dataset: {}, reset() {}, querySelector: () => ({}),
    };
    const nodes = new Map([
      ['#new-form', form], ['#create-button', {}], ['#new-form h2', {}],
      ['#new-form button.primary', {}], ['#new-form .notice-line', {}],
      ['#template-criteria', {}], ['#project-filter', { value: '' }], ['#new-project', {}],
      ['#new-dialog', { setAttribute() {}, showModal() {}, querySelector: selector => selector === '.draft-notice' ? null : { after() {} } }],
    ]);
    const context = {
      $: selector => { assert.ok(nodes.has(selector), selector); return nodes.get(selector); },
      state: { projects: [], settings: { repo } },
      document: { querySelectorAll: () => [], createElement: () => ({}) }, attachDraft() {}, taskAgentFields() {},
    };
    bindView(createTaskEditor, context).bind();
    nodes.get('#create-button').onclick();
    assert.equal(directory.open, !repo);
    nodes.get('#new-project').onchange({ target: { value: 'C:/selected-project' } });
    assert.equal(form.elements.repo.value, 'C:/selected-project');
    assert.equal(directory.open, false);
  }
});

test('sidebar displays the version returned by the server', async () => {
  const badge = { setAttribute(name, value) { this[name] = value; } };
  const context = {
    api: async path => { assert.equal(path, '/api/health'); return { version: '2.3.4' }; },
    $: selector => { assert.equal(selector, '#app-version'); return badge; },
  };
  await loadVersion({ querySelector: context.$ }, context.api);
  assert.equal(badge.textContent, 'v2.3.4');
  assert.equal(badge['aria-label'], 'Version 2.3.4');
});
test('draft notices stay outside the task editor and keep saving form values', () => {
  for (const key of ['new', 'card']) {
    let placedAfter, saved;
    const heading = { after(element) { placedAfter = ['heading', element]; } };
    const navigation = key === 'card' ? { after(element) { placedAfter = ['navigation', element]; } } : null;
    const notice = {};
    const field = { name: 'title', value: 'Une tâche à reprendre', type: 'text' };
    const password = { name: 'token', value: 'not-for-storage', type: 'password' };
    const form = { elements: [field, password] }; field.form = form;
    const host = { querySelector: selector => selector === '.detail-nav' ? navigation : selector === '.dialog-heading' ? heading : null };
    const context = {
      document: { createElement: () => notice },
      localStorage: { getItem: () => null, setItem(name, value) { saved = { name, value: JSON.parse(value) }; } },
      $: () => ({ children: [] }),
    };
    bindView(createTaskEditor, context);
    context.attachDraft(form, key, host);
    assert.equal(placedAfter[0], key === 'card' ? 'navigation' : 'heading');
    assert.equal(placedAfter[1], notice);
    host.oninput({ target: field });
    assert.equal(saved.name, 'atelier-draft:' + key);
    assert.equal(saved.value.fields.title, field.value);
    assert.equal(saved.value.fields.token, undefined);
    assert.match(notice.textContent, /Brouillon conservé/);
  }
});
