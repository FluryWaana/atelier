import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createTasks } from '../../lib/application/tasks.mjs';
import { text } from '../../lib/domain/validation.mjs';
import { checklistText } from '../../lib/domain/checklist.mjs';

const cases = [];
const add = (body, options = {}) => cases.push({ body, ...options });
for (const title of [undefined, null, false, 1, '', ' ', '\ufeff é \ud800 \u00a0', '🔧'.repeat(150), '🔧'.repeat(151)]) add({ title });
for (const status of [undefined, null, '', false, 'todo', 'doing', 'review', 'done', 'standby']) add({ title: 'Task', status });
for (const mode of [null, '', false, 'edit', 'analyze', 'bad', 1]) add({ title: 'Task', mode });
for (const agent of ['', false, 'codex', 'claude', 'missing', true]) add({ title: 'Task', agent });
for (const key of ['description', 'instructions']) {
  for (const value of [null, '', false, '\ufeff\ud800\udc00\ud800 ', 'x'.repeat(20001)]) add({ title: 'Task', [key]: value });
}
for (const model of [null, ' model/name:v1 ', '\ud800', 12, '--bad']) add({ title: 'Task', model });
for (const effort of [null, 'max', 'minimal', '\ud800']) add({ title: 'Task', effort });
for (const checklist of [null, [], [{ text: ' \ud800 ', done: true, extra: 1 }], [{ text: '🔧'.repeat(250), done: false }], [{ text: '🔧'.repeat(251), done: false }], [null], [{ text: 'x', done: 0 }], Array(21).fill({ text: 'x', done: true })]) add({ title: 'Task', checklist });
const settings = { repo: 'default-repo', taskTemplates: [{ id: 'template', description: '\ud800', instructions: ' template ', mode: 'analyze', checklist: [{ text: '\udc00', done: false }] }], unknown: '\ud800' };
for (const override of [{}, { description: null, instructions: null, checklist: null }, { description: '', instructions: '', checklist: [], mode: 'edit' }, { templateId: 'missing' }, { templateId: false }, { templateId: 42 }]) add({ title: 'Task', templateId: 'template', ...override }, { settings });
add({ title: 'Idea', status: 'invalid', mode: 'invalid' }, { brainstorming: true, settings });
add({ title: 'Idea', repo: ' synthetic ', templateId: 'template' }, { brainstorming: true, settings });
add({ title: 'Task', repo: ' synthetic ', ignored: '\ud800', status: 'doing' });
add({ title: 'Task', repo: true, model: null });
add({ title: 'Task', repo: 'synthetic', model: null }, { repositoryError: true });
add({ title: 'Task', description: false, repo: 'synthetic' }, { repositoryError: true });
add({ title: 'Task', agent: 'work', model: 'model', effort: 'custom_1' }, { settings: { agentProviders: [{ id: 'work', type: 'command', name: '\ud800' }] } });
add({ title: 'Task', agent: 'removed', mode: 'bad', status: 'bad' }, { settings: { agentProviders: [] } });
add({ title: 'Task', agent: 'work', mode: 'bad' }, { settings: { agentProviders: [{ id: 'work', type: 'bad' }] } });
add({ title: 'Task', agent: 'work' }, { settings: { agentProviders: [{ id: 'work', type: 'bad' }] } });
add({ title: 'Task', agent: 'work', model: '\ud800', effort: '\ud800' }, { settings: { agentProviders: [{ id: 'work', type: 'bad' }] } });
add({ title: 'Task', model: false, effort: '\ud800' });
add({ title: 'Task', agent: '\ud800' }, { settings: { agentProviders: [{ id: '\ud800', type: 'codex' }] } });
for (const scenario of cases) {
  const events = [];
  let card, clock = 0;
  const tasks = createTasks({
    store: { settings: () => scenario.settings || {}, putCard: value => { card = structuredClone(value); } },
    config: () => ({ repo: '', ...scenario.settings }),
    configFor: ({ repo }) => { events.push(['agentFor', repo ?? null]); return { agent: 'claude' }; },
    repository: value => {
      events.push(['repository', value]);
      const path = text(value, 2000);
      if (scenario.repositoryError) throw new Error('Synthetic repository failure');
      return 'checked:' + path;
    },
    newId: () => { events.push('id'); return 'created'; },
    timestamp: () => { events.push('timestamp'); return `2026-10-24T00:00:00.00${clock++}Z`; },
  });
  try {
    const value = tasks.create(scenario.body, Boolean(scenario.brainstorming));
    assert.deepEqual(value, card);
    scenario.expected = { value, prompt: checklistText(value.checklist), events };
  } catch (error) {
    assert.equal(card, undefined);
    scenario.expected = { error: error.message, events };
  }
}
const path = new URL('./rust-task-creation-reference.json', import.meta.url);
const encoded = JSON.stringify(cases, null, 2) + '\n';
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), JSON.parse(encoded), 'La création de tâches Node a changé.');
else writeFileSync(path, encoded);
console.log(`${cases.length} créations de tâches historiques vérifiées.`);
