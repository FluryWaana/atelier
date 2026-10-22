import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { existsSync, realpathSync, statSync, accessSync, constants } from 'node:fs';
import { join, isAbsolute, delimiter, extname, dirname } from 'node:path';
import { readAgentCredentials } from './agent-credentials.mjs';
import { agentOptions } from './domain/agent-options.mjs';
import { releaseVersion, versionState } from './domain/releases.mjs';
export { versionState } from './domain/releases.mjs';

const exec = promisify(execFile);
export function executable(agent, command = '', environment = process.env) {
  if (command) return resolveExecutable(command, environment);
  if (!['codex', 'claude'].includes(agent)) throw new Error('Agent inconnu.');
  const home = process.env.USERPROFILE || process.env.HOME || '';
  const candidates = process.platform !== 'win32' ? [join(home, '.local/bin', agent)] : agent === 'codex'
    ? [join(process.env.LOCALAPPDATA || '', 'Programs/OpenAI/Codex/bin/codex.exe')]
    : [join(home, '.local/bin/claude.exe')];
  return resolveExecutable(candidates.find(existsSync) || agent, environment);
}

export function resolveExecutable(command, environment = process.env) {
  const path = Object.entries(environment).find(([key]) => key.toUpperCase() === 'PATH')?.[1] || '';
  const candidates = isAbsolute(command) ? [command] : path.split(delimiter).filter(directory => directory && isAbsolute(directory)).flatMap(directory => process.platform === 'win32' && !extname(command) ? [join(directory, command + '.exe')] : [join(directory, command)]);
  for (const candidate of candidates) {
    if (process.platform === 'win32' && (!/^[a-zA-Z]:[\\/]/.test(candidate) || !/\.(exe|com)$/i.test(candidate))) continue;
    try {
      const resolved = realpathSync(candidate);
      if (process.platform === 'win32' && !/^[a-zA-Z]:[\\/]/.test(resolved)) continue;
      if (!statSync(resolved).isFile()) continue;
      accessSync(resolved, constants.X_OK);
      return resolved;
    } catch { /* Try the next absolute PATH entry. */ }
  }
  throw new Error('Exécutable local introuvable. Renseignez son chemin absolu.');
}

export function commandArgs(agent, mode, options = {}) {
  if (!['analyze', 'edit'].includes(mode)) throw new Error('Mode inconnu.');
  const { model, effort } = agentOptions(agent, options);
  const modelArgs = model ? ['--model', model] : [];
  if (agent === 'codex') return ['-a', 'never', 'exec', '--json', '--color', 'never', '--skip-git-repo-check', ...modelArgs, ...(effort ? ['-c', `model_reasoning_effort="${effort}"`] : []), '-s', mode === 'edit' ? 'workspace-write' : 'read-only', '-'];
  if (agent === 'claude') return ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', ...modelArgs, ...(effort ? ['--effort', effort] : []), '--permission-mode', mode === 'edit' ? 'acceptEdits' : 'plan', '--permission-prompts', 'none', '--allowedTools', 'Bash(git status:*)', 'Bash(git branch --show-current)'];
  throw new Error('Agent inconnu.');
}

