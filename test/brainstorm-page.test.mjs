import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createBrainstormView, createBrainstormProposals, brainstormQuestions } from '../public/views/brainstorm.mjs';
import { scopedCards } from '../public/ui-helpers.mjs';
import { notifyCompletions } from '../public/ui-actions.mjs';
import { bindView } from './helpers/view-context.mjs';
import { sameRepository } from '../lib/domain/paths.mjs';

function page({ state, storage = new Map() } = {}) {
  const fields = ['title', 'description', 'instructions', 'repo', 'agent', 'model', 'effort'].map(name => ({ name, value: '', disabled: false, dataset: {}, focus() { this.focused = true; } }));
  fields.forEach(field => { fields[field.name] = field; }); fields.agent.value = 'codex';
  fields.repo.options = [{ value: '' }]; fields.repo.add = option => fields.repo.options.push(option);
  const form = { elements: fields, reportValidity: () => true, reset() { fields.forEach(field => field.value = ''); } };
  const nodes = { '#brainstorm-form': form, '#brainstorm-create-local': {}, '#brainstorm-session': { dataset: {} }, '#brainstorm-page-status': {}, '#brainstorm-page-stop': {}, '#brainstorm-page-proposals': { dataset: {} } };
  for (const id of ['brainstorm-new', 'brainstorm-generate', 'brainstorm-retry', 'brainstorm-context', 'brainstorm-editor-kicker', 'brainstorm-editor-title', 'brainstorm-editor-intro', 'brainstorm-chat', 'brainstorm-start', 'brainstorm-reply', 'brainstorm-send', 'brainstorm-reply-form', 'brainstorm-chat-history', 'brainstorm-composer-agent']) nodes['#' + id] = { dataset: {}, style: {}, value: '', scrollHeight: 0, scrollTop: 0, clientHeight: 0, reportValidity: () => true, querySelectorAll: () => [], focus() {} };
  nodes['#brainstorm-page-proposals'].replaceChildren = () => {};
  nodes['#brainstorm-composer-hint'] = {}; nodes['#brainstorm-generate-hint'] = {};
  nodes['#brainstorm-results'] = { focus() { this.focused = true; }, scrollIntoView() { assert.fail('Task navigation must not scroll the page'); } };
  nodes['#brainstorm-layout'] = { dataset: { panel: 'chat' }, style: { setProperty(name, value) { this[name] = value; } }, getBoundingClientRect: () => ({ left: 100, width: 1000 }) };
  nodes['#brainstorm-splitter'] = {
    dataset: {}, offsetWidth: 24, getBoundingClientRect: () => ({ left: 780 }), focus() {},
    setAttribute(name, value) { this[name] = value; }, setPointerCapture(id) { this.pointer = id; },
    hasPointerCapture(id) { return this.pointer === id; }, releasePointerCapture() { this.pointer = null; },
  };
  nodes['#brainstorm-toggle-tasks'] = {};
  nodes['#brainstorm-toolbar'] = {};
  nodes['#brainstorm-session-summary'] = {}; nodes['#brainstorm-session-context'] = {};
  for (const step of ['idea', 'chat', 'tasks']) nodes['#brainstorm-step-' + step] = { setAttribute(name, value) { this[name] = value; } };
  const calls = [], saved = storage, opened = [];
  let failure = false;
  const context = {
    state: { cards: [], runs: [], projects: [], settings: {} }, brainstormSession: '', brainstormInitialized: false, brainstormingBusy: false,
    $: selector => nodes[selector], esc: text => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;'), taskAgentFields(form, options = {}) { for (const name of ['model', 'effort']) form.elements[name].value = options[name] || ''; }, renderBrainstorm() {}, cardPath: id => '/api/cards/' + id,
    localStorage: { setItem: (key, value) => saved.set(key, value), getItem: key => saved.get(key), removeItem: key => saved.delete(key) },
    FormData: class { constructor(f) { return f.elements.filter(field => field.name && !field.disabled).map(field => [field.name, field.value]); } },
    api: async (path, method, body) => {
      calls.push({ path, method, body });
      if (path.endsWith('/brainstorm')) { if (failure) throw new Error('Agent indisponible'); context.state.runs = [{ id: 'run', cardId: 'session', kind: 'brainstorm', status: 'running' }]; return context.state.runs[0]; }
      const session = { id: path === '/api/cards' ? 'task' : 'session', ...body, brainstormSession: path === '/api/brainstorms' || undefined };
      context.state.cards = [session]; return session;
    },
    refresh: async () => context.renderBrainstormPage(), openCard: async id => opened.push(id), toast() {}, action: fn => fn(),
  };

  if (state) context.state = state;
  bindView(createBrainstormView, context);
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
  assert.equal(p.nodes['#brainstorm-step-idea']['aria-current'], 'step');
  assert.equal(p.nodes['#brainstorm-results'].hidden, true);
  assert.equal(p.nodes['#brainstorm-toolbar'].hidden, true);
  await p.context.submitBrainstorm(false); assert.equal(p.calls.length, 0);
  p.fields.description.value = 'Simplifier l’inscription des clients.\nSupprimer les étapes inutiles.';
  await p.context.submitBrainstorm(false);
  assert.equal(p.nodes['#brainstorm-step-chat']['aria-current'], 'step');
  assert.equal(p.nodes['#brainstorm-step-idea']['aria-current'], 'false');
  assert.equal(p.calls[0].body.title, 'Simplifier l’inscription des clients.');
  assert.match(p.calls[0].body.description, /étapes inutiles/);
  assert.equal(p.nodes['#brainstorm-editor-title'].textContent, 'Simplifier l’inscription des clients.');
  assert.equal(p.nodes['#brainstorm-results'].hidden, true);
  Object.assign(p.context.state.runs[0], { status: 'completed', proposals: [{ title: 'Inscription', questions: [] }] });
  p.context.renderBrainstormPage();
  assert.equal(p.nodes['#brainstorm-results'].hidden, false);
  assert.equal(p.nodes['#brainstorm-step-tasks']['aria-current'], 'step');
  assert.equal(p.nodes['#brainstorm-step-chat']['aria-current'], 'false');
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

test('tasks open without scrolling the page and returning to chat preserves the reply', async () => {
  const p = page(); p.fields.description.value = 'Une idée'; await p.context.submitBrainstorm(false);
  const showTasks = {};
  p.nodes['#brainstorm-chat-history'].querySelectorAll = selector => selector === '[data-brainstorm-show-tasks]' ? [showTasks] : [];
  Object.assign(p.context.state.runs[0], { status: 'completed', proposals: [{ title: 'Une tâche', questions: [] }] });
  p.context.renderBrainstormPage();
  assert.equal(p.nodes['#brainstorm-toggle-tasks'].hidden, false);
  assert.equal(p.nodes['#brainstorm-layout'].dataset.panel, 'chat');
  p.nodes['#brainstorm-reply'].value = 'Précision en cours';
  showTasks.onclick();
  assert.equal(p.nodes['#brainstorm-layout'].dataset.panel, 'tasks');
  assert.equal(p.nodes['#brainstorm-results'].focused, true);
  assert.equal(p.nodes['#brainstorm-toggle-tasks'].textContent, 'Revenir à la discussion');
  p.nodes['#brainstorm-toggle-tasks'].onclick();
  assert.equal(p.nodes['#brainstorm-layout'].dataset.panel, 'chat');
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Précision en cours');
  p.nodes['#brainstorm-toggle-tasks'].onclick();
  p.context.loadBrainstormSession('');
  assert.equal(p.nodes['#brainstorm-layout'].dataset.panel, 'chat');
  assert.equal(p.nodes['#brainstorm-toggle-tasks'].hidden, true);
});

test('splitter resizes both panels with pointer and keyboard without changing the discussion', async () => {
  const p = page(), splitter = p.nodes['#brainstorm-splitter'], style = p.nodes['#brainstorm-layout'].style;
  assert.equal(splitter.hidden, true);
  p.fields.description.value = 'Une idée'; await p.context.submitBrainstorm(false);
  Object.assign(p.context.state.runs[0], { status: 'completed', proposals: [{ title: 'Une tâche', questions: [] }] });
  p.context.renderBrainstormPage();
  assert.equal(splitter.hidden, false);
  p.nodes['#brainstorm-reply'].value = 'Brouillon à garder';
  p.nodes['#brainstorm-reply'].scrollHeight = 240;
  const history = p.nodes['#brainstorm-chat-history'].innerHTML, calls = p.calls.length;
  const pointer = { button: 0, isPrimary: true, pointerId: 1, clientX: 788, preventDefault() {} };
  splitter.onpointerdown({ ...pointer, button: 2 }); assert.equal(splitter.pointer, undefined);
  splitter.onpointerdown(pointer); assert.equal(splitter.pointer, 1);
  splitter.onpointermove({ pointerId: 2, clientX: 596 }); assert.equal(style['--brainstorm-chat-size'], undefined);
  splitter.onpointermove({ pointerId: 1, clientX: 596 });
  assert.equal(style['--brainstorm-chat-size'], '50fr'); assert.equal(style['--brainstorm-task-size'], '50fr');
  assert.equal(p.nodes['#brainstorm-reply'].style.height, '180px');
  assert.equal(splitter['aria-valuenow'], '50'); assert.match(splitter['aria-valuetext'], /Discussion : 50 %/);
  splitter.onpointermove({ pointerId: 1, clientX: -100 }); assert.equal(splitter['aria-valuenow'], '30');
  splitter.onpointermove({ pointerId: 1, clientX: 2000 }); assert.equal(splitter['aria-valuenow'], '75');
  splitter.onpointercancel(pointer); assert.equal(splitter.pointer, null); assert.equal(splitter.dataset.dragging, undefined);
  splitter.onpointermove({ pointerId: 1, clientX: 596 }); assert.equal(splitter['aria-valuenow'], '75');
  for (const [key, expected] of [['ArrowLeft', '70'], ['Home', '30'], ['ArrowRight', '35'], ['End', '75']]) {
    let prevented = false;
    splitter.onkeydown({ key, preventDefault() { prevented = true; } });
    assert.equal(prevented, true); assert.equal(splitter['aria-valuenow'], expected);
  }
  splitter.onkeydown({ key: 'Tab', preventDefault() { assert.fail('Tab must retain keyboard navigation'); } });
  p.context.renderBrainstormPage();
  assert.equal(style['--brainstorm-chat-size'], '75fr');
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Brouillon à garder');
  assert.equal(p.nodes['#brainstorm-chat-history'].innerHTML, history); assert.equal(p.calls.length, calls);
});

test('page preserves edits across updates and resumes the same session after a failed launch', async () => {
  const p = page(); p.fields.title.value = 'Idée à explorer'; p.fields.instructions.value = 'Contrainte';
  p.failLaunch(true);
  await assert.rejects(p.context.submitBrainstorm(false), /indisponible/);
  assert.equal(p.context.session, 'session');
  assert.equal(p.fields.title.value, 'Idée à explorer');
  p.failLaunch(false);
  await p.context.submitBrainstorm(false);
  assert.equal(p.calls.filter(c => c.path === '/api/brainstorms').length, 1);
  assert.equal(p.calls[2].method, 'POST'); assert.equal(p.calls[2].path, '/api/cards/session/brainstorm');
  assert.equal(p.calls.some(call => call.method === 'PATCH'), false);
  assert.equal(p.nodes['#brainstorm-page-stop'].hidden, false);
  assert.equal(p.fields.title.disabled, true);
  assert.equal(JSON.parse(p.saved.get('atelier-brainstorm-page')).fields.title, 'Idée à explorer');
  p.context.renderBrainstormPage(); assert.equal(p.fields.instructions.value, 'Contrainte');
});

test('brainstorm sessions stay out of the ordinary task views', () => {

  const context = {  state: { projects: [], cards: [{ id: 'idea', brainstormSession: true }, { id: 'task' }] }, $: () => ({ value: '' }) };
  context.scopedCards = () => scopedCards(context.state, '');
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
  assert.doesNotMatch(p.nodes['#brainstorm-chat-history'].innerHTML, /<script>/i);
  assert.equal(p.nodes['#brainstorm-generate'].disabled, false);
  question.onclick();
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Pour qui ?\n');
  assert.equal(p.nodes['#brainstorm-generate'].disabled, false);
  assert.equal(p.nodes['#brainstorm-generate'].textContent, 'Envoyer et préparer les tâches');
  assert.equal(p.nodes['#brainstorm-send'].disabled, false);
  assert.deepEqual(Array.from(brainstormQuestions({ brainstorm: { questions: ['Pour qui ?'] }, proposals: [{ questions: ['Pour qui ?', 'Quel format ?'] }] })), ['Pour qui ?', 'Quel format ?']);
});

test('drafts survive switching sessions and starting a session clears the initial draft', async () => {
  const p = page(); p.fields.title.value = 'Exporter'; await p.context.submitBrainstorm(false);
  p.context.state.runs[0].status = 'completed'; p.context.renderBrainstormPage();
  p.nodes['#brainstorm-reply'].value = 'Une réponse à conserver'; p.nodes['#brainstorm-reply'].oninput();
  p.context.loadBrainstormSession('');
  assert.equal(p.fields.title.value, ''); assert.equal(p.nodes['#brainstorm-reply'].value, '');
  assert.equal(p.fields.description.focused, true);
  p.fields.title.value = 'Autre idée';
  p.context.loadBrainstormSession('session');
  assert.equal(p.nodes['#brainstorm-reply'].value, 'Une réponse à conserver');
  assert.equal(p.fields.title.value, 'Exporter');
  p.context.loadBrainstormSession('');
  assert.equal(p.fields.title.value, 'Autre idée');
});

test('conversation separates paragraphs without interpreting user or agent HTML', async () => {
  const p = page(); p.fields.description.value = 'Une idée'; await p.context.submitBrainstorm(false);
  Object.assign(p.context.state.runs[0], {
    status: 'completed', requirements: { description: 'Premier point.\n\nSecond point <img src=x>.' },
    brainstorm: { message: 'Une réponse longue.\r\n\r\nUn autre paragraphe.\nUne précision.\n\n<script>alert(1)</script>', questions: [] },
    proposals: [],
  });
  p.context.renderBrainstormPage();
  const html = p.nodes['#brainstorm-chat-history'].innerHTML;
  assert.match(html, /<p>Premier point\.<\/p><p>Second point &lt;img src=x&gt;\.<\/p>/);
  assert.match(html, /<p>Une réponse longue\.<\/p><p>Un autre paragraphe\.\nUne précision\.<\/p>/);
  assert.match(html, /<p>&lt;script&gt;alert\(1\)&lt;\/script&gt;<\/p>/);
  assert.doesNotMatch(html, /<(img|script)[ >]/);
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

  const host = { dataset: {}, querySelectorAll: () => [] };
  const context = {  sameRepository, esc: text => text,
    state: { cards: [], runs: [{ id: 'new', cardId: 'session', kind: 'brainstorm', status: 'completed', proposals: [], brainstorm: { message: 'Discutons.' } }, { id: 'old', cardId: 'session', kind: 'brainstorm', status: 'completed', proposals: [{ title: 'Export CSV', description: 'Description', checklist: [], questions: [] }] }] },
  };
  bindView(createBrainstormProposals, context);
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
  assert.equal(p.fields.title.disabled, true);
  assert.equal(p.nodes['#brainstorm-form'].hidden, true);
  assert.equal(p.nodes['#brainstorm-retry'].hidden, false);
});

test('session settings stay read-only after completion, cancellation and failure; replies never update them', async () => {
  const p = page(); p.fields.title.value = 'Une idée'; p.fields.model.value = 'my-model';
  p.fields.instructions.value = '<script>Contrainte</script>';
  await p.context.submitBrainstorm(false);
  for (const status of ['running', 'completed', 'cancelled', 'failed']) {
    p.context.state.runs[0].status = status; p.context.renderBrainstormPage();
    assert.ok(p.fields.every(field => field.disabled));
    assert.equal(p.nodes['#brainstorm-form'].hidden, true);
    assert.equal(p.nodes['#brainstorm-session-context'].hidden, false);
    assert.match(p.nodes['#brainstorm-session-context'].innerHTML, /my-model/);
    assert.match(p.nodes['#brainstorm-session-context'].innerHTML, /&lt;script&gt;/);
  }
  p.nodes['#brainstorm-reply'].value = 'Précision';
  p.nodes['#brainstorm-form'].reportValidity = () => assert.fail('Replies do not validate the initial form');
  await p.context.submitBrainstorm(false, true);
  assert.deepEqual(p.calls.at(-1), { path: '/api/cards/session/brainstorm', method: 'POST', body: { reply: 'Précision' } });
  assert.equal(p.calls.some(call => call.method === 'PATCH'), false);
  const count = p.calls.length; await p.context.submitBrainstorm(true); assert.equal(p.calls.length, count);
  p.context.loadBrainstormSession('');
  assert.ok(p.fields.every(field => !field.disabled));
  assert.equal(p.nodes['#brainstorm-form'].hidden, false);
  assert.equal(p.nodes['#brainstorm-chat'].hidden, true);
});

test('reopening and reloading a session ignore stale setting drafts but preserve unsent replies', () => {
  const session = { id: 'saved', brainstormSession: true, title: 'Idée enregistrée', description: 'Contexte initial', repo: '', agent: 'codex', model: 'original', effort: 'high' };
  const draft = JSON.stringify({ id: session.id, fields: { title: 'Ancien brouillon', repo: 'C:/wrong', model: 'wrong', agent: 'claude' }, reply: 'À envoyer' });
  const p = page({ state: { brainstorms: [session], cards: [], runs: [], projects: [], settings: {} }, storage: new Map([['atelier-brainstorm-page', draft], ['atelier-brainstorm-draft:saved', draft]]) });
  for (const reopen of [false, true]) {
    if (reopen) { p.context.loadBrainstormSession(''); p.saved.set('atelier-brainstorm-draft:saved', draft); p.context.loadBrainstormSession('saved'); }
    assert.equal(p.fields.title.value, session.title); assert.equal(p.fields.repo.value, '');
    assert.equal(p.fields.model.value, 'original'); assert.equal(p.fields.agent.value, 'codex');
    assert.equal(p.nodes['#brainstorm-reply'].value, 'À envoyer');
    assert.ok(p.fields.every(field => field.disabled));
  }
});

test('the visible conversation does not show a distracting Agents notification', async () => {

  for (const cardId of ['session', 'other']) {
    const messages = [], run = { id: 'run', cardId, kind: 'brainstorm', status: 'running' };
    const context = {
      state: { cards: [], runs: [run], settings: { platform: 'win32' } }, currentView: 'brainstorm', brainstormSession: 'session',
      api: async () => ({ cards: [], runs: [{ ...run, status: 'completed' }], settings: { platform: 'win32' } }),
      $: () => ({}), resetJiraButton: {}, connectorSupports: () => false, connectorName: () => '',
      renderTemplates() {}, renderProjects() {}, board() {}, updateDevServers() {}, updateAutomation() {},
      document: { querySelector: () => null }, toast: message => messages.push(message),
    };
    notifyCompletions(await context.api(), new Map([['run', 'running']]), new Map(), context); assert.equal(messages.length, cardId === 'session' ? 0 : 1);
  }
});
