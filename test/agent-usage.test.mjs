import { msg, markupText, locale, messageText, localizeModule } from './helpers/french-locale.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { recordAgentUsage, codexSessionModel } from '../lib/agent-usage.mjs';
import { usageText, usageTotals } from '../public/agent-usage.mjs';

const emit = (run, event) => recordAgentUsage(run, JSON.stringify(event));
const turn = (input_tokens = 1000, cached_input_tokens = 800, output_tokens = 100) => ({ type: 'turn.completed', usage: { input_tokens, cached_input_tokens, output_tokens } });

test('Codex totals include cache once and accumulate each turn at the selected API rate', () => {
  const run = { agent: 'codex', model: 'gpt-5.3-codex', status: 'completed' };
  emit(run, turn()); emit(run, turn());
  assert.equal(run.usage.inputTokens, 2000);
  assert.equal(run.usage.cacheReadTokens, 1600);
  assert.equal(run.usage.outputTokens, 200);
  assert.equal(run.usage.totalTokens, 2200);
  assert.ok(Math.abs(run.usage.costUsd - 0.00378) < 1e-12);
  assert.equal(run.usage.pricingModel, 'gpt-5.3-codex');
  assert.match(usageText(run), /Input : 2.*000/);
});

test('Codex prices the selected model and never substitutes another model for an unknown price', () => {
  for (const [model, expected] of [['gpt-5.4', 0.0022], ['gpt-5.1-codex-mini', 0.00027], ['gpt-6.1-sol', 0.00148], ['gpt-6-astra', 0.0078], ['gpt-6-luna', 0.000078], ['gpt-5.6-sol', 0.00312], ['', null], ['custom', null], ['toString', null]]) {
    const run = { agent: 'codex', model };
    emit(run, turn());
    if (expected === null) {
      assert.equal(run.usage.costUsd, null);
      assert.match(usageText(run), /Coût indisponible/);
    } else assert.ok(Math.abs(run.usage.costUsd - expected) < 1e-12);
    assert.equal(run.usage.pricingModel, model || null);
  }
});

test('Codex reads only the matching session model, ahead of any configured model', () => {
  const root = new URL('../.test-tmp/', import.meta.url);
  mkdirSync(root, { recursive: true });
  const home = mkdtempSync(new URL('usage-model-', root));
  const directory = join(home, 'sessions', '2026', '10', '21');
  mkdirSync(directory, { recursive: true });
  const id = '019d1234-1234-1234-1234-123456789abc';
  const file = join(directory, `rollout-2026-10-21T12-00-00-${id}.jsonl`);
  try {
    writeFileSync(join(directory, 'rollout-2026-10-21T13-00-00-019d1234-1234-1234-1234-123456789def.jsonl'), JSON.stringify({ type: 'turn_context', payload: { model: 'unrelated' } }) + '\n');
    writeFileSync(file, JSON.stringify({ type: 'session_meta', payload: { id } }) + '\n' + JSON.stringify({ type: 'turn_context', payload: { model: 'gpt-6.1-sol' } }) + '\n');
    for (const model of ['', 'gpt-5.4']) {
      const run = { agent: 'codex', model };
      recordAgentUsage(run, JSON.stringify({ type: 'thread.started', thread_id: id }), home);
      recordAgentUsage(run, JSON.stringify(turn()), home);
      assert.equal(run.actualModel, 'gpt-6.1-sol');
      assert.equal(run.usage.pricingModel, 'gpt-6.1-sol');
      assert.ok(Math.abs(run.usage.costUsd - 0.00148) < 1e-12);
      assert.match(usageText(run), /^gpt-6.1-sol/);
    }
    writeFileSync(file, 'invalid\n' + JSON.stringify({ type: 'turn_context', payload: { model: 'custom-actual' } }) + '\n');
    const unknown = { agent: 'codex', model: 'gpt-5.4', threadId: id };
    recordAgentUsage(unknown, JSON.stringify(turn()), home);
    assert.equal(unknown.actualModel, 'custom-actual');
    assert.equal(unknown.usage.costUsd, null);
    assert.equal(codexSessionModel('../escape', home), '');
    assert.equal(codexSessionModel('019d1234-1234-1234-1234-000000000000', home), '');
    writeFileSync(file, '{"type":"turn_context","payload":');
    assert.equal(codexSessionModel(id, home), '');
  } finally { rmSync(home, { recursive: true, force: true }); }
});

