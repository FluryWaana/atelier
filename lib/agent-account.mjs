import { spawn } from 'node:child_process';
import { dirname } from 'node:path';
import { agentEnvironment, executable, redactSecrets } from './agents.mjs';
import { claudeAccount } from './claude-account.mjs';

export function quotaWindows(result, redact = value => value) {
  const buckets = result?.rateLimitsByLimitId && typeof result.rateLimitsByLimitId === 'object'
    ? Object.values(result.rateLimitsByLimitId).slice(0, 20) : [result?.rateLimits];
  return buckets.flatMap(bucket => ['primary', 'secondary'].flatMap(kind => {
    const value = bucket?.[kind];
    if (!value || !Number.isFinite(value.usedPercent) || value.usedPercent < 0 || value.usedPercent > 100) return [];
    const name = typeof bucket.limitName === 'string' ? bucket.limitName : typeof bucket.limitId === 'string' ? bucket.limitId : 'Codex';
    return [{ name: redact(name).slice(0, 100), kind, remainingPercent: Math.round((100 - value.usedPercent) * 10) / 10,
      windowMinutes: Number.isSafeInteger(value.windowDurationMins) && value.windowDurationMins > 0 ? value.windowDurationMins : null,
      resetsAt: Number.isSafeInteger(value.resetsAt) && value.resetsAt > 0 && value.resetsAt < 253402300800 ? new Date(value.resetsAt * 1000).toISOString() : null }];
  }));
}

// Only account reads: never create a thread, start a turn, log in or redeem credits.
export async function agentAccount(provider, spawnProcess = spawn, timeoutMs = 10000) {
  if (provider.type === 'claude') return claudeAccount(provider);
  const unavailable = { subscription: '', quotas: [], quotaState: provider.auth === 'api' ? 'api' : 'unsupported' };
  if (provider.type !== 'codex' || provider.auth === 'api') return unavailable;
  let child;
  try {
    const env = agentEnvironment(provider), binary = executable(provider.type, provider.command, env);
    const redact = redactSecrets([env.CODEX_API_KEY, env.OPENAI_API_KEY, env.CODEX_ACCESS_TOKEN]);
    child = spawnProcess(binary, ['app-server'], { cwd: dirname(binary), env, shell: false, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    return await new Promise(resolve => {
      let done = false, buffer = '', bytes = 0, expected = 1;
      let result = { ...unavailable, quotaState: 'unavailable' };
      const finish = () => {
        if (done) return;
        done = true; clearTimeout(timer); child.stdin.end(); child.kill(); resolve(result);
      };
      const timer = setTimeout(finish, timeoutMs);
      const send = message => { if (!done) child.stdin.write(JSON.stringify(message) + '\n'); };
      child.on('error', finish); child.on('close', finish); child.stdin.on('error', finish);
      child.stderr.on('data', chunk => { bytes += chunk.length; if (bytes > 1024 * 1024) finish(); });
      child.stdout.setEncoding('utf8');
      child.stdout.on('data', chunk => {
        if (done) return;
        bytes += Buffer.byteLength(chunk);
        if (bytes > 1024 * 1024) return finish();
        buffer += chunk;
        let newline;
        while (!done && (newline = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
          let message;
          try { message = JSON.parse(line); } catch { return finish(); }
          if (!message || typeof message !== 'object') return finish();
          if (message.method) {
            if (message.id !== undefined) send({ id: message.id, error: { code: -32601, message: 'Read-only client' } });
            continue;
          }
          if (message.id !== expected) continue;
          if (message.error) return finish();
          if (expected === 1) {
            expected = 2;
            send({ method: 'initialized', params: {} });
            send({ id: 2, method: 'account/read', params: { refreshToken: false } });
          } else if (expected === 2) {
            const account = message.result?.account;
            if (account?.type !== 'chatgpt') { result.quotaState = account?.type === 'apiKey' ? 'api' : 'disconnected'; return finish(); }
            result.account = typeof account.email === 'string' ? redact(account.email).slice(0, 200) : '';
            result.subscription = typeof account.planType === 'string' ? redact(account.planType).slice(0, 80) : '';
            expected = 3; send({ id: 3, method: 'account/rateLimits/read' });
          } else {
            result.quotas = quotaWindows(message.result, redact);
            result.quotaState = result.quotas.length ? 'available' : 'unavailable';
            return finish();
          }
        }
      });
      send({ id: 1, method: 'initialize', params: { clientInfo: { name: 'atelier_kanban', title: 'Atelier Kanban', version: '0.1.3' } } });
    });
  } catch { child?.kill(); return { ...unavailable, quotaState: 'unavailable' }; }
}
