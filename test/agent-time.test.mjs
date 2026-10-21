import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { spawnSync } from 'node:child_process';
import { agentTime } from '../public/ui-helpers.mjs';

test('agent timestamps in milliseconds become seconds and display with the correct units', () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const context = vm.createContext({});
  vm.runInContext(source.slice(source.indexOf('function duration('), source.indexOf('function agentState(')), context);
  const startedAt = '2026-10-20T10:00:00.000Z';
  const runs = [{ cardId: 'a', status: 'completed', startedAt, endedAt: '2026-10-20T11:02:03.456Z' }];
  const seconds = agentTime(runs)[0].seconds;
  assert.equal(seconds, 3723);
  assert.equal(context.duration(seconds), '1 h 02 min 03 s');
  for (const [seconds, expected] of [[0, '0 h 00 min 00 s'], [59, '0 h 00 min 59 s'], [60, '0 h 01 min 00 s'], [3600, '1 h 00 min 00 s'], [90061, '25 h 01 min 01 s']]) {
    assert.equal(context.duration(seconds), expected);
  }
});

test('agent time accumulates executions per task, including reviews and stopped runs', () => {
  const startedAt = '2026-10-20T10:00:00Z';
  const runs = [
    { cardId: 'a', status: 'completed', startedAt, endedAt: '2026-10-20T10:01:00Z' },
    { cardId: 'a', kind: 'pr-review', status: 'running', startedAt: '2026-10-20T10:01:00Z' },
    { cardId: 'b', status: 'failed', startedAt, endedAt: '2026-10-20T10:00:10Z' },
    { cardId: 'a', status: 'cancelled', startedAt, endedAt: '2026-10-20T10:00:20Z' },
  ];
  const snapshot = structuredClone(runs);
  assert.deepEqual(agentTime(runs, Date.parse('2026-10-20T10:01:30Z')), [
    { cardId: 'a', seconds: 110, runs: 3, running: 1 },
    { cardId: 'b', seconds: 10, runs: 1, running: 0 },
  ]);
  assert.deepEqual(runs, snapshot);
  assert.deepEqual(agentTime([], Date.now()), []);
});

test('Time view shows automatic totals per task without adding human entries', () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const block = source.slice(source.indexOf("  if (currentView === 'time') {"), source.indexOf('  const openJournals ='));
  const cards = [{ id: 'a', title: '<Tâche>' }, { id: 'b', title: 'Autre' }];
  const context = vm.createContext({
    currentView: 'time', cards, ids: new Set(['a', 'b']), times: [],
    runs: cards.map(card => ({ cardId: card.id, status: 'completed', startedAt: '2026-10-20T10:00:00Z', endedAt: '2026-10-20T10:01:00Z' })),
    state: { times: [] }, timeFilters: { from: '', to: '', status: 'all' }, summaryDay: '2026-10-20',
    agentTime, dailyTime: () => [], duration: seconds => `${seconds}s`, reportMarkup: () => '',
    esc: value => String(value).replaceAll('<', '&lt;').replaceAll('>', '&gt;'),
  });
  const html = vm.runInContext(`let html = ''; ${block}; html`, context);
  assert.match(html, /id="agent-total">120s/);
  assert.match(html, /data-agent-time="a">60s/);
  assert.match(html, /data-agent-time="b">60s/);
  assert.match(html, /&lt;Tâche&gt;/);
  assert.match(html, /id="human-total">0s/);
  assert.equal(context.state.times.length, 0);
});

test('stopped executions without an end date do not keep accumulating time', () => {
  const runs = [
    { cardId: 'a', status: 'interrupted', startedAt: '2026-10-20T10:00:00Z', lastEventAt: '2026-10-20T10:00:12Z' },
    { cardId: 'a', status: 'failed', startedAt: '2026-10-20T10:00:00Z' },
    { cardId: 'a', status: 'cancelled', startedAt: '2026-10-20T10:00:00Z', endedAt: '2026-10-20T09:59:00Z' },
  ];
  assert.equal(agentTime(runs, Date.parse('2026-10-21T10:00:00Z'))[0].seconds, 12);
});

