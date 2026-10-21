import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

function page() {
  const fields = ['title', 'description', 'instructions', 'repo', 'agent', 'model', 'effort'].map(name => ({ name, value: '', disabled: false, dataset: {} }));
  fields.forEach(field => { fields[field.name] = field; }); fields.agent.value = 'codex';
  fields.repo.options = [{ value: '' }]; fields.repo.add = option => fields.repo.options.push(option);
  const form = { elements: fields, reportValidity: () => true };
  const nodes = { '#brainstorm-form': form, '#brainstorm-create-local': {}, '#brainstorm-session': { dataset: {} }, '#brainstorm-page-status': {}, '#brainstorm-page-stop': {}, '#brainstorm-page-proposals': { dataset: {} } };
  for (const id of ['brainstorm-context', 'brainstorm-editor-kicker', 'brainstorm-editor-title', 'brainstorm-editor-intro', 'brainstorm-chat', 'brainstorm-start', 'brainstorm-reply', 'brainstorm-send', 'brainstorm-reply-form', 'brainstorm-chat-history', 'brainstorm-composer-agent']) nodes['#' + id] = { dataset: {}, style: {}, value: '', scrollHeight: 0, scrollTop: 0, clientHeight: 0, reportValidity: () => true };
  const calls = [], saved = new Map(), opened = [];
  let failure = false;
  const context = vm.createContext({
    state: { cards: [], runs: [], projects: [], settings: {} }, brainstormSession: '', brainstormInitialized: false, brainstormingBusy: false,
    $: selector => nodes[selector], esc: text => text, taskAgentFields() {}, renderBrainstorm() {}, cardPath: id => '/api/cards/' + id,
    localStorage: { setItem: (key, value) => saved.set(key, value), getItem: key => saved.get(key) },
    FormData: class { constructor(f) { return f.elements.filter(field => field.name && !field.disabled).map(field => [field.name, field.value]); } },
    api: async (path, method, body) => {
      calls.push({ path, method, body });
      if (path.endsWith('/brainstorm')) { if (failure) throw new Error('Agent indisponible'); context.state.runs = [{ id: 'run', cardId: 'session', kind: 'brainstorm', status: 'running' }]; return {}; }
      const session = { id: path === '/api/cards' ? 'task' : 'session', ...body, brainstormSession: path === '/api/brainstorms' || undefined };
      context.state.cards = [session]; return session;
    },
    refresh: async () => context.renderBrainstormPage(), openCard: async id => opened.push(id), toast() {}, action: fn => fn(),
  });
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  vm.runInContext(source.slice(source.indexOf('function rememberBrainstorm('), source.indexOf('function setupBrainstorm(')), context);
  context.renderBrainstormPage();
  return { context, fields, calls, saved, opened, nodes, failLaunch: value => { failure = value; } };
}

test('page creates a local task directly without starting an agent or requiring Jira', async () => {
  const p = page(); p.fields.title.value = 'Idée locale'; p.fields.description.value = 'Contexte';
  await p.context.submitBrainstorm(true);
  assert.equal(p.calls.length, 1); assert.equal(p.calls[0].path, '/api/cards');
  assert.equal(p.calls[0].body.title, 'Idée locale'); assert.deepEqual(p.opened, ['task']);
});

test('page preserves edits across updates and resumes the same session after a failed launch', async () => {
  const p = page(); p.fields.title.value = 'Idée à explorer'; p.fields.instructions.value = 'Contrainte';
  p.failLaunch(true);
  await assert.rejects(p.context.submitBrainstorm(false), /indisponible/);
  assert.equal(p.context.brainstormSession, 'session');
  assert.equal(p.fields.title.value, 'Idée à explorer');
  p.failLaunch(false);
  await p.context.submitBrainstorm(false);
  assert.equal(p.calls.filter(c => c.path === '/api/brainstorms').length, 1);
  assert.equal(p.calls[2].method, 'PATCH'); assert.equal(p.calls[2].path, '/api/cards/session');
  assert.equal(p.nodes['#brainstorm-page-stop'].hidden, false);
  assert.equal(p.fields.title.disabled, true);
  assert.equal(JSON.parse(p.saved.get('atelier-brainstorm-page')).fields.title, 'Idée à explorer');
  p.context.renderBrainstormPage(); assert.equal(p.fields.instructions.value, 'Contrainte');
});

test('brainstorm sessions stay out of the ordinary task views', () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const context = vm.createContext({ state: { projects: [], cards: [{ id: 'idea', brainstormSession: true }, { id: 'task' }] }, $: () => ({ value: '' }) });
  vm.runInContext(source.slice(source.indexOf('function scopedCards('), source.indexOf('function duration(')), context);
  assert.equal(context.scopedCards().length, 1); assert.equal(context.scopedCards()[0].id, 'task');
});

test('first turn switches to chat and replies preserve the session and remain drafted on failure', async () => {
  const p = page(); p.fields.title.value = 'Exporter';
  await p.context.submitBrainstorm(false);
  assert.equal(p.nodes['#brainstorm-chat'].hidden, false);
  assert.equal(p.nodes['#brainstorm-context'].open, false);
  p.context.state.runs[0].status = 'completed';
  p.context.state.runs[0].proposals = [{ title: 'Export CSV', questions: ['Quel séparateur ?'] }];
  p.context.renderBrainstormPage();
  assert.match(p.nodes['#brainstorm-chat-history'].innerHTML, /Quel séparateur/);
  const reply = p.nodes['#brainstorm-reply']; reply.value = 'Utiliser le point-virgule.';
  p.failLaunch(true);
  await assert.rejects(p.context.submitBrainstorm(false, true), /indisponible/);
  assert.equal(reply.value, 'Utiliser le point-virgule.');
  assert.equal(JSON.parse(p.saved.get('atelier-brainstorm-page')).reply, reply.value);
  p.failLaunch(false);
  await p.context.submitBrainstorm(false, true);
  assert.equal(p.calls.at(-1).body.reply, 'Utiliser le point-virgule.');
  assert.equal(p.calls.filter(c => c.path === '/api/brainstorms').length, 1);
  assert.equal(reply.value, ''); assert.equal(reply.disabled, true);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  const count = p.calls.length;
  reply.value = '  '; await p.context.submitBrainstorm(false, true);
  assert.equal(p.calls.length, count);
});

test('chat composer sends on Enter, keeps Shift+Enter and composition, and sizes long messages', () => {
  const p = page(), input = p.nodes['#brainstorm-reply'], send = p.nodes['#brainstorm-send'];
  let submissions = 0, prevented = 0;
  p.nodes['#brainstorm-reply-form'].requestSubmit = () => submissions++;
  assert.equal(send.disabled, true);
  input.value = 'Une réponse'; input.scrollHeight = 240; input.oninput();
  assert.equal(send.disabled, false); assert.equal(input.style.height, '180px'); assert.equal(input.style.overflowY, 'auto');
  const key = overrides => ({ key: 'Enter', shiftKey: false, isComposing: false, preventDefault() { prevented++; }, ...overrides });
  input.onkeydown(key({ shiftKey: true })); input.onkeydown(key({ isComposing: true }));
  assert.equal(submissions, 0); assert.equal(prevented, 0);
  input.onkeydown(key()); assert.equal(submissions, 1); assert.equal(prevented, 1);
  input.value = ' '; input.scrollHeight = 30; input.oninput(); input.onkeydown(key());
  assert.equal(submissions, 1); assert.equal(send.disabled, true); assert.equal(input.style.height, '30px');
});
