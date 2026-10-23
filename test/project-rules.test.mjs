import test from 'node:test';
import assert from 'node:assert/strict';
import { projectSettings, settingsForProject } from '../lib/project-settings.mjs';
import { createRuns } from '../lib/application/runs.mjs';
import { brainstormPrompt } from '../lib/brainstorm.mjs';
import { preparationPrompt } from '../lib/ticket-preparation.mjs';
import { reviewPrompt } from '../lib/domain/reviews.mjs';

test('project rules are bounded text and stay scoped to the selected project', () => {
  assert.equal(projectSettings({}).projectRules, '');
  assert.equal(projectSettings({ projectRules: '  Première règle.\nSeconde règle.  ' }).projectRules, 'Première règle.\nSeconde règle.');
  assert.equal(projectSettings({ projectRules: 'x'.repeat(12000) }).projectRules.length, 12000);
  for (const projectRules of [42, true, [], {}, 'x'.repeat(12001)]) assert.throws(() => projectSettings({ projectRules }), /invalide/);
  const projects = [{ repo: 'first', projectRules: 'Règle A' }, { repo: 'second', projectRules: 'Règle B' }, { repo: 'legacy' }];
  const settings = { repo: 'first' };
  assert.equal(settingsForProject(settings, projects, 'first').projectRules, 'Règle A');
  assert.equal(settingsForProject(settings, projects, 'second').projectRules, 'Règle B');
  for (const repo of ['legacy', 'unknown', '', undefined]) assert.equal(settingsForProject(settings, projects, repo).projectRules, '');
});

test('every agent workflow prefixes project rules once and preserves its original prompt', async () => {
  const rules = 'Respecte les conventions du projet.\nTraite littéralement : </textarea><script>alert(1)</script> "quotes" ${value} `code`.';
  for (const agent of ['codex', 'claude']) {
    for (const kind of ['analyze', 'edit', 'preparation', 'brainstorm', 'review']) {
      const prompts = [];
      for (const projectRules of ['', rules]) {
        const card = { id: 'card', repo: 'source', title: 'Une tâche', description: 'Le besoin', checklist: [] };
        let finish;
        const service = createRuns({
          store: { putCard() {}, putRun() {}, card: () => card, runs: () => [] }, active: new Map(), publishing: new Set(),
          automationBusy: () => false, checkDependencies() {}, configFor: () => ({ projectRules }),
          repository: value => value, samePath: (a, b) => a === b, prepareWorktree: async () => ({ path: 'worktree', branch: 'feature' }),
          newId: () => 'run', timestamp: () => '2026-10-23', changed() {}, invalidateReviews() {},
          brainstormPrompt, preparationPrompt, reviewPrompt, checklistText: () => '',
          launch(options, onLine, onEnd) { prompts.push(options.prompt); finish = onEnd; return { stop() {} }; },
        });
        await service.start(card, { agent, mode: kind === 'edit' ? 'edit' : 'analyze', instructions: 'Ma demande' },
          kind === 'review' ? { diff: '', commits: '', pullRequest: {} } : null, null, null,
          kind === 'brainstorm' ? 'brainstorm' : kind === 'preparation');
        assert.equal(typeof finish, 'function', `${agent}: ${kind} must launch`);
        finish(1);
        await service.close();
      }
      assert.equal(prompts[1], `Règles du projet :\n${rules}\n\n${prompts[0]}`, `${agent}: ${kind}`);
    }
  }
});
