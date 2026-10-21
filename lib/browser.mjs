import { spawn } from 'node:child_process';
import { join } from 'node:path';

export function openBrowser(address, platform = process.platform, spawnProcess = spawn) {
  const windows = platform === 'win32';
  const command = windows ? join(process.env.SystemRoot || 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe') : platform === 'darwin' ? 'open' : 'xdg-open';
  const args = windows
    ? ['-NoProfile', '-NonInteractive', '-Command', '$ErrorActionPreference = "Stop"; Start-Process -FilePath $env:ATELIER_BROWSER_URL -WindowStyle Hidden']
    : [address];
  return new Promise((resolve, reject) => {
    const fail = () => reject(new Error('Impossible d’ouvrir le navigateur. Vérifie le navigateur par défaut puis relance Atelier.'));
    // Pass the session URL as data, never as PowerShell source or in its command line.
    const child = spawnProcess(command, args, { windowsHide: true, detached: !windows, stdio: 'ignore', ...(windows ? { timeout: 15000, env: { ...process.env, ATELIER_BROWSER_URL: address } } : {}) });
    child.once('error', fail);
    if (windows) child.once('close', code => code === 0 ? resolve() : fail());
    else child.once('spawn', () => { child.unref(); resolve(); });
  });
}