test('Claude result totals include every model and cache without counting assistant messages twice', () => {
  const run = { agent: 'claude', model: 'sonnet', status: 'completed' };
  emit(run, { type: 'system', subtype: 'init', model: 'claude-sonnet-4-6' });
  emit(run, { type: 'assistant', message: { usage: { input_tokens: 100 } } });
  const result = { type: 'result', total_cost_usd: 0.123, usage: { input_tokens: 1, output_tokens: 1 }, modelUsage: {
    'claude-sonnet-4-6': { inputTokens: 100, outputTokens: 20, cacheReadInputTokens: 800, cacheCreationInputTokens: 200 },
    'claude-haiku-4-5': { inputTokens: 50, outputTokens: 10, cacheReadInputTokens: 100, cacheCreationInputTokens: 0 },
  } };
  emit(run, result); emit(run, result);
  assert.equal(run.actualModel, 'claude-sonnet-4-6');
  assert.equal(run.usage.inputTokens, 1250);
  assert.equal(run.usage.outputTokens, 30);
  assert.equal(run.usage.totalTokens, 1280);
  assert.equal(run.usage.costUsd, 0.123);
  assert.equal(run.usage.scope, 'all-agents');
  assert.match(usageText(run), /claude-sonnet-4-6, claude-haiku-4-5/);
});

test('Claude fallback usage keeps missing prices distinct from zero, including error results', () => {
  const run = { agent: 'claude', status: 'failed' };
  emit(run, { type: 'result', is_error: true, usage: { input_tokens: 100, output_tokens: 20, cache_read_input_tokens: 80, cache_creation_input_tokens: 10 } });
  assert.equal(run.usage.inputTokens, 190);
  assert.equal(run.usage.totalTokens, 210);
  assert.equal(run.usage.costUsd, null);
  assert.match(usageText(run), /principal uniquement/);
  assert.match(usageText(run), /partiel possible/);
  emit(run, { type: 'result', total_cost_usd: 0, usage: { input_tokens: 0, output_tokens: 0 } });
  assert.equal(run.usage.costUsd, 0);
});

test('malformed and unrelated events never invent usage or overwrite valid counters', () => {
  const run = { agent: 'codex', model: 'gpt-5.4' };
  emit(run, turn());
  const previous = structuredClone(run);
  for (const event of [null, {}, turn(-1), turn(1, 2), turn('100'), turn(1, 0, 1.5), { type: 'result', usage: {} }]) emit(run, event);
  recordAgentUsage(run, 'not JSON');
  assert.deepEqual(run, previous);
});

test('display and aggregate stats distinguish historical/missing usage from measured free runs', () => {
  assert.match(usageText({ status: 'running' }), /en attente/);
  assert.match(usageText({ status: 'interrupted' }), /indisponibles/);
  assert.deepEqual(usageTotals([{ usage: { costUsd: 0 } }, { usage: { costUsd: 0.25 } }, {}, { usage: { costUsd: null } }]), { costUsd: 0.25, measured: 2, missing: 2 });
  assert.deepEqual(usageTotals([]), { costUsd: 0, measured: 0, missing: 0 });
  const legacy = { usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0, totalTokens: 15, costUsd: 1, referenceModel: true } };
  assert.deepEqual(usageTotals([legacy]), { costUsd: 0, measured: 0, missing: 1 });
  assert.match(usageText(legacy), /ancienne estimation sur un autre modèle/);
  const source = readFileSync(new URL('../public/app.js', import.meta.url), 'utf8') + readFileSync(new URL('../public/views/card.mjs', import.meta.url), 'utf8');
  assert.ok(source.includes("$('#run-usage').textContent = usageText(run)"));
  assert.ok(source.includes('${esc(usageText(run))}'));
  assert.ok(source.includes('const usage = usageTotals(runs)'));
});
