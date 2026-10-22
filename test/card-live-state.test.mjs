import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createCardView } from '../public/views/card.mjs';

function fixture(api = async () => []) {
  const nodes = new Map();
  const actions = [];
  function node() {
    return { value: '', textContent: '', dataset: {}, children: [], options: [], selectedOptions: [], isConnected: true,
      before() {}, after() {}, append() {}, prepend() {}, remove() {}, setAttribute() {},
      querySelector: () => node(), querySelectorAll: () => [], closest: () => node(),
      showModal() { this.open = true; }, close() { this.open = false; }, reportValidity: () => true,
    };
  }
  const $ = selector => {
    if (!nodes.has(selector)) nodes.set(selector, node());
    return nodes.get(selector);
  };
  const fields = Object.fromEntries(['status', 'agent', 'model', 'effort', 'repo', 'branch', 'baseBranch'].map(name => [name, node()]));
  $('#card-form').elements = fields;
  const card = { id: 'a', title: 'Tâche', status: 'todo', agent: 'codex', repo: 'repo' };
  const state = { cards: [card], runs: [], projects: [], settings: {}, queue: { ids: [] } };
  const view = createCardView({ document: { querySelector: $, createElement: node }, getState: () => state,
    api, action: fn => { const pending = fn(); actions.push(pending); return pending; }, refresh: async () => {}, toast() {}, taskAgentFields() {}, attachDraft() {}, clearDraft() {},
    devRunning: () => false, updateClocks() {}, renderBrainstorm() {}, ticketSupports: () => false,
  });
  return { view, card, state, $, fields, actions };
}

test('a card without runs locks during preparation and unlocks after failure without losing edits', async () => {
  const { view, card, $, fields } = fixture();
  await view.openCard(card.id);
  fields.repo.value = 'saisie conservée';
  card.publishing = true;
  await view.updateRun();
  for (const selector of ['#launch', '#prepare-ticket', '#prepare-worktree', '#delete-card', '#card-form button[type=submit]', '#archive-card']) {
    assert.equal($(selector).disabled, true, selector);
  }
  assert.equal(fields.status.disabled, true);
  card.publishing = false;
  await view.updateRun();
  assert.equal($('#launch').disabled, false);
  assert.equal(fields.status.disabled, false);
  assert.equal(fields.repo.value, 'saisie conservée');
  assert.equal($('#prepare-pr').disabled, true);
});

test('a card removed elsewhere closes its detail instead of leaving stale actions', async () => {
  const { view, card, state, $ } = fixture();
  await view.openCard(card.id);
  state.cards = [];
  await view.updateRun();
  assert.equal($('#card-dialog').open, false);
});

test('live updates cannot unlock a pending save and a failed save allows retry', async t => {
  t.mock.method(globalThis, 'FormData', function () { return []; });
  let rejectSave, requests = 0;
  const { view, card, $, fields, actions } = fixture(() => {
    requests++;
    return new Promise((resolve, reject) => { rejectSave = reject; });
  });
  await view.openCard(card.id);
  for (let attempt = 1; attempt <= 2; attempt++) {
    $('#card-form').onsubmit({ preventDefault() {} });
    const saving = actions.at(-1);
    await view.updateRun();
    assert.equal(fields.status.disabled, true);
    assert.equal($('#launch').disabled, true);
    $('#card-form').onsubmit({ preventDefault() {} });
    assert.equal(requests, attempt);
    const failed = assert.rejects(saving, /Connexion interrompue/);
    rejectSave(new Error('Connexion interrompue'));
    await failed;
    assert.equal(fields.status.disabled, false);
    assert.equal($('#launch').disabled, false);
  }
});

test('switching cards during a launch preserves the original agent instructions and selected detail', async t => {
  t.mock.method(globalThis, 'FormData', function () { return []; });
  let save;
  const requests = [];
  const { view, card, state, $, fields } = fixture((...args) => {
    requests.push(args);
    return args[1] === 'PATCH' ? new Promise(resolve => { save = resolve; }) : { id: 'run-a' };
  });
  await view.openCard(card.id);
  fields.agent.value = 'codex'; $('#mode').value = 'analyze'; $('#instructions').value = 'Analyser A';
  const launching = $('#launch').onclick();
  await view.updateRun();
  await $('#launch').onclick();
  assert.equal(requests.length, 1, 'a repeated launch does not save or start twice');
  state.cards.push({ ...card, id: 'b', title: 'Autre tâche' });
  await view.openCard('b');
  fields.agent.value = 'claude'; $('#instructions').value = 'Modifier B';
  save(card);
  await launching;
  assert.deepEqual(requests[1], ['/api/cards/a/run', 'POST', { agent: 'codex', mode: 'analyze', instructions: 'Analyser A' }]);
  assert.equal(view.selectedRun, '');
  assert.equal($('#instructions').value, 'Modifier B');
});

test('a successful save unlocks the refreshed form after the original form is detached', async t => {
  t.mock.method(globalThis, 'FormData', function () { return []; });
  let save;
  const { view, card, $, fields, actions } = fixture(() => new Promise(resolve => { save = resolve; }));
  await view.openCard(card.id);
  $('#card-form').onsubmit({ preventDefault() {} });
  $('#card-form').isConnected = false;
  save(card);
  await actions.at(-1);
  assert.equal(fields.status.disabled, false);
  assert.equal($('#launch').disabled, false);
  assert.equal($('#card-form button[type=submit]').disabled, false);
});

test('an automatic launch follows the run when its worktree preparation finishes later', async t => {
  t.mock.method(globalThis, 'FormData', function () { return []; });
  const requests = [];
  const { view, card, state, $, fields } = fixture((path, method) => {
    requests.push(path);
    if (path.endsWith('/run')) return { ...card, automation: { state: 'running', step: 'development' } };
    return method === 'PATCH' ? card : [];
  });
  await view.openCard(card.id);
  fields.agent.value = 'codex'; $('#mode').value = 'edit';
  await $('#launch').onclick();
  const run = { id: 'agent-run', cardId: card.id, agent: 'codex', status: 'running', activity: 'Analyse du projet', startedAt: '2026-10-23T10:00:00Z' };
  state.runs.push(run);
  $('#run-select').options = [{ value: run.id }]; $('#run-select').value = run.id;
  await view.updateRun();
  assert.equal(view.selectedRun, run.id);
  assert.equal(requests.at(-1), '/api/runs/agent-run/logs?after=0');
  assert.match($('#current-activity').textContent, /Analyse du projet/);
});
