import { spawn, execFile } from 'node:child_process';

export function runVerification(command, cwd, { signal, onOutput = () => {} }) {
  signal.throwIfAborted();
  return new Promise((resolve, reject) => {
    const env = { ...process.env };
    for (const key of ['JIRA_TOKEN', 'AZURE_DEVOPS_PAT', 'TEMPO_TOKEN']) delete env[key];
    const child = process.platform === 'win32'
      ? spawn(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', `"${command}"`], { cwd, env, windowsHide: true, windowsVerbatimArguments: true, stdio: ['ignore', 'pipe', 'pipe'] })
      : spawn('/bin/sh', ['-c', command], { cwd, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let failure;
    const stop = () => {
      if (!child.pid) return;
      if (process.platform === 'win32') execFile('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true, timeout: 5000 }, error => { if (error) child.kill(); });
      else { try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') failure = error; } }
    };
    const timer = setTimeout(() => { failure = new Error('Vérification interrompue après 10 minutes.'); stop(); }, 10 * 60 * 1000);
    signal.addEventListener('abort', stop, { once: true });
    for (const stream of [child.stdout, child.stderr]) { stream.setEncoding('utf8'); stream.on('data', onOutput); }
    child.once('error', error => { failure = error; });
    child.once('close', code => {
      clearTimeout(timer); signal.removeEventListener('abort', stop);
      if (signal.aborted) reject(new Error('Parcours arrêté.'));
      else if (failure || code !== 0) reject(failure || new Error(`Échec des vérifications (code ${code}).`));
      else resolve();
    });
  });
}
