import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, realpathSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import vm from 'node:vm';
import { createApp } from '../server.mjs';
import { openStore } from '../lib/store.mjs';
import { parsePreparation, preparationPrompt } from '../lib/ticket-preparation.mjs';

const proposal = { criteria: ['Le résultat est visible'], questions: ['Quel public est concerné ?'] };

test('preparation validates bounded criteria and questions without validating human checkboxes', () => {
  assert.deepEqual(parsePreparation('```json\n' + JSON.stringify(proposal) + '\n```'), { checklist: [{ text: proposal.criteria[0], done: false }], questions: proposal.questions });
  assert.equal(parsePreparation(JSON.stringify({ criteria: ['Une\nligne'], questions: [] })).checklist[0].text, 'Une ligne');
  for (const invalid of [null, {}, { ...proposal, criteria: [] }, { ...proposal, criteria: Array(21).fill('x') }, { ...proposal, criteria: [null] }, { ...proposal, criteria: ['x'.repeat(501)] }, { ...proposal, questions: Array(11).fill('x') }, { ...proposal, questions: [' '] }, { ...proposal, questions: [null] }, { ...proposal, questions: ['x'.repeat(501)] }]) assert.throws(() => parsePreparation(JSON.stringify(invalid)));
  assert.throws(() => parsePreparation('pas du JSON'));
  const prompt = preparationPrompt({ title: 'Exporter', criteria: ['CSV'] });
  assert.match(prompt, /lecture seule/); assert.match(prompt, /CSV/); assert.match(prompt, /N’invente pas/);
});

function request(app, path, method = 'POST', body = {}) {
  return new Promise(resolve => {
    const req = Readable.from(method === 'GET' ? [] : [JSON.stringify(body)]);
    Object.assign(req, { method, url: path, headers: { host: '127.0.0.1:4317', 'content-type': 'application/json', 'x-local-request': '1' } });
    const res = { status: 200, setHeader() {}, writeHead(status) { this.status = status; }, end(value) { resolve({ status: this.status, body: JSON.parse(value) }); } };
    app.server.emit('request', req, res);
  });
}

test('preparation uses read-only execution, keeps status and requirements, handles errors, cancellation and restart', async () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'ticket-preparation-'));
  let execution, line, finish;
  let app = createApp({ dataDir, launch(options, onLine, onEnd) { execution = options; line = onLine; finish = onEnd; return { stop() { finish(1); } }; } });
  app.server.address = () => ({ port: 4317 });
  try {
    const card = { id: 'ticket', title: 'Exporter', description: 'Un export', instructions: 'Conserver le CSV', checklist: [{ text: 'CSV', done: true }], repo: realpathSync(dataDir), agent: 'codex', mode: 'edit', status: 'doing', blockedBy: ['prerequisite'], ticket: { key: 'TEST-1' } };
    app.store.putCard(card);
    const path = '/api/cards/ticket/prepare-ticket';
    const started = await request(app, path);
    assert.equal(started.status, 200); assert.equal(started.body.kind, 'ticket-preparation');
    assert.equal(execution.mode, 'analyze'); assert.equal(execution.repo, card.repo);
    assert.match(execution.prompt, /Conserver le CSV/); assert.match(execution.prompt, /Un export/);
    assert.equal(app.store.card(card.id).worktree, undefined);
    assert.equal(app.store.card(card.id).status, 'doing');
    assert.equal((await request(app, path)).status, 409);
    line('stdout', JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: JSON.stringify(proposal) } })); finish(0);
    assert.deepEqual(app.store.run(started.body.id).preparation.questions, proposal.questions);
    assert.deepEqual(app.store.card(card.id).checklist, card.checklist);
    assert.equal(app.store.card(card.id).instructions, card.instructions);
    assert.equal(app.store.card(card.id).status, 'doing');
    const invalid = await request(app, path);
    line('stdout', JSON.stringify({ type: 'result', result: 'Réponse invalide' })); finish(0);
    assert.match(app.store.run(invalid.body.id).attention, /invalide/);
    const failed = await request(app, path); finish(1);
    assert.equal(app.store.run(failed.body.id).status, 'failed');
    const cancelled = await request(app, path);
    await request(app, `/api/runs/${cancelled.body.id}/cancel`);
    assert.equal(app.store.run(cancelled.body.id).status, 'cancelled');
    const interrupted = await request(app, path);
    await app.close();
    const store = openStore(join(dataDir, 'kanban.sqlite'));
    store.putRun({ ...interrupted.body, status: 'running' }); store.db.close();
    app = createApp({ dataDir }); app.server.address = () => ({ port: 4317 });
    assert.equal(app.store.run(interrupted.body.id).status, 'interrupted');
    assert.equal(app.store.card(card.id).status, 'doing');
    app.store.putCard({ ...card, archived: true });
    assert.equal((await request(app, path)).status, 409);
  } finally { await app.close(); rmSync(dataDir, { recursive: true, force: true }); }
});

test('proposal editor preserves edits during updates and transfers only on explicit action', async () => {
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8');
  const host = { dataset: {}, querySelectorAll: () => [criteria, questions, apply] };
  let renders = 0, copied = [], events = 0;
  Object.defineProperty(host, 'innerHTML', { set(value) { renders++; } });
  const criteria = { value: 'Critère modifié' }, questions = { value: 'Question modifiée ?' }, apply = {};
  const instructions = { value: 'Instructions humaines' }, details = {};
  const list = { replaceChildren() { copied = []; }, closest: () => details };
  const form = { dispatchEvent() { events++; } };
  const card = { id: 'ticket', title: 'Titre', instructions: instructions.value, checklist: [] };
  const run = { id: 'run', cardId: card.id, kind: 'ticket-preparation', status: 'completed', preparation: { checklist: [{ text: 'Proposé', done: false }], questions: [] }, requirements: { title: card.title, description: '', instructions: card.instructions, criteria: [] } };
  const elements = { '#ticket-proposal': host, '#apply-ticket-proposal': apply, '#proposed-criteria': criteria, '#proposed-questions': questions, '#instructions': instructions, '#criteria-list': list, '#card-form': form };
  const context = vm.createContext({ state: { cards: [card], runs: [run] }, $: selector => elements[selector], esc: s => s, action: f => f(), toast() {}, addCriterion: item => copied.push(item), Event: class {} });
  vm.runInContext(source.slice(source.indexOf('function renderTicketPreparation('), source.indexOf('function addCriterion(')), context);
  context.renderTicketPreparation(card.id); context.renderTicketPreparation(card.id);
  assert.equal(renders, 1); assert.equal(copied.length, 0);
  await apply.onclick();
  assert.equal(copied[0].text, 'Critère modifié'); assert.equal(copied[0].done, false);
  assert.match(instructions.value, /Instructions humaines[\s\S]*Question modifiée/);
  assert.equal(events, 1); assert.equal(details.open, true);
  context.renderTicketPreparation(card.id); assert.equal(apply.disabled, true);
  card.description = 'Besoin changé'; context.renderTicketPreparation(card.id);
  assert.equal(apply.disabled, true); assert.equal(criteria.disabled, true);
});
