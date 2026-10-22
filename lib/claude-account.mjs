import { homedir } from 'node:os';
import { join } from 'node:path';
import { readAgentCredentials } from './agent-credentials.mjs';
import { createHash } from 'node:crypto';
import { agentEnvironment } from './agents.mjs';

const tokenValid = value => typeof value === 'string' && value.length > 0 && value.length <= 16384 && /^[\x21-\x7e]+$/.test(value);

export async function claudeCredential(provider, environment = process.env) {
  const env = agentEnvironment(provider, '', environment);
  if (provider.auth === 'api' || env.ANTHROPIC_API_KEY || env.ANTHROPIC_AUTH_TOKEN) return { state: 'api' };
  if (env.CLAUDE_CODE_OAUTH_TOKEN) return tokenValid(env.CLAUDE_CODE_OAUTH_TOKEN) ? { token: env.CLAUDE_CODE_OAUTH_TOKEN } : { state: 'disconnected' };
  const directory = env.CLAUDE_CONFIG_DIR || join(env.USERPROFILE || env.HOME || homedir(), '.claude');
  try {
    const credentials = await readAgentCredentials(directory, '.credentials.json');
    if (!credentials) return { state: 'unavailable' };
    const oauth = credentials.claudeAiOauth;
    if (!tokenValid(oauth?.accessToken)) return { state: 'disconnected' };
    // Only the access token is used; refreshing shared credentials can disconnect Claude Code.
    return { token: oauth.accessToken, expiresAt: oauth.expiresAt };
  } catch { return { state: 'disconnected' }; }
}

export function claudeQuotaWindows(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
  return Object.entries(value).filter(([key]) => /^(five_hour|seven_day)(?:_[a-z0-9_]{1,40})?$/.test(key)).slice(0, 20).flatMap(([key, quota]) => {
    if (!quota || !Number.isFinite(quota.utilization) || quota.utilization < 0 || quota.utilization > 100) return [];
    const date = typeof quota.resets_at === 'string' && quota.resets_at.length <= 40 ? Date.parse(quota.resets_at) : NaN;
    const suffix = key.replace(/^(five_hour|seven_day)_?/, '');
    return [{ name: suffix ? suffix.replaceAll('_', ' ') : 'Claude', kind: key.startsWith('five_hour') ? 'primary' : 'secondary',
      remainingPercent: Math.round((100 - quota.utilization) * 10) / 10,
      windowMinutes: key.startsWith('five_hour') ? 300 : 10080, resetsAt: Number.isFinite(date) ? new Date(date).toISOString() : null }];
  });
}

export function createClaudeAccountReader({ request = fetch, readCredential = claudeCredential, now = Date.now } = {}) {
  const cache = new Map();
  return async provider => {
    const empty = { quotas: [], quotaState: 'unavailable' };
    let credential;
    try { credential = await readCredential(provider); } catch { return empty; }
    if (credential.state) return { ...empty, quotaState: credential.state };
    if (!tokenValid(credential.token) || credential.expiresAt !== undefined && (!Number.isFinite(credential.expiresAt) || credential.expiresAt <= now())) return { ...empty, quotaState: 'disconnected' };
    // Deduplicate aliases of the same login without retaining plaintext credentials in the cache.
    const key = createHash('sha256').update(credential.token).digest('hex');
    const cached = cache.get(key);
    if (cached && cached.until > now()) return cached.value;
    for (const [id, entry] of cache) if (entry.until <= now()) cache.delete(id);
    if (cache.size >= 100) return empty;
    const entry = { until: now() + 300000 };
    entry.value = (async () => {
      try {
        // shortcut: Anthropic's usage endpoint is undocumented; revisit if its contract changes.
        const response = await request('https://api.anthropic.com/api/oauth/usage', {
          method: 'GET', redirect: 'error', signal: AbortSignal.timeout(10000),
          headers: { Authorization: 'Bearer ' + credential.token, 'anthropic-beta': 'oauth-2025-04-20', Accept: 'application/json' },
        });
        if (!response.ok) {
          await response.body?.cancel();
          if (response.status === 429) {
            const retry = response.headers.get('retry-after');
            const until = retry && /^\d+$/.test(retry) ? now() + Number(retry) * 1000 : Date.parse(retry);
            entry.until = Number.isFinite(until) && until > now() && until < 8640000000000000 ? Math.max(entry.until, until) : now() + 900000;
            return { ...empty, quotaState: 'rate-limited', retryAt: new Date(entry.until).toISOString() };
          }
          return { ...empty, quotaState: response.status === 401 || response.status === 403 ? 'disconnected' : 'unavailable' };
        }
        const chunks = []; let size = 0;
        for await (const chunk of response.body) {
          size += chunk.length;
          if (size > 65536) throw new Error('Usage response too large');
          chunks.push(chunk);
        }
        const quotas = claudeQuotaWindows(JSON.parse(Buffer.concat(chunks).toString('utf8')));
        return { quotas, quotaState: quotas.length ? 'available' : 'unavailable', quotaCheckedAt: new Date(now()).toISOString() };
      } catch { return empty; }
    })();
    cache.set(key, entry);
    return entry.value;
  };
}

export const claudeAccount = createClaudeAccountReader();
