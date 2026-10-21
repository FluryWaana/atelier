import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { prLabel, cleanupCandidate } from '../public/workflow.mjs';
import { connectorSupports, repositoryProvider } from '../public/connector-availability.mjs';

const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
const esc = value => String(value ?? '').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('"', '&quot;');

test('PR details and cleanup candidates escape service data and show incomplete checks explicitly', () => {
  const context = vm.createContext({ msg, markupText, locale, messageText, localizeModule, esc, prLabel, cleanupCandidate, connectorSupports, repositoryProvider, worktreeChecks: [], state: { runs: [] }, devRunning: () => false });
  vm.runInContext(source.slice(source.indexOf('function prMarkup('), source.indexOf('function renderWorkflowDetail(')), context);
  vm.runInContext(source.slice(source.indexOf('function cleanupMarkup('), source.indexOf('function queueMarkup(')), context);
  const card = { id: 'a', title: '<script>danger</script>', status: 'done', worktree: {}, pullRequest: { number: 7, url: 'https://example.invalid/pr/7', checkedAt: new Date().toISOString(), refreshError: '<img src=x>', checksError: '<svg>', checks: [{ name: '<script>', state: 'pending' }] } };
  const markup = context.prMarkup(card);
  assert.doesNotMatch(markup, /<img|<svg|<script>/); assert.match(markup, /&lt;img/); assert.match(markup, /règles de branche et pipelines restent à vérifier/);
  const cleanup = context.cleanupMarkup([card, { id: 'b', title: 'En cours', worktree: {}, status: 'doing' }]);
  assert.match(cleanup, /data-clean-candidate="a"/); assert.doesNotMatch(cleanup, /data-clean-candidate="b"/); assert.doesNotMatch(cleanup, /<script>/);
});

test('delivery cleanup confirms once and never requests forced removal', async () => {
  for (const accepted of [false, true]) {
    const calls = [], button = { dataset: { cleanCandidate: 'a' }, isConnected: true };
    const context = vm.createContext({ msg, markupText, locale, messageText, localizeModule,
      document: { querySelectorAll: selector => selector === '[data-clean-candidate]' ? [button] : [] },
      $: () => null, bindPrActions() {}, action: fn => fn(), confirmAction: async () => accepted,
      cardPath: id => '/api/cards/' + id, api: async (...args) => calls.push(args), async refresh() {}, toast() {}, worktreeChecks: [],
    });
    vm.runInContext(source.slice(source.indexOf('function bindWorkflowActions(')), context);
    context.bindWorkflowActions(); await button.onclick();
    assert.equal(JSON.stringify(calls), JSON.stringify(accepted ? [['/api/cards/a/worktree', 'DELETE', {}]] : []));
  }
});
