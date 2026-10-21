import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

export function startWindowsTray(root, port) {
  const binary = join(root, 'scripts', 'WindowsTray.exe');
  if (process.platform !== 'win32' || !existsSync(binary)) return async () => {};
  const child = spawn(binary, [], { cwd: root, windowsHide: true, stdio: ['pipe', 'ignore', 'inherit'], env: { ...process.env, PORT: String(port) } });
  child.stdin.on('error', () => {});
  const closed = new Promise(resolve => {
    child.once('error', error => { console.error('Icône Atelier :', error.message); resolve(); });
    child.once('close', resolve);
  });
  return async () => {
    child.stdin.end();
    const timeout = setTimeout(() => child.kill(), 5000);
    try { await closed; } finally { clearTimeout(timeout); }
  };
}
