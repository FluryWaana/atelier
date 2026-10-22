import { globSync, openSync, readSync, closeSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

// Standard USD / million tokens, checked 2026-10-21 against the official model pages.
// https://developers.openai.com/api/docs/pricing
const codexPrices = {
  'gpt-6.1-sol': [2, 0.1, 10],
  'gpt-6-astra': [10, 1, 50],
  'gpt-6-luna': [0.1, 0.01, 0.5],
  'gpt-5.6-sol': [4, 0.4, 20],
  'gpt-5.4': [2.5, 0.25, 15],
  'gpt-5.3-codex': [1.75, 0.175, 14],
  'gpt-5.1-codex-mini': [0.25, 0.025, 2],
};
const count = value => Number.isSafeInteger(value) && value >= 0;
const amount = value => typeof value === 'number' && Number.isFinite(value) && value >= 0;

export function codexSessionModel(threadId, codexHome = process.env.CODEX_HOME || join(homedir(), '.codex')) {
  if (typeof threadId !== 'string' || !/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(threadId)) return '';
  let fd;
  try {
    const [file] = globSync(`sessions/*/*/*/rollout-*-${threadId}.jsonl`, { cwd: codexHome });
    if (!file) return '';
    fd = openSync(join(codexHome, file), 'r');
    // shortcut: inspect only the first MiB for turn metadata; report unknown if a future CLI moves it beyond this bound.
    const buffer = Buffer.alloc(1024 * 1024);
    const size = readSync(fd, buffer);
    for (const line of buffer.toString('utf8', 0, size).split('\n')) {
      if (!line.includes('"turn_context"')) continue;
      try {
        const event = JSON.parse(line);
        if (event.type === 'turn_context' && typeof event.payload?.model === 'string' && event.payload.model.length <= 180) return event.payload.model;
      } catch { /* A trailing partial line is not metadata. */ }
    }
  } catch { /* Session files are optional; never fail the agent for missing metadata. */ }
  finally { if (fd !== undefined) closeSync(fd); }
  return '';
}

export function recordAgentUsage(run, line, codexHome) {
  let event;
  try { event = JSON.parse(line); } catch { return; }
  if (!event || typeof event !== 'object') return;
  if ((run.agentType || run.agent) === 'codex' && event.type === 'thread.started' && typeof event.thread_id === 'string') run.threadId = event.thread_id;
  if ((run.agentType || run.agent) === 'claude' && event.type === 'system' && event.subtype === 'init' && typeof event.model === 'string') run.actualModel = event.model;
  if ((run.agentType || run.agent) === 'codex' && event.type === 'turn.completed' && event.usage) {
    const u = event.usage;
    if (![u.input_tokens, u.cached_input_tokens, u.output_tokens].every(count) || u.cached_input_tokens > u.input_tokens) return;
    const previous = run.usage;
    const inputTokens = (previous?.inputTokens || 0) + u.input_tokens;
    const cacheReadTokens = (previous?.cacheReadTokens || 0) + u.cached_input_tokens;
    const outputTokens = (previous?.outputTokens || 0) + u.output_tokens;
    const actualModel = codexSessionModel(run.threadId, codexHome);
    if (actualModel) run.actualModel = actualModel;
    const pricingModel = run.actualModel || run.model || null;
    const prices = Object.hasOwn(codexPrices, pricingModel) ? codexPrices[pricingModel] : null;
    // shortcut: CLI totals omit service tier and per-request context sizes; estimate standard short-context tokens only until those are exposed.
    const costUsd = prices ? ((inputTokens - cacheReadTokens) * prices[0] + cacheReadTokens * prices[1] + outputTokens * prices[2]) / 1e6 : null;
    run.usage = { inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens: 0, totalTokens: inputTokens + outputTokens, costUsd, pricingModel, source: 'standard-api', pricingDate: '2026-10-21' };
  }
  if ((run.agentType || run.agent) === 'claude' && event.type === 'result') {
    const models = event.modelUsage && typeof event.modelUsage === 'object' ? Object.entries(event.modelUsage) : [];
    const u = event.usage;
    const entries = models.length ? models.map(([, usage]) => usage && [usage.inputTokens, usage.outputTokens, usage.cacheReadInputTokens ?? 0, usage.cacheCreationInputTokens ?? 0])
      : u ? [[u.input_tokens, u.output_tokens, u.cache_read_input_tokens ?? 0, u.cache_creation_input_tokens ?? 0]] : [];
    if (!entries.length || entries.some(values => !values || !values.every(count))) return;
    const [uncached, outputTokens, cacheReadTokens, cacheWriteTokens] = entries.reduce((sum, values) => sum.map((value, index) => value + values[index]), [0, 0, 0, 0]);
    const inputTokens = uncached + cacheReadTokens + cacheWriteTokens;
    run.usage = { inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, totalTokens: inputTokens + outputTokens, costUsd: amount(event.total_cost_usd) ? event.total_cost_usd : null, source: 'cli', scope: models.length ? 'all-agents' : 'main-agent', models: models.map(([model]) => model) };
  }
}
