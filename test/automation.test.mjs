import test from 'node:test';
import assert from 'node:assert/strict';
import { createAutomation } from '../lib/automation.mjs';
import { projectSettings, settingsForProject } from '../lib/project-settings.mjs';
import { reviewRequirements } from '../lib/reviews.mjs';

const finding = priority => ({ priority, title: 'Défaut', file: 'app.js', line: 1, problem: 'Scénario et impact', suggestion: 'Correction' });
function setup(overrides = {}) {
  const cards = new Map([['ticket', { id: 'ticket', title: 'Titre', agent: 'codex', repo: 'repo', status: 'todo', worktree: { path: 'worktree' } }]]), runs = new Map(), calls = [];
  const clone = value => structuredClone(value);
  const store = { cards: () => [...cards.values()].map(clone), card: id => clone(cards.get(id)), putCard: card => cards.set(card.id, clone(card)), putRun: run => runs.set(run.id, clone(run)) };
  let sequence = 0;
  const settings = { automationEnabled: true, autoFix: true, testCommand: 'npm test', prTarget: 'main' };
  const context = { source: 'a'.repeat(40), target: 'b'.repeat(40), base: 'c'.repeat(40) };
  const dependencies = {
    store, settingsFor: () => settings, changed() {}, syncStatus: async () => calls.push('sync'),
    executeAgent: async (card, options, review, signal, onStarted) => {
      calls.push(review ? 'review' : options.instructions.includes('Corrige uniquement') ? 'correction' : 'development');
      const run = { id: String(++sequence), status: 'completed', summary: 'Terminé', ...(review ? { kind: 'pr-review', reviewContext: review, review: { summary: 'Bilan', criteria: [], findings: [] } } : {}) };
      store.putRun(run); onStarted(run); return run;
    },
    preview: async () => ({ fingerprint: 'content', title: 'Titre', body: 'Description' }),
    publish: async (card, options) => { calls.push('publish'); assert.equal(options.commitChanges, true); assert.equal(options.draftOnly, true); assert.equal(options.base, 'main'); assert.match(options.body, /Résultat : réussi/); card.pullRequest = { number: 1, url: 'https://example.invalid/pr/1' }; store.putCard(card); },
    reviewContext: async () => context,
    verify: async (command, cwd, { onOutput }) => { calls.push('tests'); assert.equal(command, settings.testCommand); assert.equal(cwd, 'worktree'); onOutput('Tests OK'); },
    ...overrides,
  };
  const automation = createAutomation(dependencies);
  const wait = async () => { for (let i = 0; i < 100 && automation.busy('ticket'); i++) await new Promise(resolve => setTimeout(resolve, 1)); assert.equal(automation.busy('ticket'), false, 'Le parcours doit terminer'); return store.card('ticket'); };
  const start = () => automation.start(store.card('ticket'), { agent: 'codex', mode: 'edit', instructions: '' });
  return { automation, store, settings, calls, context, dependencies, wait, start, runs };
}

test('automatic workflow verifies, publishes, reviews and persists its final notification', async () => {
  const fixture = setup(); fixture.start();
  assert.throws(fixture.start, /déjà en cours/);
  const card = await fixture.wait();
  assert.deepEqual(fixture.calls, ['development', 'tests', 'publish', 'review', 'sync']);
  assert.equal(card.status, 'review'); assert.equal(card.automation.state, 'ready');
  assert.equal(card.automation.tests.state, 'passed'); assert.equal(card.automation.tests.output, 'Tests OK');
  assert.equal(card.automation.counts.P0, 0); assert.equal(card.automation.cycles, 0);
  assert.equal(card.automation.reportId, '2'); assert.match(card.automation.message, /aucun critère/);
});

test('failed tests stop publication and resume only the failed step', async () => {
  let failed = true;
  const fixture = setup({ verify: async () => { if (failed) throw new Error('Test rouge'); } });
  fixture.start(); let card = await fixture.wait();
  assert.equal(card.automation.state, 'error'); assert.equal(card.automation.step, 'tests');
  assert.equal(card.automation.tests.state, 'failed'); assert.match(card.automation.message, /Test rouge/);
  assert.deepEqual(fixture.calls, ['development']);
  failed = false; fixture.automation.start(card, {}, true); card = await fixture.wait();
  assert.equal(card.automation.state, 'ready'); assert.equal(fixture.calls.filter(c => c === 'development').length, 1);
});

