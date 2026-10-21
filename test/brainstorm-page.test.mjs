import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { sameRepository } from '../lib/domain/paths.mjs';

function page() {
  const fields = ['title', 'description', 'instructions', 'repo', 'agent', 'model', 'effort'].map(name => ({ name, value: '', disabled: false, dataset: {} }));
  fields.forEach(field => { fields[field.name] = field; }); fields.agent.value = 'codex';
  fields.repo.options = [{ value: '' }]; fields.repo.add = option => fields.repo.options.push(option);
  const form = { elements: fields, reportValidity: () => true, reset() { fields.forEach(field => field.value = ''); } };
  const nodes = { '#brainstorm-form': form, '#brainstorm-create-local': {}, '#brainstorm-session': { dataset: {} }, '#brainstorm-page-status': {}, '#brainstorm-page-stop': {}, '#brainstorm-page-proposals': { dataset: {} } };
  for (const id of ['brainstorm-new', 'brainstorm-generate', 'brainstorm-retry', 'brainstorm-context', 'brainstorm-editor-kicker', 'brainstorm-editor-title', 'brainstorm-editor-intro', 'brainstorm-chat', 'brainstorm-start', 'brainstorm-reply', 'brainstorm-send', 'brainstorm-reply-form', 'brainstorm-chat-history', 'brainstorm-composer-agent']) nodes['#' + id] = { dataset: {}, style: {}, value: '', scrollHeight: 0, scrollTop: 0, clientHeight: 0, reportValidity: () => true, querySelectorAll: () => [], focus() {} };
  nodes['#brainstorm-page-proposals'].replaceChildren = () => {};
  nodes['#brainstorm-composer-hint'] = {}; nodes['#brainstorm-generate-hint'] = {};
  nodes['#brainstorm-results'] = {};
  nodes['#brainstorm-toolbar'] = {};
  const calls = [], saved = new Map(), opened = [];
  let failure = false;
  const context = vm.createContext({ msg, markupText, locale, messageText, localizeModule,
    state: { cards: [], runs: [], projects: [], settings: {} }, brainstormSession: '', brainstormInitialized: false, brainstormingBusy: false,
    $: selector => nodes[selector], esc: text => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;'), taskAgentFields() {}, renderBrainstorm() {}, cardPath: id => '/api/cards/' + id,
    localStorage: { setItem: (key, value) => saved.set(key, value), getItem: key => saved.get(key), removeItem: key => saved.delete(key) },
    FormData: class { constructor(f) { return f.elements.filter(field => field.name && !field.disabled).map(field => [field.name, field.value]); } },
    api: async (path, method, body) => {
      calls.push({ path, method, body });
      if (path.endsWith('/brainstorm')) { if (failure) throw new Error('Agent indisponible'); context.state.runs = [{ id: 'run', cardId: 'session', kind: 'brainstorm', status: 'running' }]; return context.state.runs[0]; }
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

test('a single idea starts a discussion with an automatic title and no empty tasks panel', async () => {
  const p = page();
  assert.equal(p.nodes['#brainstorm-results'].hidden, true);
  assert.equal(p.nodes['#brainstorm-toolbar'].hidden, true);
  await p.context.submitBrainstorm(false); assert.equal(p.calls.length, 0);
  p.fields.description.value = 'Simplifier l’inscription des clients.\nSupprimer les étapes inutiles.';
  await p.context.submitBrainstorm(false);
  assert.equal(p.calls[0].body.title, 'Simplifier l’inscription des clients.');
  assert.match(p.calls[0].body.description, /étapes inutiles/);
  assert.equal(p.nodes['#brainstorm-editor-title'].textContent, 'Simplifier l’inscription des clients.');
  assert.equal(p.nodes['#brainstorm-results'].hidden, true);
  Object.assign(p.context.state.runs[0], { status: 'completed', proposals: [{ title: 'Inscription', questions: [] }] });
  p.context.renderBrainstormPage();
  assert.equal(p.nodes['#brainstorm-results'].hidden, false);
  assert.equal(p.nodes['#brainstorm-page-status'].hidden, true);
});

test('preparing tasks includes an unsent reply and keeps it on failure', async () => {
  const p = page(); p.fields.description.value = 'Une idée'; await p.context.submitBrainstorm(false);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  p.nodes['#brainstorm-reply'].value = 'Seulement trois étapes.'; p.nodes['#brainstorm-reply'].oninput();
  assert.equal(p.nodes['#brainstorm-generate'].disabled, false);
  p.failLaunch(true);
  await assert.rejects(p.nodes['#brainstorm-generate'].onclick(), /indisponible/);
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Seulement trois étapes.');
  p.failLaunch(false); await p.nodes['#brainstorm-generate'].onclick();
  assert.match(p.calls.at(-1).body.reply, /^Seulement trois étapes\./);
  assert.match(p.calls.at(-1).body.reply, /découpage/);
  assert.equal(p.nodes['#brainstorm-reply'].value, '');
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
  const context = vm.createContext({ msg, markupText, locale, messageText, localizeModule, state: { projects: [], cards: [{ id: 'idea', brainstormSession: true }, { id: 'task' }] }, $: () => ({ value: '' }) });
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
  assert.equal(reply.value, ''); assert.equal(reply.disabled, false); assert.equal(p.nodes['#brainstorm-send'].disabled, true);
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

test('conversation shows escaped answers and clickable questions without requiring proposals', async () => {
  const p = page(); p.fields.title.value = 'Export'; await p.context.submitBrainstorm(false);
  const question = { dataset: { brainstormQuestion: '0' } };
  p.nodes['#brainstorm-chat-history'].querySelectorAll = selector => selector === '[data-brainstorm-question]' ? [question] : [];
  Object.assign(p.context.state.runs[0], { status: 'completed', brainstorm: { message: '<script>Texte utilisateur</script>', questions: ['Pour qui ?'] }, proposals: [] });
  p.context.renderBrainstormPage();
  assert.match(p.nodes['#brainstorm-chat-history'].innerHTML, /&lt;script&gt;/);
  assert.doesNotMatch(p.nodes['#brainstorm-chat-history'].innerHTML, /<script>/);
  assert.equal(p.nodes['#brainstorm-generate'].disabled, false);
  question.onclick();
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Pour qui ?\n');
  assert.equal(p.nodes['#brainstorm-generate'].disabled, false);
  assert.equal(p.nodes['#brainstorm-generate'].textContent, 'Envoyer et préparer les tâches');
  assert.equal(p.nodes['#brainstorm-send'].disabled, false);
  assert.deepEqual(Array.from(p.context.brainstormQuestions({ brainstorm: { questions: ['Pour qui ?'] }, proposals: [{ questions: ['Pour qui ?', 'Quel format ?'] }] })), ['Pour qui ?', 'Quel format ?']);
});

test('drafts survive switching sessions and starting a session clears the initial draft', async () => {
  const p = page(); p.fields.title.value = 'Exporter'; await p.context.submitBrainstorm(false);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  p.nodes['#brainstorm-reply'].value = 'Une réponse à conserver'; p.nodes['#brainstorm-reply'].oninput();
  p.context.loadBrainstormSession('');
  assert.equal(p.fields.title.value, ''); assert.equal(p.nodes['#brainstorm-reply'].value, '');
  p.fields.title.value = 'Autre idée';
  p.context.loadBrainstormSession('session');
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Une réponse à conserver');
  assert.equal(p.fields.title.value, 'Exporter');
  p.context.loadBrainstormSession('');
  assert.equal(p.fields.title.value, 'Autre idée');
});

test('running agents allow drafting but not sending and explicit breakdown does not consume a draft', async () => {
  const p = page(); p.fields.title.value = 'Export'; await p.context.submitBrainstorm(false);
  const input = p.nodes['#brainstorm-reply']; input.value = 'À garder'; input.oninput();
  assert.equal(input.disabled, false); assert.equal(p.nodes['#brainstorm-send'].disabled, true);
  const count = p.calls.length; await p.context.submitBrainstorm(false, true); assert.equal(p.calls.length, count);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  await p.context.submitBrainstorm(false, true, 'Propose le découpage.');
  assert.equal(p.calls.at(-1).body.reply, 'Propose le découpage.'); assert.equal(input.value, 'À garder');
});

test('task panel preserves and labels the previous breakdown during a later conversation', () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const host = { dataset: {}, querySelectorAll: () => [] };
  const context = vm.createContext({ msg, markupText, messageText, sameRepository, esc: text => text,
    state: { cards: [], runs: [{ id: 'new', cardId: 'session', kind: 'brainstorm', status: 'completed', proposals: [], brainstorm: { message: 'Discutons.' } }, { id: 'old', cardId: 'session', kind: 'brainstorm', status: 'completed', proposals: [{ title: 'Export CSV', description: 'Description', checklist: [], questions: [] }] }] },
  });
  vm.runInContext(source.slice(source.indexOf('function renderBrainstorm('), source.indexOf('function renderTicketPreparation(')), context);
  context.renderBrainstorm('session', host);
  assert.match(host.innerHTML, /Export CSV/); assert.match(host.innerHTML, /Dernier découpage proposé/);
  assert.match(host.innerHTML, /data-brainstorm-proposal="0" open/);
  context.state.cards.push({ id: 'draft', brainstormSource: { runId: 'old', index: 0 } });
  host.querySelectorAll = selector => selector.includes('[open]') ? [{ dataset: { brainstormProposal: '0' } }] : [];
  context.renderBrainstorm('session', host);
  assert.match(host.innerHTML, /data-brainstorm-proposal="0" open/); assert.match(host.innerHTML, /Tâche créée/);
  context.state.runs[0].status = 'running'; host.querySelectorAll = () => [];
  context.renderBrainstorm('session', host);
  assert.doesNotMatch(host.innerHTML, /data-brainstorm-proposal="0" open/);
  context.state.cards = [{ id: 'session', repo: 'C:/other' }]; context.state.settings = { platform: 'win32' };
  context.state.runs[0].status = 'completed'; context.state.runs[1].brainstormRepo = 'C:/original';
  context.renderBrainstorm('session', host);
  assert.match(host.innerHTML, /Le projet a changé/); assert.match(host.innerHTML, /data-brainstorm-index="0" disabled/);
  context.state.runs = [{ id: 'generating', cardId: 'session', kind: 'brainstorm', status: 'running' }];
  context.renderBrainstorm('session', host);
  Object.assign(context.state.runs[0], { status: 'completed', proposals: [{ title: 'Une nouvelle tâche', description: '', checklist: [], questions: [] }] });
  context.renderBrainstorm('session', host);
  assert.match(host.innerHTML, /data-brainstorm-proposal="0" open/);
});

test('invalid agent output is available as escaped text and offers retry', async () => {
  const p = page(); p.fields.title.value = 'Export'; await p.context.submitBrainstorm(false);
  Object.assign(p.context.state.runs[0], { status: 'completed', attention: 'Format invalide', summary: '<img src=x onerror=alert(1)>' });
  p.context.renderBrainstormPage();
  const html = p.nodes['#brainstorm-chat-history'].innerHTML;
  assert.match(html, /Voir la réponse brute/); assert.match(html, /&lt;img/); assert.doesNotMatch(html, /<img/);
  assert.equal(p.nodes['#brainstorm-retry'].hidden, false);
});

test('an accepted reply stays sent and the composer recovers even when refresh fails', async () => {
  const p = page(); p.fields.title.value = 'Export'; await p.context.submitBrainstorm(false);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  p.nodes['#brainstorm-reply'].value = 'Réponse envoyée';
  p.context.refresh = async () => { throw new Error('Réseau indisponible'); };
  await assert.rejects(p.context.submitBrainstorm(false, true), /Réseau/);
  assert.equal(JSON.parse(p.saved.get('atelier-brainstorm-page')).reply, '');
  assert.equal(p.nodes['#brainstorm-reply'].disabled, false);
  assert.equal(p.nodes['#brainstorm-send'].disabled, true);
});

test('a created session survives launch and refresh failures without duplicating it', async () => {
  const p = page(); p.fields.title.value = 'À conserver'; p.failLaunch(true);
  p.context.refresh = async () => { throw new Error('Réseau indisponible'); };
  await assert.rejects(p.context.submitBrainstorm(false));
  assert.equal(JSON.parse(p.saved.get('atelier-brainstorm-page')).id, 'session');
  assert.equal(p.fields.title.disabled, false);
});

test('the visible conversation does not show a distracting Agents notification', async () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  for (const cardId of ['session', 'other']) {
    const messages = [], run = { id: 'run', cardId, kind: 'brainstorm', status: 'running' };
    const context = vm.createContext({ msg, messageText, localizeModule,
      state: { cards: [], runs: [run], settings: { platform: 'win32' } }, currentView: 'brainstorm', brainstormSession: 'session',
      api: async () => ({ cards: [], runs: [{ ...run, status: 'completed' }], settings: { platform: 'win32' } }),
      $: () => ({}), resetJiraButton: {}, connectorSupports: () => false, connectorName: () => '',
      renderTemplates() {}, renderProjects() {}, board() {}, updateDevServers() {}, updateAutomation() {},
      document: { querySelector: () => null }, toast: message => messages.push(message),
    });
    vm.runInContext(source.slice(source.indexOf('async function refresh('), source.indexOf('function connectorName(')), context);
    await context.refresh(); assert.equal(messages.length, cardId === 'session' ? 0 : 1);
  }
});
