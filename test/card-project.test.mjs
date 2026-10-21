import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createBoardView } from '../public/views/board.mjs';

function render(card, projects = [], runs = [], save = async () => {}) {
  const elements = new Map();
  let renders = 0;
  const popover = { style: {}, offsetHeight: 52, previousElementSibling: { getBoundingClientRect: () => ({ bottom: 790, right: 900 }) } };
  const stop = { dataset: { cardStopAgent: 'running-id' }, disabled: false }, remove = { dataset: { deleteCard: card.id }, disabled: false };
  const menu = {}, archive = { dataset: { archiveCard: card.id }, disabled: false };
  const select = { dataset: { cardProject: card.id }, value: 'D:\\Projet' };
  const document = {
    querySelector(key) { if (!elements.has(key)) elements.set(key, { value: '' }); return elements.get(key); },
    querySelectorAll: key => key === '[data-card-project]' ? [select] : key === '.card-actions' ? [menu] : key === '.card-action-menu' ? [popover] : key === '[data-archive-card]' ? [archive] : key === '[data-card-stop-agent]' ? [stop] : key === '[data-delete-card]' ? [remove] : [],
  };
  const state = { cards: [card], projects, runs, settings: { platform: 'win32' } };
  const rerender = () => { renders++; view.render({ cards: [card], state, query: '', focusFilter: 'all' }); };
  const view = createBoardView({ document, window: { innerWidth: 1000, innerHeight: 800 }, api: save,
    confirmAction: async () => true, action: fn => fn(), async refresh() {}, toast() {}, openCard() {}, rerender,
  });
  rerender();
  return { get html() { return elements.get('#board').innerHTML; }, get renders() { return renders; }, select, menu, archive, popover, stop, remove };
}

test('cards show a project selector, including missing and unlisted projects', () => {
  const card = { id: 'a', title: 'Une tâche', status: 'todo', agent: 'codex', repo: '' };
  assert.match(render(card).html, /value="" selected>Sans projet/);
  const projects = [{ repo: 'D:\\Projet', name: 'Projet <A>' }];
  assert.match(render({ ...card, repo: 'd:\\projet' }, projects).html, /selected>Projet &lt;A&gt;/);
  assert.match(render({ ...card, repo: 'D:\\Autre' }).html, /selected>Autre/);
  assert.match(render({ ...card, worktree: { path: 'work' } }).html, /project-pill[^>]* disabled/);
  assert.match(render(card, [], [{ cardId: 'a', status: 'running' }]).html, /project-pill[^>]* disabled/);
});

test('selecting a project saves only the repository and restores the selector on failure', async () => {
  const card = { id: 'a', title: 'Tâche', status: 'todo', agent: 'codex' };
  const calls = [];
  const { select } = render(card, [], [], async (...args) => calls.push(args));
  let stopped = false;
  select.onclick({ stopPropagation() { stopped = true; } });
  assert.equal(stopped, true);
  await select.onchange();
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/cards/a', 'PATCH', { repo: 'D:\\Projet' }]]));
  const failure = render(card, [], [], async () => { throw new Error('Refus'); });
  await assert.rejects(failure.select.onchange(), /Refus/);
  assert.equal(failure.renders, 2);
  assert.doesNotMatch(failure.html, /project-pill[^>]* disabled/);
});


test('idle cards in every column offer archive and local deletion, running agents offer stop', () => {
  const card = { id: 'a', title: 'Tâche <A>', agent: 'codex' };
  for (const status of ['todo', 'doing', 'review', 'done', 'standby']) {
    const html = render({ ...card, status }).html;
    assert.match(html, /data-archive-card="a"  title=/);
    assert.match(html, /data-delete-card="a"/);
    assert.doesNotMatch(html, /data-card-stop-agent=/);
  }
  assert.match(render({ ...card, status: 'todo', publishing: true }).html, /data-archive-card="a" disabled/);
  const running = render({ ...card, status: 'doing' }, [], [{ cardId: 'a', status: 'completed' }, { id: 'running-id', cardId: 'a', status: 'running' }]).html;
  assert.match(running, /data-card-stop-agent="running-id"/);
  assert.doesNotMatch(running, /data-(archive|delete)-card=/);
  const stopped = render({ ...card, status: 'doing' }, [], [{ cardId: 'a', status: 'cancelled' }]).html;
  assert.match(stopped, /data-archive-card="a"/);
  assert.match(stopped, /data-delete-card="a"/);
});

test('card menu prevents opening or dragging the card and archives through the existing endpoint', async () => {
  const calls = [];
  const card = { id: 'a', title: 'Tâche', status: 'done', agent: 'codex' };
  const { menu, archive } = render(card, [], [], async (...args) => calls.push(args));
  let stopped = 0, prevented = 0;
  const event = { stopPropagation() { stopped++; }, preventDefault() { prevented++; } };
  menu.onclick(event); menu.ondragstart(event);
  assert.equal(stopped, 2); assert.equal(prevented, 1);
  await archive.onclick();
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/cards/a/archive', 'POST', { archived: true }]]));
  assert.equal(archive.disabled, false);
  const failed = render(card, [], [], async () => { throw new Error('Refus'); });
  await assert.rejects(failed.archive.onclick(), /Refus/);
  assert.equal(failed.archive.disabled, false);
});


test('card menu stays aligned to its button and within the viewport', () => {
  const { popover } = render({ id: 'a', title: 'Tâche', status: 'done', agent: 'codex' });
  popover.ontoggle({ newState: 'open' });
  assert.equal(popover.style.top, '740px');
  assert.equal(popover.style.right, '100px');
});


test('card actions cancel the actual run and delete only the local card', async () => {
  const calls = [];
  const { stop, remove } = render({ id: 'a', title: 'Tâche', status: 'doing', agent: 'codex' }, [], [], async (...args) => calls.push(args));
  await stop.onclick(); await remove.onclick();
  assert.equal(JSON.stringify(calls), JSON.stringify([['/api/runs/running-id/cancel', 'POST', {}], ['/api/cards/a', 'DELETE', {}]]));
  assert.equal(stop.disabled, false); assert.equal(remove.disabled, false);
});
