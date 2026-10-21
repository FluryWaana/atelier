import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { agentOptions } from './domain/agent-options.mjs';

const exec = promisify(execFile);
export function executable(agent) {
  if (!['codex', 'claude'].includes(agent)) throw new Error('Agent inconnu.');
  const home = process.env.USERPROFILE || process.env.HOME || '';
  const candidates = process.platform !== 'win32' ? [join(home, '.local/bin', agent)] : agent === 'codex'
    ? [join(process.env.LOCALAPPDATA || '', 'Programs/OpenAI/Codex/bin/codex.exe')]
    : [join(home, '.local/bin/claude.exe')];
  return candidates.find(existsSync) || agent;
}

export function commandArgs(agent, mode, options = {}) {
  if (!['analyze', 'edit'].includes(mode)) throw new Error('Mode inconnu.');
  const { model, effort } = agentOptions(agent, options);
  const modelArgs = model ? ['--model', model] : [];
  if (agent === 'codex') return ['-a', 'never', 'exec', '--json', '--color', 'never', '--skip-git-repo-check', ...modelArgs, ...(effort ? ['-c', `model_reasoning_effort="${effort}"`] : []), '-s', mode === 'edit' ? 'workspace-write' : 'read-only', '-'];
  if (agent === 'claude') return ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', ...modelArgs, ...(effort ? ['--effort', effort] : []), '--permission-mode', mode === 'edit' ? 'acceptEdits' : 'plan', '--permission-prompts', 'none'];
  throw new Error('Agent inconnu.');
}

export async function agentStatus(agent) {
  try {
    const binary = executable(agent);
    const { stdout: version } = await exec(binary, ['--version'], { windowsHide: true, timeout: 10000 });
    let connected = false;
    try {
      const result = await exec(binary, agent === 'codex' ? ['login', 'status'] : ['auth', 'status'], { windowsHide: true, timeout: 10000 });
      connected = agent === 'codex' ? /Logged in/i.test(result.stdout + result.stderr) : JSON.parse(result.stdout).loggedIn === true;
    } catch { /* Installed but authentication is unavailable. */ }
    return { installed: true, connected, version: version.trim() };
  } catch { return { installed: false, connected: false, version: '' }; }
}

export function startAgent({ agent, mode, repo, prompt, model, effort }, onLine, onEnd, spawnProcess = spawn) {
  const env = { ...process.env };
  delete env.CLAUDECODE;
  delete env.JIRA_TOKEN;
  delete env.AZURE_DEVOPS_PAT;
  delete env.TEMPO_TOKEN;
  const child = spawnProcess(executable(agent), commandArgs(agent, mode, { model, effort }), { cwd: repo, shell: false, detached: process.platform !== 'win32', windowsHide: true, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let ended = false;
  const finish = (code, error) => { if (!ended) { ended = true; clearTimeout(timer); onEnd(code, error); } };
  const timer = setTimeout(() => { onLine('system', 'Durée maximale de 60 minutes atteinte.'); stop(); }, 60 * 60 * 1000);
  timer.unref();
  for (const [name, stream] of [['stdout', child.stdout], ['stderr', child.stderr]]) {
    let buffer = '';
    stream.setEncoding('utf8');
    stream.on('data', chunk => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) { onLine(name, buffer.slice(0, newline)); buffer = buffer.slice(newline + 1); }
      if (buffer.length > 65536) { onLine(name, buffer); buffer = ''; }
    });
    stream.on('end', () => { if (buffer) onLine(name, buffer); });
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
    if (e.type === 'result') return e.result || e.errors?.join('\n') || '';
    if (e.item) return e.item.text || e.item.aggregated_output || e.item.command || e.item.type || '';
    if (e.type === 'error' || e.type === 'turn.failed') return e.message || e.error?.message || line;
    if (e.type === 'system') return e.subtype === 'init' ? `Session Claude · ${e.model || ''}` : '';
    return '';
  } catch { return line; }
}

// Only public messages and tool activity are shown; private reasoning is ignored.
export function eventProgress(line) {
  let e;
  try { e = JSON.parse(line); } catch { return null; }
  const tool = e.message?.content?.find(c => c.type === 'tool_use');
  if (e.type === 'stream_event') {
    if (e.event?.delta?.type === 'text_delta') return { activity: 'Rédaction de la réponse', delta: e.event.delta.text };
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
}
