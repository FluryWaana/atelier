import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import { matchesFocus } from '../public/ui-helpers.mjs';

const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
const esc = value => String(value ?? '').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');
function view(card) {
  const host = { dataset: {} }, resume = {}, stop = {}, requests = [];
  const context = vm.createContext({
    state: { cards: [card], projects: [], times: [] }, selected: card.id, esc,
    $: selector => selector === '#automation-status' ? host : selector === '#resume-automation' && card.automation?.state === 'error' ? resume : selector === '#stop-automation' && card.automation?.state === 'running' ? stop : null,
    cardPath: id => '/api/cards/' + id, action: fn => fn(), api: async (...args) => requests.push(args), refresh: async () => {}, renderDetail() {}, updateRun: async () => {},
    scopedCards: () => [card], matchesFocus,
  });
  vm.runInContext(source.slice(source.indexOf('function updateAutomation('), source.indexOf('function updateDevServers(')), context);
  vm.runInContext(source.slice(source.indexOf('function overviewMarkup('), source.indexOf('function openCommands(')), context);
  context.updateAutomation();
  return { host, resume, stop, requests, overview: () => context.overviewMarkup([card], []) };
}
test('workflow UI escapes logs and errors and resumes or stops the matching card', async () => {
  const card = { id: 'ticket', title: 'Titre', status: 'standby', automation: { state: 'error', step: 'tests', cycles: 0, message: '<script>erreur</script>', tests: { state: 'failed', command: 'npm test', output: '<img src=x>' } } };
  const failed = view(card);
  assert.match(failed.host.innerHTML, /Vérifications · Échec/);
  assert.match(failed.host.innerHTML, /&lt;img/); assert.doesNotMatch(failed.host.innerHTML, /<script>/);
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
  const context = vm.createContext({ state: { cards: [{ id: 'ticket' }], runs: [run] }, selectedRun: 'run', esc, runNames: { completed: 'Terminé' }, $: selector => selector === '#pr-review-result' ? host : selector === '#check-review' ? check : null });
  vm.runInContext(source.slice(source.indexOf('function renderReviewReport('), source.indexOf('function projectConfig(')), context);
  context.renderReviewReport('ticket');
  assert.match(host.innerHTML, /Impossible à vérifier · &lt;Critère>/); assert.match(host.innerHTML, /&lt;script>/); assert.doesNotMatch(host.innerHTML, /<script>/);
  assert.doesNotMatch(host.innerHTML, /type="checkbox"/);
  delete run.review.criteria; context.renderReviewReport('ticket');
  assert.match(host.innerHTML, /ancienne review ne contient pas de bilan/);
});
