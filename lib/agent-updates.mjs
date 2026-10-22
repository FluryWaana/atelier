import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { dirname, join, relative, isAbsolute, sep } from 'node:path';
import { agentEnvironment, executable } from './agents.mjs';

const exec = promisify(execFile);
export function agentUpdateInfo(provider, resolve = executable, environment = process.env) {
  if (!['codex', 'claude'].includes(provider.type) || provider.command) return { canUpdate: false, updateReason: 'custom' };
  try {
    const binary = resolve(provider.type), home = environment.USERPROFILE || environment.HOME;
    if (!home || !isAbsolute(home)) return { canUpdate: false, updateReason: 'managed' };
    const roots = [join(home, '.local', 'bin'), join(home, '.local', 'share', provider.type)];
    const native = roots.some(root => {
      const path = relative(root, binary);
      return path && !isAbsolute(path) && path !== '..' && !path.startsWith('..' + sep);
    });
    return { canUpdate: Boolean(native), updateReason: native ? '' : 'managed' };
  } catch { return { canUpdate: false, updateReason: 'missing' }; }
}

export async function updateAgent(provider, execute = exec, resolve = executable, environment = process.env) {
  const binary = resolve(provider.type);
  if (!agentUpdateInfo(provider, () => binary, environment).canUpdate) throw new Error('Cette installation doit être mise à jour avec son gestionnaire habituel.');
  const env = agentEnvironment({ type: 'command' }, '', environment);
  const homeKey = provider.type === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
  if (provider.configDir || environment[homeKey]) env[homeKey] = provider.configDir || environment[homeKey];
  for (const key of ['DISABLE_UPDATES', 'DISABLE_AUTOUPDATER']) if (environment[key]) env[key] = environment[key];
  try {
    await execute(binary, ['update'], { cwd: dirname(binary), env, shell: false, windowsHide: true, timeout: 180000, killSignal: 'SIGKILL', maxBuffer: 1024 * 1024 });
  } catch {
    // CLI output can include private configuration; do not return raw stdout, stderr or spawn errors.
    throw new Error('La mise à jour du CLI a échoué ou dépassé trois minutes. Vérifiez les droits, la connexion et le mode d’installation, puis réessayez.');
  }
}
