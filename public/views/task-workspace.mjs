import { esc, cardPath } from '../view-helpers.mjs';
import { msg, markupText, messageText } from '../i18n.mjs';
import { highlightedLines } from '../syntax-highlight.mjs';

const lineLimit = 10000;
export function codeMarkup({ content, patch, unavailable, path }) {
  if (unavailable) return `<p class="hint">${esc(messageText(unavailable))}</p>`;
  let oldLine = 0, newLine = 0, inHunk = false, added = 0, removed = 0;
  const lines = (patch ?? content ?? '').split('\n');
  if (lines.at(-1) === '') lines.pop();
  const highlighted = patch === undefined ? highlightedLines(lines.slice(0, lineLimit).join('\n'), path) : [];
  const rows = lines.slice(0, lineLimit).map((line, index) => {
    if (patch === undefined) return `<div class="code-line"><span class="line-number">${index + 1}</span><code>${highlighted[index] || ' '}</code></div>`;
    const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunk) { oldLine = Number(hunk[1]); newLine = Number(hunk[2]); inHunk = true; return `<div class="diff-hunk">${esc(line)}</div>`; }
    if (!inHunk) return `<div class="diff-meta">${esc(line)}</div>`;
    const kind = line[0];
    if (!['+', '-', ' '].includes(kind)) return `<div class="diff-meta">${esc(line)}</div>`;
    if (kind === '+') added++;
    if (kind === '-') removed++;
    return `<div class="code-line diff-line ${kind === '+' ? 'line-added' : kind === '-' ? 'line-removed' : ''}"><span class="line-number">${kind === '+' ? '' : oldLine++}</span><span class="line-number">${kind === '-' ? '' : newLine++}</span><code>${esc(line)}</code></div>`;
  }).join('');
  return `${patch !== undefined ? `<p class="diff-counts"><span class="added">+${added}</span> <span class="removed">−${removed}</span> ${markupText("lignes affichées")}</p>` : ''}${lines.length > lineLimit ? `<p class="hint">${markupText("Aperçu limité aux 10 000 premières lignes.")}</p>` : ''}<div class="code-view" tabindex="0" aria-label="${markupText(patch === undefined ? 'Contenu du fichier' : 'Diff du fichier')}">${rows || `<p class="hint">${markupText("Fichier vide ou changement de métadonnées uniquement.")}</p>`}</div>`;
}

