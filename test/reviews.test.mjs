import test from 'node:test';
import assert from 'node:assert/strict';
import { parseReview, correctionInstructions, prepareReview, reviewPrompt, reviewRequirements } from '../lib/reviews.mjs';

const finding = priority => ({ priority, title: 'Titre', file: 'app.js', line: 1, problem: 'Scénario', suggestion: 'Correction' });
test('review validates and orders defects before optional suggestions', () => {
  const report = parseReview(JSON.stringify({ summary: 'Bilan', findings: ['suggestion', 'P2', 'P0', 'P3', 'P1'].map(finding) }));
  assert.deepEqual(report.findings.map(f => f.priority), ['P0', 'P1', 'P2', 'P3', 'suggestion']);
  for (const change of [{ priority: 'critical' }, { line: 0 }, { problem: null }]) {
    assert.throws(() => parseReview(JSON.stringify({ summary: 'Bilan', findings: [{ ...finding('P1'), ...change }] })), /invalide/);
  }
  assert.throws(() => parseReview('Compte rendu non structuré'));
  assert.match(reviewPrompt({}), /suggestion/);
  assert.throws(() => correctionInstructions({ kind: 'pr-review', status: 'completed', review: report, reviewStaleReason: 'Périmé' }, [0]), /à jour/);
});

test('review checks that the published source and target match the clean worktree', async () => {
  const source = 'a'.repeat(40), target = 'b'.repeat(40), base = 'c'.repeat(40);
  const card = { branch: 'feature/test', worktree: { path: 'repo' }, pullRequest: { repository: 'org/project/repo', number: 1 } };
  let dirty = false;
  const git = async (command, args) => {
    const text = args.join(' ');
    if (text === 'remote get-url origin') return 'https://dev.azure.com/org/project/_git/repo';
    if (text === 'branch --show-current') return card.branch;
    if (text === 'rev-parse HEAD') return source;
    if (text === 'status --porcelain') return dirty ? ' M app.js' : '';
    if (text === 'rev-parse FETCH_HEAD') return target;
    if (args[0] === 'merge-base') return base;
    return '';
  };
  const request = async () => ({ status: 'active', sourceRefName: 'refs/heads/feature/test', targetRefName: 'refs/heads/main', lastMergeSourceCommit: { commitId: source }, lastMergeTargetCommit: { commitId: target } });
  assert.equal((await prepareReview(card, '', git, request)).source, source);
  dirty = true; await assert.rejects(prepareReview(card, '', git, request), /worktree propre/);
});

test('criteria coverage keeps the original text, orders assessments and requires evidence for every criterion', () => {
  const criteria = ['Connexion refusée sans mot de passe', 'Message visible après succès', 'Export accessible'];
  const report = { summary: 'Bilan', findings: [], criteria: [
    { index: 2, status: 'unverifiable', evidence: 'Accès au service indisponible.' },
    { index: 0, text: 'Texte inventé', status: 'covered', evidence: ' auth.mjs:12 refuse la requête ; test/auth.test.mjs couvre le cas. ' },
    { index: 1, status: 'not-covered', evidence: 'app.js:42 ne rend aucun message après succès.' },
  ] };
  const parsed = parseReview(JSON.stringify(report), criteria);
  assert.deepEqual(parsed.criteria.map(item => item.text), criteria);
  assert.deepEqual(parsed.criteria.map(item => item.status), ['covered', 'not-covered', 'unverifiable']);
  assert.match(parsed.criteria[0].evidence, /^auth.mjs/);
  for (const change of [
    { criteria: undefined }, { criteria: report.criteria.slice(1) }, { criteria: [...report.criteria, report.criteria[0]] },
    { criteria: report.criteria.map(item => ({ ...item, index: 0 })) },
    { criteria: report.criteria.map(item => ({ ...item, index: -1 })) },
    { criteria: report.criteria.map(item => ({ ...item, status: 'passed' })) },
    { criteria: report.criteria.map(item => ({ ...item, evidence: '   ' })) },
    { criteria: report.criteria.map(item => ({ ...item, evidence: 'x'.repeat(5001) })) },
    { criteria: [null, ...report.criteria.slice(1)] },
  ]) assert.throws(() => parseReview(JSON.stringify({ ...report, ...change }), criteria), /critères/);
  assert.deepEqual(parseReview(JSON.stringify({ summary: 'Aucun critère', findings: [], criteria: [] }), []).criteria, []);
  assert.equal(parseReview(JSON.stringify({ summary: 'Ancienne review', findings: [] })).criteria, undefined);
});

test('the reviewer receives a requirements snapshot and test evidence without human checkbox states', () => {
  const card = { title: 'Connexion', description: 'Une session doit être ouverte.', instructions: 'Conserver le comportement existant.', checklist: [{ text: 'Refus sans mot de passe', done: true }] };
  const requirements = reviewRequirements(card);
  const prompt = reviewPrompt({ requirements, verification: { command: 'npm test', output: 'Tests OK', state: 'passed' } });
  assert.match(prompt, /Une session doit être ouverte/); assert.match(prompt, /Refus sans mot de passe/);
  assert.match(prompt, /Tests OK/); assert.match(prompt, /unverifiable/); assert.match(prompt, /Ne coche jamais/);
  assert.doesNotMatch(prompt, /"done":true/);
  card.checklist[0].text = 'Autre besoin'; assert.equal(requirements.criteria[0], 'Refus sans mot de passe');
});
