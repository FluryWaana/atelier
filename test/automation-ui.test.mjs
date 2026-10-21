import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createAutomationView, createCardView } from '../public/views/card.mjs';
import { overviewMarkup } from '../public/views/overview.mjs';
import { bindView } from './helpers/view-context.mjs';
import { matchesFocus } from '../public/ui-helpers.mjs';

const esc = value => String(value ?? '').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');
function view(card) {
  const host = { dataset: {} }, resume = {}, stop = {}, requests = [];
  const context = {
    state: { cards: [card], projects: [], times: [] }, selected: card.id, esc,
    $: selector => selector === '#automation-status' ? host : selector === '#resume-automation' && card.automation?.state === 'error' ? resume : selector === '#stop-automation' && card.automation?.state === 'running' ? stop : null,
    cardPath: id => '/api/cards/' + id, action: fn => fn(), api: async (...args) => requests.push(args), refresh: async () => {}, renderDetail() {}, updateRun: async () => {},
    scopedCards: () => [card], matchesFocus,
  };
  Object.assign(context, createAutomationView({ document: { querySelector: context.$ }, getState: () => context.state, api: context.api, action: context.action, refresh: context.refresh, onChanged: async () => {} }));

  context.updateAutomation(card.id);
  return { host, resume, stop, requests, overview: () => overviewMarkup([card], [], context.state, [card]) };
}
test('workflow UI escapes logs and errors and resumes or stops the matching card', async () => {
  const card = { id: 'ticket', title: 'Titre', status: 'standby', automation: { state: 'error', step: 'tests', cycles: 0, message: '<script>erreur</script>', tests: { state: 'failed', command: 'npm test', output: '<img src=x>' } } };
  const failed = view(card);
  assert.match(failed.host.innerHTML, /Vérifications · Échec/);
  assert.match(failed.host.innerHTML, /&lt;img/); assert.doesNotMatch(failed.host.innerHTML, /<script>/i);
  await failed.resume.onclick(); assert.equal(failed.requests[0][0], '/api/cards/ticket/resume-automation');
  const running = view({ ...card, automation: { ...card.automation, state: 'running' } });
  await running.stop.onclick(); assert.equal(running.requests[0][0], '/api/cards/ticket/stop-automation');
  assert.equal(matchesFocus(card, [], 'attention'), true);
});
test('final notification persists in the overview with its PR link until the task is done', () => {
  const card = { id: 'ticket', title: 'Titre', status: 'review', pullRequest: { number: 1, url: 'https://example.invalid/pr/1' }, automation: { state: 'ready', step: 'review', cycles: 1, message: 'PR prête pour ton contrôle.', counts: { P0: 0, P1: 0, P2: 2, P3: 0, suggestion: 1 } } };
  const ready = view(card);
  assert.match(ready.overview(), /PR prête pour ton contrôle/); assert.match(ready.overview(), /Suggestions : 1/);
  assert.match(ready.overview(), /https:\/\/example.invalid\/pr\/1/);
  assert.doesNotMatch(view({ ...card, status: 'done' }).overview(), /PR prête pour ton contrôle/);
});

test('functional coverage counts stay visible on the card and in the persistent notification', () => {
  const card = { id: 'ticket', title: 'Titre', status: 'standby', automation: { state: 'blocked', step: 'review', cycles: 0, message: 'Intervention humaine nécessaire.', acceptance: { total: 3, covered: 1, notCovered: 1, unverifiable: 1 } } };
  const result = view(card);
  assert.match(result.host.innerHTML, /1\/3 couverts · 1 non couverts · 1 impossibles à vérifier/);
  assert.match(result.overview(), /1\/3 couverts/);
});

test('criterion evidence is escaped and old reviews are explicitly identified', () => {
  const host = { dataset: {}, insertAdjacentHTML(position, html) { this.innerHTML += html; } }, check = {};
  const run = { id: 'run', cardId: 'ticket', kind: 'pr-review', status: 'completed', startedAt: '2026-10-20T12:00:00Z', reviewContext: {}, review: { summary: 'Bilan', findings: [], criteria: [{ index: 0, text: '<Critère>', status: 'unverifiable', evidence: '<script>preuve</script>' }] } };
  const context = {  state: { cards: [{ id: 'ticket' }], runs: [run] }, selectedRun: 'run', esc, runNames: { completed: 'Terminé' }, $: selector => selector === '#pr-review-result' ? host : selector === '#check-review' ? check : null };
  bindView(createCardView, context);
  context.renderReviewReport('ticket');
  assert.match(host.innerHTML, /Impossible à vérifier · &lt;Critère&gt;/); assert.match(host.innerHTML, /&lt;script&gt;/); assert.doesNotMatch(host.innerHTML, /<script>/i);
  assert.doesNotMatch(host.innerHTML, /type="checkbox"/);
  delete run.review.criteria; context.renderReviewReport('ticket');
  assert.match(host.innerHTML, /ancienne review ne contient pas de bilan/);
});

test('overview renders the state of recent runs through the shared display helper', () => {
  const card = { id: 'task', title: 'Recent task', status: 'review' };
  const run = { id: 'run', cardId: card.id, agent: 'codex', status: 'completed' };
  const html = overviewMarkup([card], [run], { cards: [card], times: [] }, [card]);
  assert.match(html, /data-agent-state="run"/);
  assert.match(html, /Terminé · à vérifier/);
});
