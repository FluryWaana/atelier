import test from 'node:test';
import assert from 'node:assert/strict';
import { createTasks } from '../lib/application/tasks.mjs';
import { createProjects } from '../lib/application/projects.mjs';
import { blockers, dependencies } from '../lib/domain/tasks.mjs';

function fixture() {
  let data = { cards: [], projects: [], times: [], settings: {} }, sequence = 0;
  const events = [], queued = [], active = new Set();
  const store = {
    cards: () => structuredClone(data.cards), card: id => structuredClone(data.cards.find(card => card.id === id) || null),
    projects: () => structuredClone(data.projects), times: () => structuredClone(data.times),
    settings: () => structuredClone(data.settings), saveSettings: settings => { data.settings = structuredClone(settings); },
    putCard(card) { data.cards = [...data.cards.filter(item => item.id !== card.id), structuredClone(card)]; },
    putProject(project) { data.projects = [...data.projects.filter(item => item.id !== project.id), structuredClone(project)]; },
    deleteCard(id) { data.cards = data.cards.filter(card => card.id !== id); },
    deleteProject(id) { data.projects = data.projects.filter(project => project.id !== id); },
    transaction(change) { const before = structuredClone(data); try { change(); } catch (error) { data = before; throw error; } },
  };
  const shared = { store, repository: value => value, newId: () => 'id-' + ++sequence, timestamp: () => '2026-10-21T10:00:00.000Z', busy: id => active.has(id), invalidateReviews: id => events.push(['review', id]) };
  const taskOptions = { ...shared, config: () => ({ repo: 'C:/project' }), configFor: () => ({ agent: 'claude' }), queuedIds: () => queued,
    launchTask(card, options) { events.push(['launch', card.status, options]); store.putCard({ ...card, status: 'doing' }); },
    async syncCardStatus(id) { events.push(['sync', id]); return store.card(id); },
  };
  const projectOptions = { ...shared, samePath: (a, b) => a.toLowerCase() === b.toLowerCase(), projectSettings: value => value, detectLaunch: () => ({ candidates: [], warnings: [] }), devServers: { state: () => [], running: () => false }, backup: () => events.push(['backup']) };
  return { store, events, active, queued, taskOptions, projectOptions, tasks: createTasks(taskOptions), projects: createProjects(projectOptions), setTimes: times => { data.times = times; } };
}

test('task use cases inherit defaults and duplicate only editable local content without starting agents', () => {
  const { tasks, store, events } = fixture();
  const card = tasks.create({ title: '  Besoin  ', status: 'doing', checklist: [{ text: 'Critère', done: true }] });
  assert.equal(card.title, 'Besoin'); assert.equal(card.agent, 'claude'); assert.equal(card.repo, 'C:/project');
  store.putCard({ ...card, ticket: { key: 'PROJ-1' }, worktree: { path: 'work' }, pullRequest: { number: 1 }, blockedBy: ['other'] });
  const copy = tasks.duplicate(card.id);
  assert.equal(copy.status, 'todo'); assert.deepEqual(copy.checklist, [{ text: 'Critère', done: false }]);
  for (const key of ['jira', 'worktree', 'pullRequest', 'blockedBy']) assert.equal(copy[key], undefined);
  assert.deepEqual(events, []);
});

test('task edits preserve Jira authority and route a move into doing through execution then synchronization', async () => {
  const { tasks, store, events } = fixture();
  const card = tasks.create({ title: 'Titre Jira' }); store.putCard({ ...card, ticket: { key: 'PROJ-1' } });
  const result = await tasks.update(card.id, { title: 'À ignorer', status: 'doing', instructions: 'Instruction', mode: 'analyze' });
  assert.equal(result.title, 'Titre Jira'); assert.equal(result.status, 'doing');
  assert.deepEqual(events, [['launch', 'todo', { agent: 'claude', mode: 'analyze', instructions: 'Instruction' }], ['review', card.id], ['sync', card.id]]);
});

test('dependency validation rejects missing tasks, duplicates and cycles, including archived prerequisites', () => {
  const cards = [{ id: 'a', status: 'todo' }, { id: 'b', status: 'todo', archived: true, blockedBy: ['a'] }];
  for (const value of [['a'], ['missing'], ['b', 'b'], ['b'], 'b']) assert.throws(() => dependencies('a', value, cards));
  assert.deepEqual(dependencies('b', ['a'], cards), ['a']);
  assert.equal(blockers({ blockedBy: ['b'] }, cards)[0].id, 'b');
  assert.equal(blockers({ blockedBy: ['missing'] }, cards)[0].status, 'missing');
});

test('moving into doing invalidates reviews when requirements change, including analysis runs', async () => {
  const { tasks, events } = fixture();
  const card = tasks.create({ title: 'Besoin initial' });
  await tasks.update(card.id, { status: 'doing', mode: 'analyze', checklist: [{ text: 'Nouveau critère', done: false }] });
  assert.deepEqual(events.map(event => event[0]), ['launch', 'review', 'sync']);
  events.length = 0;
  await tasks.update(card.id, { status: 'review' });
  events.length = 0;
  await tasks.update(card.id, { status: 'doing' });
  assert.deepEqual(events.map(event => event[0]), ['launch', 'sync']);
});

test('task deletion retains cards with worktrees, dependencies, queued work or time history', () => {
  const { tasks, store, queued, setTimes } = fixture();
  const card = tasks.create({ title: 'À conserver' });
  queued.push(card.id); assert.throws(() => tasks.remove(card.id), /file/); queued.length = 0;
  store.putCard({ id: 'other', blockedBy: [card.id] }); assert.throws(() => tasks.remove(card.id), /Bloquée/); store.deleteCard('other');
  setTimes([{ cardId: card.id, seconds: 60 }]); assert.throws(() => tasks.remove(card.id), /historique/); setTimes([]);
  store.putCard({ ...card, serviceWorktrees: { front: { path: 'work' } } }); assert.throws(() => tasks.remove(card.id), /worktrees/);
  assert.ok(store.card(card.id));
  store.putCard(card); assert.deepEqual(tasks.remove(card.id), { ok: true }); assert.equal(store.card(card.id), null);
});