test('changes during tests or after testing cannot be published', async () => {
  for (const changeAt of [2, 3]) {
    let previews = 0;
    const fixture = setup({ preview: async () => ({ fingerprint: ++previews >= changeAt ? 'changed' : 'original' }) });
    fixture.start(); const card = await fixture.wait();
    assert.equal(card.automation.state, 'error'); assert.equal(card.automation.step, 'tests');
    assert.equal(fixture.calls.includes('publish'), false);
  }
});

test('P0/P1 corrections are limited to two cycles and never include suggestions', async () => {
  let corrections = 0, reviews = 0, sequence = 0;
  const second = setup({ executeAgent: async (card, options, context, signal, onStarted) => {
    if (!context && options.instructions) { corrections++; assert.match(options.instructions, /P1/); assert.doesNotMatch(options.instructions, /Facultatif/); }
    if (context) reviews++;
    const run = { id: String(++sequence), status: 'completed', kind: context ? 'pr-review' : undefined, review: context ? { summary: 'Bilan', criteria: [], findings: [finding('P1'), { ...finding('suggestion'), title: 'Facultatif' }] } : undefined };
    onStarted(run); return run;
  } });
  second.start(); const card = await second.wait();
  assert.equal(corrections, 2); assert.equal(reviews, 3); assert.equal(card.automation.cycles, 2);
  assert.equal(card.automation.state, 'blocked'); assert.equal(card.status, 'standby');
  assert.equal(card.automation.counts.suggestion, 1); assert.equal(card.automation.counts.P1, 1);
});

test('blocking findings remain manual when auto correction is disabled', async () => {
  const fixture = setup({ executeAgent: async (card, options, context, signal, onStarted) => {
    const run = { id: context ? 'review' : 'dev', status: 'completed', review: context ? { summary: 'Bilan', criteria: [], findings: [finding('P0')] } : undefined }; onStarted(run); return run;
  } });
  fixture.settings.autoFix = false; fixture.start(); const card = await fixture.wait();
  assert.equal(card.automation.state, 'blocked'); assert.equal(card.automation.cycles, 0);
});

test('invalid or outdated reviews never become ready; review resumes without publication', async () => {
  let invalid = true;
  const fixture = setup({ executeAgent: async (card, options, context, signal, onStarted) => {
    const run = { id: context ? 'review' : 'dev', status: 'completed', review: context && !invalid ? { summary: 'Bilan', criteria: [], findings: [finding('suggestion')] } : undefined }; onStarted(run); return run;
  } });
  fixture.start(); let card = await fixture.wait();
  assert.equal(card.automation.state, 'error'); assert.equal(card.automation.step, 'review');
  invalid = false; fixture.automation.start(card, {}, true); card = await fixture.wait();
  assert.equal(card.automation.state, 'ready'); assert.equal(card.automation.counts.suggestion, 1);
  assert.equal(fixture.calls.filter(c => c === 'publish').length, 1);
  let count = 0;
  const outdated = setup({ reviewContext: async () => ({ source: String(++count), target: 'target' }) });
  outdated.start(); card = await outdated.wait();
  assert.equal(card.automation.state, 'error'); assert.match(card.automation.message, /changé/);
  assert.ok([...outdated.runs.values()].some(run => run.reviewStaleReason));
});

test('cancel during checks prevents publication and server shutdown drains the workflow', async () => {
  let entered;
  const started = new Promise(resolve => { entered = resolve; });
  const fixture = setup({ verify: async (command, cwd, { signal }) => {
    entered(); await new Promise(resolve => signal.addEventListener('abort', resolve, { once: true }));
  } });
  fixture.start(); await started; await fixture.automation.close();
  const card = await fixture.wait();
  assert.equal(card.automation.state, 'cancelled'); assert.equal(fixture.calls.includes('publish'), false);
});

test('restart interrupts persisted workflows and preserves their next step', () => {
  const fixture = setup();
  const card = fixture.store.card('ticket'); card.automation = { state: 'running', step: 'publish', cycles: 1 }; fixture.store.putCard(card);
  createAutomation(fixture.dependencies);
  const recovered = fixture.store.card('ticket');
  assert.equal(recovered.automation.state, 'interrupted'); assert.equal(recovered.automation.step, 'publish');
  assert.equal(recovered.status, 'standby'); assert.equal(recovered.automation.cycles, 1);
});