export function agentEnvironment(provider, apiKey = '', environment = process.env) {
  const env = {};
  const allowed = /^(PATH|PATHEXT|SYSTEMROOT|WINDIR|COMSPEC|SYSTEMDRIVE|HOME|USERPROFILE|APPDATA|LOCALAPPDATA|PROGRAMDATA|PROGRAMFILES|PROGRAMFILES\(X86\)|TEMP|TMP|TMPDIR|LANG|LC_[A-Z_]+|TERM|COLORTERM)$/i;
  for (const [key, value] of Object.entries(environment)) if (allowed.test(key)) env[key] = value;
  const credentials = { codex: ['CODEX_API_KEY', 'OPENAI_API_KEY', 'CODEX_ACCESS_TOKEN'], claude: ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN'] };
  if (provider.auth !== 'api' && !provider.configDir) for (const key of credentials[provider.type] || []) if (environment[key]) env[key] = environment[key];
  const homeKey = provider.type === 'codex' ? 'CODEX_HOME' : provider.type === 'claude' ? 'CLAUDE_CONFIG_DIR' : '';
  if (homeKey && environment[homeKey]) env[homeKey] = environment[homeKey];
  if (provider.configDir && provider.type !== 'command') env[provider.type === 'codex' ? 'CODEX_HOME' : 'CLAUDE_CONFIG_DIR'] = provider.configDir;
  if (provider.auth === 'api' && apiKey) env.CODEX_API_KEY = apiKey;
  return env;
}

export function redactSecrets(secrets) {
  const values = [...new Set(secrets.filter(Boolean).flatMap(secret => [secret, JSON.stringify(secret).slice(1, -1)]))].sort((a, b) => b.length - a.length);
  return value => values.reduce((text, secret) => text.replaceAll(secret, '[secret]'), value);
}

export function createVersionChecker(request = fetch) {
  const cache = new Map();
  return async provider => {
    const name = provider.type === 'codex' ? '@openai/codex' : provider.type === 'claude' ? '@anthropic-ai/claude-code' : provider.npmPackage;
    if (!name) return '';
    const previous = cache.get(name);
    if (previous && Date.now() - previous.time < 60 * 60 * 1000) return previous.value;
    const value = (async () => {
      try {
        const response = await request('https://registry.npmjs.org/' + encodeURIComponent(name) + '/latest', { signal: AbortSignal.timeout(5000), redirect: 'error' });
        if (!response.ok) { await response.body?.cancel(); return ''; }
        const chunks = []; let size = 0;
        for await (const chunk of response.body) {
          size += chunk.length;
          if (size > 65536) return '';
          chunks.push(chunk);
        }
        return releaseVersion(JSON.parse(Buffer.concat(chunks).toString('utf8')).version);
      } catch { return ''; }
    })();
    cache.set(name, { time: Date.now(), value });
    value.then(version => { if (!version) cache.delete(name); });
    return value;
  };
}
const latestVersion = createVersionChecker();

export async function agentStatus(agent, provider = { type: agent, auth: 'cli' }, apiKey = '', execute = exec, latest = latestVersion) {
  const published = await latest(provider);
  const base = { latestVersion: published, updateState: 'unknown', account: '', authMethod: provider.auth || 'cli' };
  try {
    const env = agentEnvironment(provider, apiKey), binary = executable(provider.type, provider.command, env);
    const redact = redactSecrets([apiKey, env.CODEX_API_KEY, env.OPENAI_API_KEY, env.CODEX_ACCESS_TOKEN, env.ANTHROPIC_API_KEY, env.ANTHROPIC_AUTH_TOKEN, env.CLAUDE_CODE_OAUTH_TOKEN]);
    const options = { windowsHide: true, timeout: 10000, maxBuffer: 65536, cwd: dirname(binary), env };
    const { stdout } = await execute(binary, provider.type === 'command' ? provider.versionArgs : ['--version'], { ...options, env: agentEnvironment({ type: 'command' }, '', env) });
    const version = redact(stdout).trim().slice(0, 200);
    let connected = false, account = '', subscription = '', authMethod = provider.auth || 'cli';
    if (provider.auth === 'api' || provider.type === 'codex' && env.CODEX_API_KEY) return { ...base, authMethod: 'api', installed: true, connected: false, configured: Boolean(env.CODEX_API_KEY), version, updateState: versionState(version, published) };
    try {
      const args = provider.type === 'codex' ? ['login', 'status'] : provider.type === 'claude' ? ['auth', 'status'] : provider.statusArgs;
      if (args?.length) {
        const result = await execute(binary, args, options);
        if (provider.type === 'codex') {
          connected = /Logged in/i.test(result.stdout + result.stderr);
          authMethod = /API key/i.test(result.stdout + result.stderr) ? 'api' : 'ChatGPT';
          if (connected && authMethod === 'ChatGPT') {
            // Read only the public identity claim; never expose the cached tokens.
            try {
              const auth = await readAgentCredentials(env.CODEX_HOME || join(env.USERPROFILE || env.HOME || '', '.codex'), 'auth.json');
              const claims = JSON.parse(Buffer.from(auth.tokens?.id_token?.split('.')[1] || '', 'base64url').toString('utf8'));
              if (typeof claims.email === 'string') account = redact(claims.email).slice(0, 200);
            } catch { /* Keyring-backed authentication may not expose an identity. */ }
          }
        } else {
          const status = JSON.parse(result.stdout);
          connected = status.loggedIn === true;
          if (typeof status.email === 'string') account = redact(status.email).slice(0, 200);
          if (typeof status.authMethod === 'string') authMethod = redact(status.authMethod).slice(0, 100);
          if (typeof status.subscriptionType === 'string') subscription = redact(status.subscriptionType).slice(0, 80);
        }
      }
    } catch { /* Installed but authentication is unavailable. */ }
    return { ...base, installed: true, connected, version, account: redact(account), subscription, authMethod: redact(authMethod), updateState: versionState(version, published) };
  } catch { return { ...base, installed: false, connected: false, version: '' }; }
}

export function startAgent({ agent, mode, repo, prompt, model, effort, provider = { type: agent, auth: 'cli' }, apiKey = '' }, onLine, onEnd, spawnProcess = spawn) {
  if (!['analyze', 'edit'].includes(mode)) throw new Error('Mode inconnu.');
  if (provider.auth === 'api' && !apiKey) throw new Error('Renseignez la clé API de ce fournisseur.');
  const env = agentEnvironment(provider, apiKey);
  const args = provider.type === 'command' ? provider[mode + 'Args'].map(arg => arg.replaceAll('{model}', model || '').replaceAll('{effort}', effort || '')) : commandArgs(provider.type, mode, { model, effort });
  const child = spawnProcess(executable(provider.type, provider.command, env), args, { cwd: repo, shell: false, detached: process.platform !== 'win32', windowsHide: true, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let ended = false;
  let output = '';
  const secrets = [apiKey, env.CODEX_API_KEY, env.OPENAI_API_KEY, env.CODEX_ACCESS_TOKEN, env.ANTHROPIC_API_KEY, env.ANTHROPIC_AUTH_TOKEN, env.CLAUDE_CODE_OAUTH_TOKEN].filter(Boolean);
  const redact = redactSecrets(secrets);
  const emit = (stream, line) => {
    line = redact(line);
    // shortcut: custom CLIs return plain text; add an adapter when structured progress or usage is needed.
    if (provider.type === 'command' && stream === 'stdout') output = (output + line + '\n').slice(-100000);
    onLine(stream, line);
  };
  const finish = (code, error) => { if (!ended) { ended = true; clearTimeout(timer); if (provider.type === 'command') onLine('stdout', JSON.stringify({ type: 'result', result: output.trim() })); onEnd(code, error ? redact(error) : error); } };
  const timer = setTimeout(() => { onLine('system', 'Durée maximale de 60 minutes atteinte.'); stop(); }, 60 * 60 * 1000);
  timer.unref();
  for (const [name, stream] of [['stdout', child.stdout], ['stderr', child.stderr]]) {
    let buffer = '', dropping = false;
    const limit = 1024 * 1024;
    const oversized = () => onLine('system', 'Ligne de sortie trop longue : contenu masqué.');
    stream.setEncoding('utf8');
    stream.on('data', chunk => {
      if (dropping) {
        const end = chunk.indexOf('\n');
        if (end < 0) return;
        dropping = false; chunk = chunk.slice(end + 1);
      }
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) { if (newline > limit) oversized(); else emit(name, buffer.slice(0, newline)); buffer = buffer.slice(newline + 1); }
      // Never publish a partial line: a credential can straddle any chunk boundary.
      if (buffer.length > limit) { oversized(); buffer = ''; dropping = true; }
    });
    stream.on('end', () => { if (buffer) emit(name, buffer); });
  }
  child.once('error', error => finish(1, error.message));
  child.once('close', code => finish(code ?? 1));
  child.stdin.on('error', () => {});
  child.stdin.end(prompt);
  function stop() {
    if (ended) return;
    if (process.platform === 'win32' && child.pid) {
      execFile('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true }, error => { if (error) child.kill(); });
    } else if (child.pid) {
      try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    } else child.kill('SIGTERM');
  }
  return { stop };
}

export function eventText(line) {
  try {
    const e = JSON.parse(line);
    if (e.item?.type === 'reasoning') return '';
    if (e.type === 'assistant') return (e.message?.content || []).map(c => c.text || (c.type === 'tool_use' ? `→ ${c.name}` : '')).filter(Boolean).join('\n');
    if (e.type === 'result') {
      const result = typeof e.result === 'string' ? e.result : e.errors?.join('\n') || '';
      const denials = Array.isArray(e.permission_denials) ? e.permission_denials.map(denial => `Permission refusée : ${denial?.tool_name || 'outil inconnu'} · ${JSON.stringify(denial?.tool_input ?? {}).slice(0, 2000)}`) : [];
      return [result, ...denials].filter(Boolean).join('\n');
    }
    if (e.item) return [e.item.text, e.item.aggregated_output, e.item.command, e.item.type].find(value => typeof value === 'string' && value) || '';
    if (e.type === 'error' || e.type === 'turn.failed') return [e.message, e.error?.message].find(value => typeof value === 'string' && value) || line;
    if (e.type === 'system') return e.subtype === 'init' ? `Session Claude · ${e.model || ''}` : '';
    return '';
  } catch { return line; }
}

// Only public messages and tool activity are shown; private reasoning is ignored.
export function eventProgress(line) {
  try {
    const e = JSON.parse(line);
    const tool = e.message?.content?.find(c => c.type === 'tool_use');
    if (e.type === 'stream_event') {
      if (e.event?.delta?.type === 'text_delta' && typeof e.event.delta.text === 'string') return { activity: 'Rédaction de la réponse', delta: e.event.delta.text };
      if (e.event?.type === 'content_block_start' && e.event.content_block?.type === 'tool_use') return { activity: `Outil : ${e.event.content_block.name}` };
      return null;
    }
    if (tool) return { activity: `Outil : ${tool.name}${tool.input?.command || tool.input?.file_path ? ' · ' + (tool.input.command || tool.input.file_path) : ''}`, clearPreview: true };
    if (e.type === 'assistant') return { activity: 'Message reçu', clearPreview: true };
    if (e.type === 'user' && e.message?.content?.some(c => c.type === 'tool_result')) return { activity: 'Résultat d’outil reçu' };
    if (e.item?.type === 'command_execution') return { activity: `${e.type === 'item.completed' ? 'Commande terminée' : 'Commande en cours'} : ${e.item.command || ''}` };
    if (e.item?.type === 'file_change') return { activity: `Modification de fichiers : ${(e.item.changes || []).map(c => c.path).join(', ')}` };
    if (e.item?.type === 'agent_message') return { activity: 'Message reçu' };
    if (e.type === 'system' && e.subtype === 'api_retry') return { activity: `Nouvelle tentative de connexion (${e.attempt})` };
    if (e.type === 'thread.started' || e.type === 'system' && e.subtype === 'init') return { activity: 'Agent connecté · préparation' };
    if (e.type === 'turn.started') return { activity: 'Travail en cours' };
    if (e.type === 'turn.completed' || e.type === 'result') return { activity: 'Finalisation de l’exécution', clearPreview: true };
    return null;
  } catch { return null; }
}