test('archive and deletion preserve historical guards, UTF-16 and strict card links', () => {
  const { tasks, store, active, setTimes } = fixture();
  const card = tasks.create({ title: '\ud800' });
  store.putCard({ ...card, unknown: { future: '\udc00' } });
  active.add(card.id);
  assert.throws(() => tasks.archive(card.id, { archived: 'true' }), { message: 'État d’archive invalide.', code: 'invalid' });
  assert.throws(() => tasks.archive(card.id, { archived: true }), { code: 'conflict' });
  active.clear();
  for (const entry of [{ cardId: card.id, runningSince: 'now' }, { cardId: card.id, exportState: 'sending' }]) {
    setTimes([entry]); assert.throws(() => tasks.archive(card.id, { archived: true }), { code: 'conflict' });
  }
  setTimes([{ cardId: card.id, exportState: 'uncertain' }]);
  const archived = tasks.archive(card.id, { archived: true });
  assert.equal(archived.title, '\ud800'); assert.equal(archived.unknown.future, '\udc00');
  assert.equal(tasks.archive(card.id, { archived: false }).archived, false);
  assert.throws(() => tasks.remove(card.id), { code: 'conflict' });
  setTimes([{ cardId: true }]);
  store.putCard({ id: 'other', blockedBy: 'prefix-' + card.id + '-suffix' });
  assert.throws(() => tasks.remove(card.id), { code: 'conflict' });
  store.putCard({ id: 'other', blockedBy: {} });
  assert.throws(() => tasks.remove(card.id), { message: 'c.blockedBy?.includes is not a function' });
  store.deleteCard('other');
  assert.deepEqual(tasks.remove(card.id), { ok: true });
});

test('task templates preserve settings and UTF-16, reset criteria and enforce fifty entries', () => {
  const { tasks, store } = fixture();
  const card = tasks.create({ title: 'Source', description: '\ud800', checklist: [{ text: '\udc00', done: true }] });
  store.saveSettings({ future: { '\ud800': '\udc00' }, taskQueue: { paused: true, ids: [card.id] } });
  const first = tasks.createTemplate({ cardId: card.id, name: ' \ud800 ' });
  assert.equal(first.name, '\ud800'); assert.equal(first.description, '\ud800');
  assert.deepEqual(first.checklist, [{ text: '\udc00', done: false }]);
  for (let index = 1; index < 50; index++) tasks.createTemplate({ cardId: card.id, name: 'Template' });
  assert.throws(() => tasks.createTemplate({ cardId: card.id, name: 'Overflow' }), /50 maximum/);
  tasks.removeTemplate('missing'); assert.equal(store.settings().taskTemplates.length, 50);
  tasks.removeTemplate(first.id);
  tasks.createTemplate({ cardId: card.id, name: '🔧'.repeat(50) });
  assert.throws(() => tasks.createTemplate({ cardId: card.id, name: '🔧'.repeat(51) }), /Texte invalide/);
  assert.throws(() => tasks.createTemplate({ cardId: card.id, name: ' ' }), /Nom de modèle/);
  assert.deepEqual(store.settings().future, { '\ud800': '\udc00' });
  assert.deepEqual(store.settings().taskQueue, { paused: true, ids: [card.id] });
  assert.equal(store.card(card.id).checklist[0].done, true);
});

test('project relocation backs up before writes and rolls back cards when an operation fails', () => {
  const { projects, store, events, projectOptions } = fixture();
  const project = projects.create({ name: 'Projet', repo: 'C:/old' });
  store.putCard({ id: 'card', repo: 'C:/old', status: 'done' });
  const brokenBackup = createProjects({ ...projectOptions, backup() { throw new Error('backup failed'); } });
  assert.throws(() => brokenBackup.update(project.id, { repo: 'C:/new' }), /backup failed/);
  assert.equal(store.projects()[0].repo, 'C:/old');
  const brokenReview = createProjects({ ...projectOptions, invalidateReviews() { throw new Error('review failed'); } });
  assert.throws(() => brokenReview.update(project.id, { repo: 'C:/new' }), /review failed/);
  assert.equal(store.projects()[0].repo, 'C:/old'); assert.equal(store.card('card').repo, 'C:/old');
  events.length = 0; projects.update(project.id, { repo: 'C:/new' });
  assert.deepEqual(events, [['backup'], ['review', 'card']]);
  assert.equal(store.card('card').status, 'done'); assert.equal(store.card('card').repo, 'C:/new');
});

test('removing a project archives its cards, but an active timer prevents any change', () => {
  const { projects, store, setTimes } = fixture();
  const project = projects.create({ name: 'Projet', repo: 'C:/project' });
  store.putCard({ id: 'card', repo: project.repo, status: 'doing' });
  setTimes([{ cardId: 'card', runningSince: '2026-10-21T10:00:00Z' }]);
  assert.throws(() => projects.remove(project.id, { archiveTasks: true }), /chronomètres/);
  assert.equal(store.projects().length, 1); assert.equal(store.card('card').archived, undefined);
  setTimes([]); assert.deepEqual(projects.remove(project.id, { archiveTasks: true }), { ok: true, archived: 1 });
  assert.equal(store.card('card').status, 'doing'); assert.equal(store.card('card').archived, true);
});