test('resuming with a different check command reruns checks before reviewing', async () => {
  let failed = true;
  const fixture = setup({ reviewContext: async () => { if (failed) throw new Error('Indisponible'); return { source: 'a', target: 'b' }; } });
  fixture.start(); let card = await fixture.wait(); assert.equal(card.automation.step, 'review');
  failed = false; fixture.settings.testCommand = 'npm run checks'; fixture.automation.start(card, {}, true); card = await fixture.wait();
  assert.equal(card.automation.state, 'ready'); assert.equal(card.automation.tests.command, 'npm run checks');
  assert.equal(fixture.calls.filter(c => c === 'tests').length, 2);
});

test('review waits for Azure commit metadata before starting the reviewer', async () => {
  let attempts = 0;
  const fixture = setup({ reviewContext: async () => {
    if (++attempts === 1) throw Object.assign(new Error('Azure actualise la PR'), { retryable: true });
    return { source: 'a', target: 'b' };
  } });
  fixture.start();
  await new Promise(resolve => setTimeout(resolve, 1050));
  const card = await fixture.wait();
  assert.equal(card.automation.state, 'ready'); assert.equal(attempts, 3);
});

test('automation needs explicit per-project activation and validates configuration', () => {
  assert.equal(settingsForProject({ automationEnabled: true }, [], '').automationEnabled, false);
  assert.throws(() => projectSettings({ automationEnabled: 'on' }), /invalide/);
  assert.throws(() => projectSettings({ automationEnabled: true }), /commande/);
  assert.throws(() => projectSettings({ prTarget: '-main' }), /commande/);
  const saved = projectSettings({ automationEnabled: true, testCommand: 'npm test', prTarget: 'main' });
  assert.equal(saved.autoFix, true);
  const fixture = setup(); fixture.settings.testCommand = '';
  assert.throws(fixture.start, /configure/); assert.equal(fixture.calls.length, 0);
});

test('functional gaps or unverified criteria require human intervention even with no technical defect', async () => {
  for (const status of ['covered', 'not-covered', 'unverifiable']) {
    const fixture = setup({ executeAgent: async (card, options, context, signal, onStarted) => {
      const run = { id: context ? 'review' : 'dev', status: 'completed', ...(context ? { kind: 'pr-review', review: { summary: 'Bilan', findings: [], criteria: card.checklist.map((item, index) => ({ index, text: item.text, status, evidence: 'src/app.js:12 ; limite de vérification explicitée.' })) } } : {}) };
      onStarted(run); return run;
    } });
    const initial = fixture.store.card('ticket'); initial.checklist = [{ text: 'Comportement attendu', done: false }]; fixture.store.putCard(initial);
    fixture.start(); const card = await fixture.wait();
    assert.equal(card.automation.state, status === 'covered' ? 'ready' : 'blocked');
    assert.equal(card.status, status === 'covered' ? 'review' : 'standby');
    assert.equal(card.automation.acceptance.total, 1); assert.equal(card.automation.acceptance.covered, status === 'covered' ? 1 : 0);
    assert.equal(card.checklist[0].done, false); assert.equal(card.automation.cycles, 0);
    assert.match(card.automation.message, status === 'covered' ? /prête/ : /Intervention humaine/);
  }
});

test('a missing coverage report cannot validate a ticket with acceptance criteria', async () => {
  const fixture = setup();
  const card = fixture.store.card('ticket'); card.checklist = [{ text: 'Comportement attendu', done: true }]; fixture.store.putCard(card);
  fixture.start(); const result = await fixture.wait();
  assert.equal(result.automation.state, 'error'); assert.match(result.automation.message, /critères actuels/);
  assert.equal(result.checklist[0].done, true);
});

test('a requirement changed during review invalidates the result before readiness', async () => {
  let fixture;
  fixture = setup({ executeAgent: async (card, options, context, signal, onStarted) => {
    const run = { id: context ? 'review' : 'dev', status: 'completed', ...(context ? { kind: 'pr-review', reviewContext: { ...context, requirements: reviewRequirements(card) }, review: { summary: 'Bilan', findings: [], criteria: [] } } : {}) };
    onStarted(run);
    if (context) { const updated = fixture.store.card(card.id); updated.description = 'Nouveau besoin Jira'; fixture.store.putCard(updated); }
    return run;
  } });
  fixture.start(); const result = await fixture.wait();
  assert.equal(result.automation.state, 'error'); assert.match(result.automation.message, /besoin ou les critères ont changé/);
  assert.match(fixture.runs.get('review').reviewStaleReason, /besoin ou les critères ont changé/);
});