test('live refresh keeps filtered Time totals separate from the all-date task total', () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const total = {};
  const period = { dataset: { agentTime: 'a' }, hasAttribute: () => true };
  const card = { dataset: { agentTime: 'a' }, hasAttribute: () => false };
  const clock = { dataset: { clock: 'old' } };
  const context = vm.createContext({
    state: { runs: [
      { cardId: 'a', status: 'completed', startedAt: new Date(2026, 9, 20, 23, 30).toISOString(), endedAt: new Date(2026, 9, 21, 1).toISOString() },
      { id: 'old', cardId: 'b', status: 'interrupted', startedAt: '2026-10-20T10:00:00Z', lastEventAt: '2026-10-20T10:00:12Z', endedAt: '2026-10-21T10:00:00Z' },
    ], times: [] },
    agentTime, timeFilters: { from: '2026-10-20', to: '2026-10-20' }, selectedRun: null,
    scopedCards: () => [{ id: 'a' }], duration: seconds => `${seconds}s`,
    $: selector => selector === '#agent-total' ? total : null,
    document: { querySelectorAll: selector => selector === '[data-agent-time]' ? [period, card] : selector === '[data-clock]' ? [clock] : [] },
  });
  vm.runInContext(source.slice(source.indexOf('function updateClocks('), source.indexOf('async function refresh(')), context);
  context.updateClocks();
  assert.equal(total.textContent, '1800s');
  assert.equal(period.textContent, '1800s');
  assert.equal(card.textContent, '5400s');
  assert.equal(clock.textContent, 'Durée : 12s');
});

test('date filters split overnight runs into local days and include the entire end date', () => {
  const runs = [{ cardId: 'a', status: 'completed', startedAt: new Date(2026, 9, 20, 23, 59, 30, 500).toISOString(), endedAt: new Date(2026, 9, 21, 0, 1, 0, 750).toISOString() }];
  const now = Date.now();
  const first = agentTime(runs, now, { from: '2026-10-20', to: '2026-10-20' });
  const second = agentTime(runs, now, { from: '2026-10-21', to: '2026-10-21' });
  assert.equal(first[0].seconds, 29);
  assert.equal(second[0].seconds, 61);
  assert.equal(first[0].seconds + second[0].seconds, agentTime(runs)[0].seconds);
  assert.deepEqual(agentTime(runs, now, { to: '2026-10-19' }), []);
  assert.deepEqual(agentTime(runs, now, { from: '2026-10-22' }), []);
  assert.deepEqual(agentTime(runs, now, { from: '2026-10-22', to: '2026-10-20' }), []);
  assert.equal(agentTime(runs, now, { from: '2026-10-21', status: 'exported' })[0].seconds, 61);
});

test('interrupted history uses the last signal even if an old restart recorded a later end date', () => {
  const runs = [{ cardId: 'a', status: 'interrupted', startedAt: '2026-10-20T10:00:00Z', lastEventAt: '2026-10-20T10:00:12Z', endedAt: '2026-10-21T10:00:00Z' }];
  assert.deepEqual(agentTime(runs), [{ cardId: 'a', seconds: 12, runs: 1, running: 0, estimated: true }]);
  assert.deepEqual(agentTime([{ ...runs[0], startedAt: 'invalid' }]), []);
});

test('local day filtering follows daylight saving changes rather than fixed 24 hour windows', () => {
  const helper = new URL('../public/ui-helpers.mjs', import.meta.url).href;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import assert from 'node:assert/strict';
    import { agentTime } from ${JSON.stringify(helper)};
    for (const [day, start, end, hours] of [
      ['2026-03-29', '2026-03-29T00:00:00+01:00', '2026-03-30T00:00:00+02:00', 23],
      ['2026-10-25', '2026-10-25T00:00:00+02:00', '2026-10-26T00:00:00+01:00', 25],
    ]) {
      const runs = [{ cardId: 'a', status: 'completed', startedAt: start, endedAt: end }];
      assert.equal(agentTime(runs, Date.now(), { from: day, to: day })[0].seconds, hours * 3600);
    }
  `], { env: { ...process.env, TZ: 'Europe/Paris' }, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
});
