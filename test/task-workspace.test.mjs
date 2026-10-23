import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { codeMarkup, createTaskWorkspace, fileListMarkup } from '../public/views/task-workspace.mjs';

test('unified diff renders old and new line numbers, signs and escaped source', () => {
  const html = codeMarkup({ patch: '--- a/file\n+++ b/file\n@@ -4,2 +4,3 @@\n unchanged\n-old\n+<script>new</script>\n+last\n' });
  assert.match(html, /line-removed.*>5<\/span><span class="line-number"><\/span><code>-old/);
  assert.match(html, /line-added.*line-number">5<\/span><code>\+&lt;script&gt;new/);
  assert.match(html, /line-number">6<\/span><code>\+last/);
  assert.match(html, /added">\+2/);
  assert.match(html, /removed">−1/);
  assert.doesNotMatch(html, /<script>/);
});

test('file previews preserve whitespace, number lines and disclose display limits', () => {
  const html = codeMarkup({ content: '  <div>\n\tvalue\n' });
  assert.match(html, /<code>  &lt;div&gt;/);
  assert.match(html, /line-number">2<\/span><code>\tvalue/);
  assert.match(codeMarkup({ content: '\n'.repeat(10001) }), /10 000/);
  assert.match(codeMarkup({ unavailable: 'Fichier binaire : aperçu textuel indisponible.' }), /Fichier binaire/);
});

test('diff rows show only a status icon and basename, with full paths available accessibly', () => {
  const html = fileListMarkup([{ path: 'src/task.js', status: 'M' }, { path: 'test/task.js', status: 'A' }, { path: 'old.txt', status: 'D' }], { diff: true, selectedPath: 'src/task.js' });
  assert.match(html, /aria-label="Modifié : src\/task.js" title="src\/task.js"/);
  assert.match(html, /file-icon status-M/);
  assert.match(html, /file-icon status-A/);
  assert.match(html, /file-icon status-D/);
  assert.equal((html.match(/class="file-name">task.js/g) || []).length, 2);
  assert.doesNotMatch(html, />Modifié</);
  assert.match(html, /data-path="src\/task.js" aria-current="true"/);
});

test('explorer nests folders first, restores expansion, and opens search matches safely', () => {
  const entries = ['z.txt', 'src/views/card.js', 'src/app.js', '__proto__/safe.txt', '<img>.txt'].map(path => ({ path }));
  const html = fileListMarkup(entries, { expanded: new Set(['src']), selectedPath: 'src/app.js' });
  assert.match(html, /data-directory="src" open/);
  assert.match(html, /data-directory="src\/views" >/);
  assert.ok(html.indexOf('data-directory="src"') < html.indexOf('data-path="z.txt"'));
  assert.ok(html.indexOf('data-directory="src/views"') < html.indexOf('data-path="src/app.js"'));
  assert.match(html, /data-path="__proto__\/safe.txt"/);
  assert.match(html, /&lt;img&gt;.txt/);
  assert.doesNotMatch(html, /<img>/);
  const filtered = fileListMarkup(entries, { filter: 'CARD' });
  assert.match(filtered, /data-directory="src" open/);
  assert.match(filtered, /data-directory="src\/views" open/);
  assert.doesNotMatch(filtered, /data-path="z.txt"/);
  assert.match(fileListMarkup(entries, { filter: 'missing' }), /Aucun fichier/);
  assert.doesNotThrow(() => fileListMarkup([{ path: 'src' }, { path: 'src/added.txt' }]));
});

test('a deletion and an untracked replacement keep separate selection states', () => {
  const entries = [{ path: 'same.txt', status: 'D' }, { path: 'same.txt', status: 'A', untracked: true }];
  for (const selectedUntracked of [false, true]) {
    const html = fileListMarkup(entries, { diff: true, selectedPath: 'same.txt', selectedUntracked });
    const rows = html.match(/<button\b.*?<\/button>/g);
    assert.equal(rows.length, 2);
    assert.match(rows[Number(selectedUntracked)], /aria-current="true"/);
    assert.match(rows[Number(!selectedUntracked)], /aria-current="false"/);
    assert.match(rows[1], /data-untracked="true"/);
  }
});

test('clicking and refreshing a replacement requests its untracked content', async () => {
  const nodes = [], requests = [];
  const buttons = [false, true].map(untracked => ({ dataset: { path: 'same.txt', untracked: String(untracked) }, setAttribute(key, value) { this[key] = value; } }));
  function node() {
    const queries = new Map();
    const element = {
      children: [], dataset: {}, value: '', isConnected: true,
      append(...children) { this.children.push(...children); }, setAttribute() {},
      querySelector(selector) {
        if (selector === 'form' || selector.startsWith('#') || selector === '.run-box' || selector === '.task-editor') return null;
        if (!queries.has(selector)) queries.set(selector, node());
        return queries.get(selector);
      },
      querySelectorAll: selector => selector === '[data-path]' ? buttons : [],
    };
    nodes.push(element); return element;
  }
  const detail = node();
  const view = createTaskWorkspace({ document: { querySelector: () => detail, createElement: node }, api: async path => {
    requests.push(path);
    return path.endsWith('/changes')
      ? { root: '/repo', branch: 'task', base: '12345678', files: [{ path: 'same.txt', status: 'D' }, { path: 'same.txt', status: 'A', untracked: true }] }
      : { patch: '@@ -0,0 +1 @@\n+replacement' };
  } });
  const card = { id: 'task', status: 'review' };
  view.reset(card); view.mount(card);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(requests.at(-1), '/api/cards/task/file-diff?path=same.txt&untracked=false');
  await buttons[1].onclick();
  assert.equal(requests.at(-1), '/api/cards/task/file-diff?path=same.txt&untracked=true');
  const panel = nodes.find(node => node.id === 'task-panel-diff');
  await panel.querySelector('.refresh-files').onclick();
  assert.equal(requests.at(-1), '/api/cards/task/file-diff?path=same.txt&untracked=true');
  assert.equal(buttons[0]['aria-current'], 'false');
  assert.equal(buttons[1]['aria-current'], 'true');
});

test('review opens the diff, tabs load lazily and keyboard navigation preserves the mounted panels', () => {
  const nodes = [], requests = [];
  function node() {
    const queries = new Map();
    const element = { children: [], dataset: {}, attrs: {}, value: '', isConnected: true,
      append(...children) { this.children.push(...children); },
      setAttribute(key, value) { this.attrs[key] = value; }, focus() { this.focused = true; },
      querySelector(selector) {
        if (selector === 'form' || selector === '.dialog-heading' || selector.startsWith('#') || selector === '.run-box') return null;
        if (!queries.has(selector)) queries.set(selector, node());
        return queries.get(selector);
      },
      querySelectorAll() { return []; },
    };
    nodes.push(element); return element;
  }
  const detail = node();
  const view = createTaskWorkspace({ document: { querySelector: () => detail, createElement: node }, api: path => { requests.push(path); return new Promise(() => {}); } });
  const card = { id: 'review', status: 'review' };
  view.reset(card); view.mount(card);
  const get = id => nodes.find(element => element.id === id);
  assert.equal(get('task-tab-diff').attrs['aria-selected'], 'true');
  assert.equal(get('task-panel-details').hidden, true);
  assert.deepEqual(requests, ['/api/cards/review/changes']);
  const details = get('task-panel-details');
  details.draft = 'unsaved';
  get('task-tab-diff').onkeydown({ key: 'ArrowRight', preventDefault() {} });
  assert.equal(get('task-tab-files').focused, true);
  assert.deepEqual(requests, ['/api/cards/review/changes', '/api/cards/review/files']);
  get('task-tab-details').onclick();
  assert.equal(details.hidden, false);
  assert.equal(details.draft, 'unsaved');
  get('task-tab-files').onclick();
  assert.equal(requests.length, 2, 'revisiting a tab preserves its loaded content');
  view.reset({ ...card, status: 'todo' }); view.mount(card);
  assert.equal(nodes.filter(element => element.id === 'task-panel-details').at(-1).hidden, false);
});