const icon = (paths, className = '') => `<svg class="file-icon ${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const statusIcons = {
  A: '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M12 8v8M8 12h8"/>',
  M: '<path d="m15 5 4 4M4 20l5-1L20 8a3 3 0 0 0-4-4L5 15z"/>',
  D: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5m4-5v5"/>',
};
const fileIcon = path => icon('<path d="M14 3H5v18h14V8zM14 3v5h5"/><path d="M8 12h8M8 16h6"/>', /\.(?:[cm]?[jt]sx?|html?|css|scss|py|go|rs|cs|java)$/.test(path) ? 'file-code-icon' : 'file-text-icon');

export function fileListMarkup(entries, { diff = false, filter = '', selectedPath = '', selectedUntracked = false, expanded = new Set() } = {}) {
  const statuses = { A: msg('Créé'), M: msg('Modifié'), D: msg('Supprimé'), T: msg('Type modifié') };
  const visible = entries.filter(entry => entry.path.toLocaleLowerCase().includes(filter.toLocaleLowerCase()));
  const file = entry => `<button type="button" class="file-entry" data-path="${esc(entry.path)}" aria-current="${entry.path === selectedPath && Boolean(entry.untracked) === selectedUntracked ? 'true' : 'false'}" aria-label="${esc(diff ? `${statuses[entry.status] || entry.status} : ${entry.path}` : entry.path)}" title="${esc(entry.path)}" data-untracked="${Boolean(entry.untracked)}">${diff ? icon(statusIcons[entry.status] || statusIcons.M, `status-${esc(entry.status)}`) : fileIcon(entry.path)}<span class="file-name">${esc(entry.path.split('/').at(-1))}</span></button>`;
  if (!visible.length) return `<p class="hint file-empty">${markupText('Aucun fichier.')}</p>`;
  if (diff) return visible.map(file).join('');
  const root = new Map();
  for (const entry of visible) {
    const parts = entry.path.split('/');
    let parent = root, path = '';
    for (const [index, name] of parts.entries()) {
      path += (path ? '/' : '') + name;
      if (index === parts.length - 1) { if (!parent.get(name)?.children) parent.set(name, { ...entry, name }); }
      else {
        if (!parent.get(name)?.children) parent.set(name, { name, path, children: new Map() });
        parent = parent.get(name).children;
      }
    }
  }
  function folder(children) {
    return [...children.values()].sort((a, b) => Number(Boolean(b.children)) - Number(Boolean(a.children)) || a.name.localeCompare(b.name)).map(entry => entry.children
      ? `<details class="file-directory" data-directory="${esc(entry.path)}" ${filter || expanded.has(entry.path) ? 'open' : ''}><summary title="${esc(entry.path)}">${icon('<path d="m9 5 7 7-7 7"/>', 'folder-chevron')}${icon('<path d="M3 7V5h7l2 3h9v12H3z"/>', 'folder-icon')}<span class="file-name">${esc(entry.name)}</span></summary><div class="file-children">${folder(entry.children)}</div></details>`
      : file(entry)).join('');
  }
  return folder(root);
}

function createFileBrowser({ host, card, api, diff }) {
  let request = 0, entries = [], selectedPath = '', selectedUntracked = false;
  const expanded = new Set();
  host.innerHTML = `<div class="file-toolbar"><div><h3 title="${markupText(diff ? 'Comparaison avec le point de départ de la tâche, commits et modifications locales inclus.' : 'Lecture seule · fichiers suivis et nouveaux fichiers non ignorés par Git.')}">${markupText(diff ? 'Changements de la tâche' : 'Fichiers du projet')}</h3><p class="hint file-context"></p></div><button type="button" class="refresh-files">${markupText('Actualiser')}</button></div><div class="file-browser"><aside class="file-sidebar"><div class="file-sidebar-heading">${markupText(diff ? 'Changements' : 'Explorateur')}<span class="file-count"></span></div><label class="file-search"><span class="sr-only">${markupText('Filtrer les fichiers')}</span><input type="search" class="file-filter" autocomplete="off" placeholder="${markupText('Filtrer les fichiers')}"></label><p class="file-message" role="status"></p><nav class="file-list" aria-label="${markupText('Fichiers')}"></nav></aside><section class="file-preview" aria-label="${markupText('Aperçu du fichier')}"><p class="hint">${markupText('Sélectionne un fichier pour le lire.')}</p></section></div>`;
  const $ = selector => host.querySelector(selector);
  const current = id => id === request && host.isConnected;
  function renderList() {
    const filter = $('.file-filter').value;
    $('.file-list').innerHTML = fileListMarkup(entries, { diff, filter, selectedPath, selectedUntracked, expanded });
    $('.file-list').querySelectorAll('[data-path]').forEach(button => button.onclick = () => selectFile(button.dataset.path, button.dataset.untracked === 'true'));
    $('.file-list').querySelectorAll('[data-directory]').forEach(directory => {
      directory.ontoggle = () => { if (!filter) { if (directory.open) expanded.add(directory.dataset.directory); else expanded.delete(directory.dataset.directory); } };
      directory.querySelector('summary').onkeydown = event => {
        if (event.key === 'ArrowRight') { event.preventDefault(); directory.open = true; }
        if (event.key === 'ArrowLeft') { event.preventDefault(); directory.open = false; }
      };
    });
  }
  async function selectFile(path, untracked = false) {
    selectedPath = path; selectedUntracked = untracked;
    $('.file-list').querySelectorAll('[data-path]').forEach(button => button.setAttribute('aria-current', String(button.dataset.path === path && (button.dataset.untracked === 'true') === untracked)));
    const id = ++request;
    $('.file-preview').innerHTML = `<p role="status">${markupText('Chargement…')}</p>`;
    try {
      const file = await api(cardPath(card.id) + (diff ? '/file-diff' : '/file') + '?path=' + encodeURIComponent(path) + (diff ? '&untracked=' + untracked : ''));
      if (!current(id)) return;
      $('.file-preview').innerHTML = `<h4 class="file-title">${esc(path)}</h4>${codeMarkup(file)}`;
    } catch (error) {
      if (current(id)) $('.file-preview').innerHTML = `<p class="inline-error" role="alert">${esc(messageText(error.message))}</p>`;
    }
  }
  async function load() {
    const id = ++request;
    $('.file-message').textContent = msg('Chargement…');
    $('.refresh-files').disabled = true;
    entries = []; renderList();
    $('.file-preview').innerHTML = '';
    try {
      const result = await api(cardPath(card.id) + (diff ? '/changes' : '/files'));
      if (!current(id)) return;
      entries = diff ? result.files : result.files.map(path => ({ path }));
      $('.file-context').textContent = `${result.root}${diff ? ` · ${result.branch} · ${result.base.slice(0, 8)}` : result.worktree ? msg(' · Worktree de la tâche') : msg(' · Dépôt source')}`;
      $('.file-count').textContent = String(entries.length);
      $('.file-count').title = msg('{0} fichier(s)', entries.length);
      $('.file-message').textContent = entries.length ? '' : msg(diff ? 'Aucun changement dans cette tâche.' : 'Aucun fichier.');
      if (!selectedPath && entries.length) {
        const parts = entries[0].path.split('/'); parts.pop();
        for (let i = 1; i <= parts.length; i++) expanded.add(parts.slice(0, i).join('/'));
      }
      renderList();
      if (entries.length) {
        const entry = entries.find(entry => entry.path === selectedPath && Boolean(entry.untracked) === selectedUntracked) || entries[0];
        await selectFile(entry.path, Boolean(entry.untracked));
      }
    } catch (error) {
      if (current(id)) $('.file-message').textContent = messageText(error.message);
    } finally { if (host.isConnected) $('.refresh-files').disabled = false; }
  }
  $('.file-filter').oninput = renderList;
  $('.refresh-files').onclick = load;
  return load();
}

export function createTaskWorkspace({ document, api }) {
  let activeTab = 'details';
  let show = () => {};
  function reset(card) { activeTab = card?.status === 'review' ? 'diff' : 'details'; }
  function mount(card) {
    const detail = document.querySelector('#card-detail');
    const tabs = document.createElement('nav'); tabs.className = 'task-tabs'; tabs.setAttribute('role', 'tablist'); tabs.setAttribute('aria-label', msg('Sections de la tâche'));
    const panels = {};
    for (const key of ['details', 'activity', 'diff', 'files']) {
      const panel = document.createElement('section'); panel.id = 'task-panel-' + key; panel.className = 'task-panel';
      panel.setAttribute('role', 'tabpanel'); panel.setAttribute('aria-labelledby', 'task-tab-' + key); panel.tabIndex = 0;
      panels[key] = panel;
    }
    for (const child of [...detail.children]) if (child !== detail.querySelector('.task-heading')) panels.details.append(child);
    for (const selector of ['.run-box', '#automation-status', '#agent-summary', '#run-details']) {
      const element = detail.querySelector(selector) || panels.details.querySelector(selector);
      if (element) panels.activity.append(element);
    }
    const activityAside = document.createElement('aside'); activityAside.className = 'task-aside';
    for (const selector of ['#prepare-ticket', '#brainstorm-ticket']) {
      const element = panels.details.querySelector(selector)?.closest('section');
      if (element) activityAside.append(element);
    }
    function columns(panel, mainChildren, aside) {
      const layout = document.createElement('div'); layout.className = 'task-columns';
      const main = document.createElement('div'); main.className = 'task-main'; main.append(...mainChildren);
      layout.append(main, aside); panel.append(layout);
    }
    const editor = panels.details.querySelector('.task-editor');
    const detailsAside = document.createElement('aside'); detailsAside.className = 'task-aside';
    for (const child of [...panels.details.children]) if (child !== editor) detailsAside.append(child);
    columns(panels.details, editor ? [editor] : [], detailsAside);
    columns(panels.activity, [...panels.activity.children], activityAside);
    detail.append(tabs, ...Object.values(panels));
    const buttons = [], loaded = new Set();
    function select(key) {
      activeTab = key;
      for (const [name, panel] of Object.entries(panels)) panel.hidden = name !== key;
      for (const button of buttons) { button.setAttribute('aria-selected', String(button.dataset.tab === key)); button.tabIndex = button.dataset.tab === key ? 0 : -1; }
      if (['diff', 'files'].includes(key) && !loaded.has(key)) {
        loaded.add(key);
        createFileBrowser({ host: panels[key], card, api, diff: key === 'diff' });
      }
    }
    show = select;
    for (const [key, label] of [['details', 'La tâche'], ['activity', 'Agent & activité'], ['diff', 'Diff'], ['files', 'Fichiers']]) {
      const button = document.createElement('button'); button.type = 'button'; button.id = 'task-tab-' + key; button.dataset.tab = key; button.textContent = msg(label);
      button.setAttribute('role', 'tab'); button.setAttribute('aria-controls', panels[key].id);
      button.onclick = () => select(key);
      button.onkeydown = event => {
        const index = buttons.indexOf(button);
        const next = { ArrowRight: (index + 1) % 4, ArrowLeft: (index + 3) % 4, Home: 0, End: 3 }[event.key];
        if (next === undefined) return;
        event.preventDefault(); buttons[next].focus(); select(buttons[next].dataset.tab);
      };
      buttons.push(button); tabs.append(button);
    }
    panels.details.querySelector('form')?.addEventListener('invalid', () => select('details'), true);
    select(activeTab);
  }
  return { reset, mount, show: key => show(key) };
}
