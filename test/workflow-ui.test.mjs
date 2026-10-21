import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createCardView } from '../public/views/card.mjs';
import { createTimeView } from '../public/views/time.mjs';
import { createWorkspaceWorkflows } from '../public/views/workspace-workflows.mjs';
import { bindView } from './helpers/view-context.mjs';
import { prLabel, cleanupCandidate } from '../public/workflow.mjs';
import { connectorSupports, repositoryProvider } from '../public/connector-availability.mjs';

const esc = value => String(value ?? '').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

test('work report escapes the date before inserting it in an HTML attribute', () => {
  const context = { esc, summaryDay: '\"><img src=x onerror=alert(1)>', scopedCards: () => [], workReport: () => '', state: { cards: [], runs: [], times: [] } };
  const date = {};
  context.$ = selector => selector === '#report-day' ? date : null;
  context.board = () => {};
  bindView(createTimeView, context).bindReport();
  date.onchange({ target: { value: context.summaryDay } });
  const html = context.reportMarkup();
  assert.match(html, /value="&quot;&gt;&lt;img/);
  assert.doesNotMatch(html, /<img/i);
});

test('PR details and cleanup candidates escape service data and show incomplete checks explicitly', () => {
  const context = {  esc, prLabel, cleanupCandidate, connectorSupports, repositoryProvider, worktreeChecks: [], state: { runs: [] }, devRunning: () => false };
  bindView(createCardView, context);
  context.getWorktreeChecks = () => context.worktreeChecks;
  bindView(createWorkspaceWorkflows, context);
  const card = { id: 'a', title: '<script>danger</script>', status: 'done', worktree: {}, pullRequest: { number: 7, url: 'https://example.invalid/pr/7', checkedAt: new Date().toISOString(), refreshError: '<img src=x>', checksError: '<svg>', checks: [{ name: '<script>', state: 'pending' }] } };
  const markup = context.prMarkup(card);
  assert.doesNotMatch(markup, /<img|<svg|<script>/i); assert.match(markup, /&lt;img/); assert.match(markup, /règles de branche et pipelines restent à vérifier/);
  const cleanup = context.cleanupMarkup([card, { id: 'b', title: 'En cours', worktree: {}, status: 'doing' }]);
  assert.match(cleanup, /data-clean-candidate="a"/); assert.doesNotMatch(cleanup, /data-clean-candidate="b"/); assert.doesNotMatch(cleanup, /<script>/i);
});

test('delivery cleanup confirms once and never requests forced removal', async () => {
  for (const accepted of [false, true]) {
    const calls = [], button = { dataset: { cleanCandidate: 'a' }, isConnected: true };
    const context = {
      document: { querySelectorAll: selector => selector === '[data-clean-candidate]' ? [button] : [] },
      $: () => null, bindPrActions() {}, action: fn => fn(), confirmAction: async () => accepted,
      cardPath: id => '/api/cards/' + id, api: async (...args) => calls.push(args), async refresh() {}, toast() {}, worktreeChecks: [],
    };
    context.clearWorktreeChecks = () => { context.worktreeChecks = []; };
    bindView(createWorkspaceWorkflows, context);
    context.bindWorkflowActions(); await button.onclick();
    assert.equal(JSON.stringify(calls), JSON.stringify(accepted ? [['/api/cards/a/worktree', 'DELETE', {}]] : []));
  }
});
