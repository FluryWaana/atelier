import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

export function startTray(root, port, { platform = process.platform, env = process.env, spawnProcess = spawn } = {}) {
  const name = { win32: 'WindowsTray.exe', darwin: 'MacTray', linux: 'linux-tray.py' }[platform];
  if (!name || platform === 'linux' && (!(env.DISPLAY || env.WAYLAND_DISPLAY) || !env.DBUS_SESSION_BUS_ADDRESS)) return async () => {};
  const binary = join(root, 'scripts', name);
  if (!existsSync(binary)) return async () => {};
  const child = spawnProcess(platform === 'linux' ? '/usr/bin/python3' : binary, platform === 'linux' ? ['-B', binary] : [], { cwd: root, windowsHide: true, stdio: ['pipe', 'ignore', 'inherit'], env: { ...env, PORT: String(port) } });
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
