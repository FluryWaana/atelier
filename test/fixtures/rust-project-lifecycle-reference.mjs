import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createProjects } from '../../lib/application/projects.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { text } from '../../lib/domain/validation.mjs';
import { projectSettings } from '../../lib/project-settings.mjs';

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
for (const body of [{}, { name: 'ignored', unknown: 'ignored' }, { projectRules: ' \ud800 ' }, { repo: 'new' }, { repo: 'repo' }, { repo: null }, { repo: false }, { frontRepo: 'front', backRepo: 'back' }, { linkedProjectId: 'project' }, { linkedProjectId: 'missing' }]) add('update', body);
add('update', { projectRules: '\ud800' }, { projects: [{ ...project, '\ud800': '\udc00', frontRepo: null }] });
add('update', { repo: 'new' }, { id: 'missing' });
add('update', { projectRules: false }, { active: ['archived'] });
for (const windows of [true, false]) add('update', { repo: 'REPO' }, { windows });
for (const repo of ['repo', 'new']) {
  add('update', { repo }, { cards: [{ ...cards[0], worktree: { path: 'kept' } }] });
  add('update', { repo }, { running: true });
}
for (const role of ['front', 'back']) {
  const options = { cards: [{ ...cards[0], serviceWorktrees: { [role]: { path: 'kept' } } }], projects: [{ ...project, [role + 'Repo']: 'checked:service' }] };
  add('update', { [role + 'Repo']: 'service' }, options);
  add('update', { [role + 'Repo']: 'other-service' }, options);
}
const pair = [project, { id: 'other-project', name: 'Other', repo: 'checked:other' }];
add('update', { repo: 'other' }, { projects: pair });
add('update', { linkedProjectId: 'other-project' }, { projects: pair });
add('update', { repositoryConnector: 'git' });
add('update', { repositoryConnector: 'git' }, { connectorAvailable: true });
add('update', {}, { projects: [{ ...project, repositoryConnector: 'missing' }] });
add('update', { repo: 'new' }, { failBackup: true });
add('update', { repo: 'new' }, { failCard: 'archived' });
add('update', { repo: 'new' }, { failReview: 'card' });
for (const scenario of cases) {
  scenario.projects ??= [project]; scenario.cards ??= cards;
  let projects = structuredClone(scenario.projects), saved = structuredClone(scenario.cards);
  const events = [];
  const service = createProjects({
    store: {
      projects: () => structuredClone(projects), cards: () => structuredClone(saved), times: () => scenario.times || [],
      putProject: value => {
        events.push('project:' + value.id);
        const index = projects.findIndex(project => project.id === value.id);
        if (index < 0) projects.push(structuredClone(value)); else projects[index] = structuredClone(value);
      },
      putCard: value => {
        events.push('card:' + value.id);
        if (value.id === scenario.failCard) throw new Error('Synthetic card write failure');
        saved = saved.map(card => card.id === value.id ? structuredClone(value) : card);
      },
      deleteProject: id => { projects = projects.filter(project => project.id !== id); },
      transaction: change => { events.push('transaction'); const before = structuredClone([projects, saved]); try { change(); } catch (error) { [projects, saved] = before; throw error; } },
    },
    samePath: (a,b) => sameRepository(a,b,scenario.windows ? 'win32' : 'linux'),
    repository: value => { events.push({ repository: value }); return 'checked:' + text(value, 2000); },
    projectSettings: body => projectSettings(body, scenario.settings || {}),
    repositoryConnectorAvailable: () => Boolean(scenario.connectorAvailable),
    backup: () => { events.push('backup'); if (scenario.failBackup) throw new Error('Synthetic backup failure'); },
    invalidateReviews: (id, reason) => { events.push({ invalidate: id, reason }); if (scenario.failReview === id) throw new Error('Synthetic review failure'); },
    busy: id => (scenario.active || []).includes(id),
    devServers: { state: () => [{ cardId: 'server', projectIds: ['project'] }], running: () => Boolean(scenario.running) },
    newId: () => 'new-project', timestamp: () => { events.push('timestamp'); return 'now'; },
  });
  try {
    const value = scenario.action === 'create' ? service.create(scenario.body) : scenario.action === 'archive' ? service.archiveTasks(scenario.body.repo) : scenario.action === 'update' ? service.update(scenario.id || 'project', scenario.body) : service.remove(scenario.id || 'project', scenario.body);
    scenario.expected = { value, projects, cards: saved };
  } catch (error) { scenario.expected = { error: error.message, projects, cards: saved }; }
  if (scenario.action === 'update') scenario.expected.events = events;
}
const path = new URL('./rust-project-lifecycle-reference.json', import.meta.url);
const encoded = JSON.stringify(cases, null, 2) + '\n';
if (process.argv.includes('--check')) assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), JSON.parse(encoded), 'Le cycle de vie des projets Node a changé.');
else writeFileSync(path, encoded);
console.log(`${cases.length} opérations de projets historiques vérifiées.`);
