import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createProjects } from '../../lib/application/projects.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { text } from '../../lib/domain/validation.mjs';

const project = { id: 'project', name: '\ud800', repo: 'checked:repo', unknown: { keep: '\udc00' } };
const cards = [{ id: 'card', title: '\ud800', repo: project.repo, status: 'doing', unknown: '\udc00' }, { id: 'archived', title: 'Archived', repo: project.repo, status: 'done', archived: true }, { id: 'other', title: 'Other', repo: 'elsewhere' }];
const cases = [];
const add = (action, body = {}, options = {}) => cases.push({ action, body, ...options });
for (const name of [undefined, null, '', ' ', ' \ud800 ', '🔧'.repeat(50), '🔧'.repeat(51)]) add('create', { name, repo: 'new' });
add('create', { name: 'New', repo: 'repo' });
add('create', { name: 'New' });
add('create', { name: 'New', repo: false });
for (const windows of [true, false]) add('create', { name: 'New', repo: 'REPO' }, { windows });
for (const repo of [undefined, null, '', ' ', 'checked:repo', ' checked:repo ', 'absent']) add('archive', { repo });
for (const entry of [{ cardId: 'card', runningSince: 'now' }, { cardId: 'card', exportState: 'sending' }, { cardId: 'card', exportState: 'uncertain' }, { cardId: true, runningSince: 'now' }, { cardId: 'other', runningSince: 'now' }]) add('archive', { repo: project.repo }, { times: [entry] });
add('archive', { repo: project.repo }, { active: ['archived'] });
add('archive', { repo: project.repo }, { projects: [] });
add('archive', { repo: project.repo }, { failCard: 'archived' });
for (const archiveTasks of [undefined, null, false, true, 1, 'true']) add('remove', { archiveTasks });
add('remove', {}, { id: 'missing' });
add('remove', { archiveTasks: true }, { active: ['card'], running: true });
add('remove', { archiveTasks: 'invalid' }, { running: true });
add('remove', { archiveTasks: true }, { times: [{ cardId: 'card', runningSince: 'now' }] });
add('remove', { archiveTasks: false }, { times: [{ cardId: 'card', runningSince: 'now' }] });
add('remove', { archiveTasks: true }, { failCard: 'archived' });
for (const scenario of cases) {
  scenario.projects ??= [project]; scenario.cards ??= cards;
  let projects = structuredClone(scenario.projects), saved = structuredClone(scenario.cards);
  const service = createProjects({
    store: {
      projects: () => structuredClone(projects), cards: () => structuredClone(saved), times: () => scenario.times || [],
      putProject: value => { projects.push(structuredClone(value)); },
      putCard: value => {
        if (value.id === scenario.failCard) throw new Error('Synthetic card write failure');
        saved = saved.map(card => card.id === value.id ? structuredClone(value) : card);
      },
      deleteProject: id => { projects = projects.filter(project => project.id !== id); },
      transaction: change => { const before = structuredClone(saved); try { change(); } catch (error) { saved = before; throw error; } },
    },
    samePath: (a,b) => sameRepository(a,b,scenario.windows ? 'win32' : 'linux'),
    repository: value => 'checked:' + text(value, 2000),
    busy: id => (scenario.active || []).includes(id),
    devServers: { state: () => [{ cardId: 'server', projectIds: ['project'] }], running: () => Boolean(scenario.running) },
    newId: () => 'new-project', timestamp: () => 'now',
  });
  try {
    const value = scenario.action === 'create' ? service.create(scenario.body) : scenario.action === 'archive' ? service.archiveTasks(scenario.body.repo) : service.remove(scenario.id || 'project', scenario.body);
    scenario.expected = { value, projects, cards: saved };
  } catch (error) { scenario.expected = { error: error.message, projects, cards: saved }; }
}
const path = new URL('./rust-project-lifecycle-reference.json', import.meta.url);
const encoded = JSON.stringify(cases, null, 2) + '\n';
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), JSON.parse(encoded), 'Le cycle de vie des projets Node a changé.');
else writeFileSync(path, encoded);
console.log(`${cases.length} opérations de projets historiques vérifiées.`);
