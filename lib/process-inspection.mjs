import { execFile } from 'node:child_process';
import { isIP } from 'node:net';
import { promisify } from 'node:util';

const exec = promisify(execFile);
export function tcpListeners(output) {
  const listeners = new Map();
  for (const line of output.split(/\r?\n/)) {
    const match = line.match(/^\s*TCP\s+(\S+)\s+\S+\s+(?:LISTENING|LISTEN|ÉCOUTE|ECOUTE)\s+(\d+)\s*$/i);
    if (!match) continue;
    const separator = match[1].lastIndexOf(':');
    const address = match[1].slice(0, separator).replace(/^\[|\]$/g, '');
    const port = Number(match[1].slice(separator + 1)), pid = Number(match[2]);
    if (!isIP(address) || !Number.isInteger(port) || port < 1 || port > 65535 || pid < 1) continue;
    listeners.set(`${pid}/${address}/${port}`, { pid, address, port });
  }
  return [...listeners.values()];
}

export function descendantPids(pid, processes) {
  const ids = new Set([pid]);
  let added;
  do {
    added = false;
    for (const process of processes) if (ids.has(process.ParentProcessId) && !ids.has(process.ProcessId)) { ids.add(process.ProcessId); added = true; }
  } while (added);
  return [...ids];
}

export function listenerUrl(listener, output = '') {
  const address = listener.address === '0.0.0.0' ? '127.0.0.1' : listener.address === '::' ? '::1' : listener.address;
  const host = isIP(address) === 6 ? `[${address}]` : address;
  const cleanOutput = output.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');
  for (const match of cleanOutput.matchAll(/https?:\/\/[^\s<>"'()]+/g)) {
    try {
      const url = new URL(match[0]);
      const hostname = url.hostname.replace(/^\[|\]$/g, '');
      if (Number(url.port || (url.protocol === 'https:' ? 443 : 80)) === listener.port && ['localhost', address, listener.address].includes(hostname) && !url.username && !url.password) return `${url.protocol}//${host}:${listener.port}${url.pathname}${url.search}${url.hash}`;
    } catch { /* Ignore incomplete URLs in streamed output. */ }
  }
  return `http://${host}:${listener.port}/`;
}

export function posixProcesses(output) {
  return output.split(/\r?\n/).flatMap(line => {
    const match = line.match(/^\s*(\d+)\s+(\d+)\s*$/);
    return match ? [{ ProcessId: Number(match[1]), ParentProcessId: Number(match[2]) }] : [];
  });
}

export function lsofListeners(output) {
  let pid;
  const listeners = [];
  for (const line of output.split(/\r?\n/)) {
    if (/^p\d+$/.test(line)) pid = Number(line.slice(1));
    if (!pid || !line.startsWith('n')) continue;
    const addressPort = line.slice(1), separator = addressPort.lastIndexOf(':');
    const address = addressPort.slice(0, separator).replace(/^\[|\]$/g, ''), port = Number(addressPort.slice(separator + 1));
    if ((isIP(address) || address === '*') && Number.isInteger(port) && port > 0 && port <= 65535) listeners.push({ pid, address: address === '*' ? '0.0.0.0' : address, port });
  }
  return listeners;
}

export async function inspectProcesses(platform = process.platform, execute = exec) {
  const options = { encoding: 'utf8', windowsHide: true, timeout: 6000, maxBuffer: 4 * 1024 * 1024 };
  if (platform !== 'win32') {
    const [processes, sockets] = await Promise.all([
      execute('ps', ['-axo', 'pid=,ppid='], options),
      execute('lsof', ['-nP', '-iTCP', '-sTCP:LISTEN', '-Fpn'], options).catch(error => {
        if (error.code === 1 && !error.stdout && !error.stderr) return { stdout: '' };
        throw error;
      }),
    ]);
    return { processes: posixProcesses(processes.stdout), listeners: lsofListeners(sockets.stdout) };
  }
  const [processes, sockets] = await Promise.all([
    execute('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', 'Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId -ErrorAction Stop | Select-Object ProcessId,ParentProcessId | ConvertTo-Json -Compress'], { ...options, timeout: 15000 }),
    execute('netstat.exe', ['-ano'], options),
  ]);
  const parsed = JSON.parse(processes.stdout || '[]');
  return { processes: Array.isArray(parsed) ? parsed : [parsed], listeners: tcpListeners(sockets.stdout) };
}
