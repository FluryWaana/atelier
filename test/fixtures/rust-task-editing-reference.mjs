import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createTasks } from '../../lib/application/tasks.mjs';
import { text } from '../../lib/domain/validation.mjs';

const seed = { id: 'card', title: 'Title \ud800', description: 'Description', instructions: '', repo: 'old', agent: 'codex', model: 'model', effort: 'high', mode: 'edit', checklist: [{ text: '\udc00', done: false }], status: 'todo', createdAt: 'created', updatedAt: 'old', unknown: { keep: '\ud800' } };
const cases = [];
const add = (body, options = {}) => cases.push({ body, ...options });
add({});
for (const title of ['', ' ', null, 42, 'New \ud800', 'Title \ud800', '🔧'.repeat(151)]) add({ title });
for (const status of ['todo', 'doing', 'review', 'done', 'standby', 'invalid', null]) add({ status });
add({ status: 'doing' }, { card: { ...seed, status: 'doing' } });
add({ title: 'Changed', status: 'doing' });
add({ title: 'Changed', status: 'doing' }, { launchError: true });
for (const body of [{ title: null, description: false }, { title: 'Ignore', description: 'Ignore', instructions: 'Keep' }]) add(body, { card: { ...seed, ticket: { key: 'TEST-1' } } });
for (const body of [{ checklist: [{ text: '\udc00', done: true }] }, { checklist: [{ text: 'Changed', done: false }] }, { checklist: null }, { instructions: ' Instruction \ud800 ' }, { description: null }, { mode: 'bad' }, { mode: 'analyze' }]) add(body);
for (const body of [{ agent: 'claude' }, { agent: 'codex' }, { agent: 'missing' }, { agent: null }, { model: null, effort: null }, { model: false, effort: '\ud800' }, { agent: 'claude', model: 'opus', effort: 'max' }]) add(body);
add({}, { card: { ...seed, model: null } });
const { model, effort, ...legacy } = seed;
add({}, { card: legacy });
for (const body of [{ repo: 'new' }, { repo: '' }, { repo: null }, { branch: 'next' }, { baseBranch: '' }, { baseBranch: null }, { baseBranch: 'main' }]) add(body, { card: { ...seed, worktree: { path: 'protected' } } });
for (const body of [{ repo: 'old' }, { repo: '' }, { repo: null }, { repo: ' new ' }, { repo: true }, { branch: ' branch ', baseBranch: ' base ' }, { branch: null }]) add(body);
add({ repo: 'new', agent: 'missing' }, { repositoryError: true });
add({ title: null }, { busy: true });
add({ title: null }, { busy: true, card: { ...seed, archived: true } });
add({}, { missing: true });
for (const blockedBy of [[], ['other'], ['missing'], ['card'], ['other', 'other'], null, 'other']) add({ blockedBy });
add({ blockedBy: ['other'] }, { others: [{ id: 'other', blockedBy: ['card'] }] });
add({ blockedBy: ['other'] }, { others: [{ id: 'other', status: 'todo', archived: true }] });

for (const scenario of cases) {
  let saved = structuredClone(scenario.card || seed);
  scenario.card = structuredClone(saved);
  const events = [];
  const tasks = createTasks({
    store: {
      card: () => scenario.missing ? null : structuredClone(saved),
      cards: () => [structuredClone(saved), ...structuredClone(scenario.others || [{ id: 'other', status: 'done' }])],
      putCard: card => { events.push(['put', structuredClone(card)]); saved = structuredClone(card); },
    },
    config: () => ({}), busy: () => Boolean(scenario.busy),
    repository: value => { events.push(['repository', value]); const path = text(value, 2000); if (scenario.repositoryError) throw new Error('Synthetic repository failure'); return 'checked:' + path; },
    timestamp: () => { events.push('timestamp'); return 'now'; },
    launchTask: (card, options) => {
      events.push(['launch', structuredClone(card), structuredClone(options)]);
      if (scenario.launchError) throw new Error('Synthetic launch failure');
      saved = { ...structuredClone(card), status: 'doing' };
    },
    invalidateReviews: () => events.push('invalidate'),
    syncCardStatus: () => { events.push('sync'); return structuredClone(saved); },
  });
  try { scenario.expected = { value: await tasks.update('card', scenario.body), saved, events }; }
  catch (error) { scenario.expected = { error: error.message, saved, events }; }
}
const path = new URL('./rust-task-editing-reference.json', import.meta.url);
const encoded = JSON.stringify(cases, null, 2) + '\n';
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), JSON.parse(encoded), 'Les modifications de tâches Node ont changé.');
else writeFileSync(path, encoded);
console.log(`${cases.length} modifications de tâches historiques vérifiées.`);
