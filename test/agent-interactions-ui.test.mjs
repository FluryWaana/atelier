import './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createAgentInteractions } from '../public/views/agent-interactions.mjs';

function fixture(api = async () => {}) {
  const request = { id: 'q', kind: 'question', title: '<script>Question</script>', questions: [{ id: '0', question: 'Quel format ?', options: [{ label: '<img src=x>', description: 'Description' }] }] };
  const state = { cards: [{ id: 'card', title: '<b>Carte</b>' }], runs: [{ id: 'run', cardId: 'card', status: 'running', interactions: [request] }] };
  const reply = { dataset: { reply: 'q' } }, later = {}, free = { value: '' }, choice = { value: '0' }, fields = [free, choice, later];
  const banner = { innerHTML: '', querySelectorAll: () => [reply], querySelector: () => null };
  const dialog = { open: false, showModal() { this.open = true; }, close() { this.open = false; } };
  let html = '', selections = [], pending;
  const form = { elements: { namedItem: () => free }, querySelector: () => later,
    querySelectorAll: selector => selector.includes(':checked') ? selections : fields,
    get innerHTML() { return html; }, set innerHTML(value) { html = value; free.value = ''; selections = []; },
  };
  const nodes = { '#agent-interactions': banner, '#agent-reply-dialog': dialog, '#agent-reply-form': form };
  const document = { hidden: true, querySelector: selector => nodes[selector] };
  const notices = [], toasts = [], calls = [];
  class Notification { static permission = 'granted'; constructor(title, options) { Object.assign(this, { title, options }); notices.push(this); } close() {} }
  const view = createAgentInteractions({ document, window: { Notification, focus() {} }, getState: () => state, api: async (...args) => { calls.push(args); return api(...args); },
    action: fn => { pending = fn(); }, refresh: async () => view.render(), toast: (...args) => toasts.push(args) });
  const submit = value => form.onsubmit({ preventDefault() {}, submitter: { value } });
  return { view, state, request, banner, dialog, form, free, choice, fields, document, notices, toasts, calls, submit, pending: () => pending, open: () => reply.onclick(), later: () => later.onclick(), select: () => { selections = [choice]; } };
}

test('waiting notifications persist across refresh, escape content and appear only once', () => {
  const f = fixture();
  f.view.render();
  assert.equal(f.banner.hidden, false);
  assert.match(f.banner.innerHTML, /&lt;b&gt;Carte&lt;\/b&gt;/);
  assert.equal(f.notices.length, 1);
  assert.match(f.document.title, /\(1\)/);
  f.open();
  assert.equal(f.dialog.open, true);
  assert.match(f.form.innerHTML, /&lt;script&gt;Question/);
  assert.match(f.form.innerHTML, /&lt;img src=x&gt;/);
  f.free.value = 'En cours de saisie';
  f.view.render();
  assert.equal(f.free.value, 'En cours de saisie');
  assert.equal(f.notices.length, 1);
  f.later();
  assert.equal(f.dialog.open, false); assert.equal(f.banner.hidden, false);
  f.open();
  f.state.runs[0].interactions = [];
  f.view.render();
  assert.equal(f.dialog.open, false); assert.equal(f.banner.hidden, true);
  assert.equal(f.document.title, 'Atelier Kanban');
});

test('answers require input, submit once and preserve the draft after a transport failure', async () => {
  let fail;
  const f = fixture(() => new Promise((resolve, reject) => { fail = reject; }));
  f.view.render(); f.open();
  f.submit('answer');
  assert.equal(f.calls.length, 0);
  assert.equal(f.toasts.at(-1)[0], 'Répondez à chaque question.');
  f.select(); f.submit('answer'); f.submit('answer');
  assert.equal(f.calls.length, 1);
  assert.deepEqual(f.calls[0], ['/api/runs/run/reply', 'POST', { requestId: 'q', decision: 'answer', answers: { 0: ['<img src=x>'] } }]);
  assert.ok(f.fields.every(field => field.disabled));
  f.free.value = 'Autre réponse';
  fail(new Error('Offline'));
  await assert.rejects(f.pending(), /Offline/);
  assert.equal(f.free.value, 'Autre réponse');
  assert.equal(f.dialog.open, true);
  assert.ok(f.fields.every(field => !field.disabled));
});

test('a free-text reply overrides an option and declining does not require answers', async () => {
  const f = fixture(); f.view.render(); f.open();
  f.select(); f.free.value = '  Autre choix  '; f.submit('answer');
  await f.pending();
  assert.deepEqual(f.calls[0][2].answers, { 0: ['Autre choix'] });
  assert.equal(f.dialog.open, false);
  f.request.kind = 'approval'; f.request.canAllow = false; delete f.request.questions;
  f.open();
  assert.doesNotMatch(f.form.innerHTML, /value="allow"/);
  f.submit('deny'); await f.pending();
  assert.equal(f.calls.at(-1)[2].decision, 'deny');
});

test('a late reply cannot close another request or erase its draft', async () => {
  let finish;
  const f = fixture(() => new Promise(resolve => { finish = resolve; }));
  f.view.render(); f.open(); f.free.value = 'Réponse A'; f.submit('answer');
  const pendingA = f.pending();
  const second = { ...f.request, id: 'second', title: 'Question B' };
  f.state.runs[0].interactions.push(second); f.view.render();
  f.notices.at(-1).onclick();
  assert.match(f.form.innerHTML, /Question B/);
  f.free.value = 'Brouillon B';
  f.state.runs[0].interactions = [second];
  finish(); await pendingA;
  assert.equal(f.dialog.open, true);
  assert.equal(f.free.value, 'Brouillon B');
});

test('concurrent replies keep separate submission locks on success and failure', async () => {
  for (const failed of [false, true]) {
    const completions = [];
    const f = fixture(() => new Promise((resolve, reject) => { completions.push({ resolve, reject }); }));
    f.view.render(); f.open(); f.free.value = 'Réponse A'; f.submit('answer');
    const pendingA = f.pending();
    const second = { ...f.request, id: 'second', title: 'Question B' };
    f.state.runs[0].interactions.push(second); f.view.render(); f.notices.at(-1).onclick();
    assert.ok(f.fields.every(field => !field.disabled), 'a new request has its own controls');
    f.free.value = 'Réponse B'; f.submit('answer');
    const pendingB = f.pending();
    assert.equal(f.calls.length, 2);
    if (failed) {
      completions[0].reject(new Error('Offline A'));
      await assert.rejects(pendingA, /Offline A/);
    } else { completions[0].resolve(); await pendingA; }
    assert.equal(f.dialog.open, true);
    assert.ok(f.fields.every(field => field.disabled), 'answer A cannot unlock the pending answer B');
    f.submit('answer'); assert.equal(f.calls.length, 2, 'B still rejects double submissions');
    completions[1].resolve(); await pendingB;
    assert.equal(f.dialog.open, false);
    assert.ok(f.fields.every(field => !field.disabled));
  }
});
