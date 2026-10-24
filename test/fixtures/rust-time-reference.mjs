import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { openStore } from '../../lib/store.mjs';
import { createTimeTracking } from '../../lib/application/time-tracking.mjs';

const origin = Date.parse('2026-10-22T09:00:00Z');
const fields = { cardId: 'card', seconds: 60, date: '2026-10-22', startTime: '09:00:00', description: ' Work \ud800 ' };
const draft = { id: 'seed', ...fields, exportState: 'draft', extra: { future: '\udc00', nullable: null } };
const cases = [
  { name: 'creation and validation', steps: [
    ...[undefined, null, 1, 'missing', 'archived'].map(cardId => ({ action: 'create', body: { ...fields, cardId } })),
    ...[null, 1, 'true'].map(start => ({ action: 'create', body: { ...fields, start } })),
    { action: 'create', body: { ...fields, seconds: 0 } },
    { action: 'create', body: fields },
    { action: 'create', body: { ...fields, start: false } },
    { action: 'create', body: { ...fields, start: true } },
    { action: 'create', body: { ...fields, start: true } },
    { action: 'tick', now: origin + 1999 },
    { action: 'tick', now: origin + 2999 },
    { action: 'tick', now: origin - 1000 },
    { action: 'stop', id: 'entry-3', now: origin + 65001 },
    { action: 'remove', id: 'entry-3' },
    { action: 'remove', id: 'entry-3' },
  ] },
  { name: 'restart and uncertain exports', initial: [
    { ...draft, runningSince: '2026-10-22T08:00:00.000Z', exportState: 'sending', exportPayload: { work: 1 }, exportJira: { id: 42 }, error: 'lost' },
    { ...draft, id: 'empty', runningSince: '', exportState: 'exported' },
  ], steps: [
    { action: 'update', id: 'seed', body: fields },
    { action: 'reset-export', id: 'seed', body: { confirmed: 'true' } },
    { action: 'reset-export', id: 'seed', body: { confirmed: true }, busy: true },
    { action: 'reset-export', id: 'seed', body: { confirmed: true }, busy: false },
    { action: 'update', id: 'seed', body: { ...fields, seconds: '0x78' } },
    { action: 'reset-export', id: 'seed', body: { confirmed: true } },
    { action: 'remove', id: 'empty' },
    { action: 'stop', id: 'empty', busy: true },
    { action: 'unknown', id: 'seed', busy: false },
    { action: 'update', id: 'missing', body: fields },
  ] },
  { name: 'cap, fractions, multiple timers and close', steps: [
    { action: 'put', body: { ...draft, seconds: 86399, runningSince: '2026-10-22T09:00:00.100Z' } },
    { action: 'put', body: { ...draft, id: 'other', seconds: 0, runningSince: '2026-10-22T09:00:00.500Z' } },
    { action: 'update', id: 'other', body: fields },
    { action: 'remove', id: 'other' },
    { action: 'tick', now: origin + 1099 },
    { action: 'tick', now: origin + 1100 },
    { action: 'stop', id: 'seed', now: origin + 2900 },
    { action: 'close', now: origin + 3200 },
    { action: 'tick', now: origin + 999999 },
  ] },
  { name: 'historical card identifier bindings', steps: [
    ...['1', 1, 1.5, true, false, {}, [], ['card']].map(cardId => ({ action: 'create', body: { ...fields, cardId } })),
  ] },
];

for (const scenario of cases) {
  const store = openStore(':memory:');
  let now = origin, sequence = 0, busy = false;
  store.putCard({ id: 'card' }); store.putCard({ id: 'archived', archived: true });
  store.putCard({ id: '1' }); store.putCard({ id: '1.5' });
  for (const entry of scenario.initial || []) store.putTime(entry);
  const service = createTimeTracking({ store, now: () => now, newId: () => `entry-${++sequence}`, changed() {}, exporting: () => busy });
  scenario.initialized = store.times();
  for (const step of scenario.steps) {
    if (step.now !== undefined) now = step.now;
    if (step.busy !== undefined) busy = step.busy;
    try {
      let value;
      if (step.action === 'put') store.putTime(step.body);
      else if (['tick', 'close'].includes(step.action)) service[step.action]();
      else value = await service.execute(step.action, step.id, step.body || {});
      step.expected = { value: value ?? null };
    } catch (error) { step.expected = { error: error.message }; }
    step.times = store.times();
  }
  store.db.close();
}
const path = new URL('./rust-time-reference.json', import.meta.url);
const encoded = JSON.stringify(cases, null, 2) + '\n';
if (process.argv.includes('--check')) assert.equal(readFileSync(path, 'utf8'), encoded);
else writeFileSync(path, encoded);
console.log(`${cases.reduce((sum, scenario) => sum + scenario.steps.length, 0)} transitions de temps historiques vérifiées.`);
