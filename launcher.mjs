import { spawn } from 'node:child_process';
import { mkdirSync, openSync, closeSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { updateLock, processExists } from './scripts/update-worker.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const port = Number(process.env.PORT || 4317);
const url = `http://127.0.0.1:${port}`;
async function health() {
  let response;
  try { response = await fetch(`${url}/api/health`, { signal: AbortSignal.timeout(1000) }); }
  catch { return null; }
  const value = await response.json();
  if (value.app !== 'atelier-kanban' || (process.platform === 'win32' ? resolve(value.workspace).toLowerCase() !== root.toLowerCase() : resolve(value.workspace) !== root)) {
    throw new Error('Ce port est utilisé par une autre application ou une autre copie Atelier.');
  }
  return value;
}
try {
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Port invalide.');
  const lock = updateLock(root);
  if (lock) {
    if (processExists(lock.pid) || processExists(lock.ownerPid)) throw new Error('Mise à jour en cours. Patiente avant de relancer Atelier.');
    const recovery = spawn(join(lock.stage, process.platform === 'win32' ? 'node.exe' : 'node'), [join(lock.stage, 'scripts', 'update-worker.mjs'), lock.stage, '--recover'], { cwd: lock.stage, windowsHide: true, stdio: 'inherit' });
    await new Promise((resolve, reject) => { recovery.on('error', reject); recovery.on('exit', code => code === 0 ? resolve() : reject(new Error('Récupération impossible. Consulte le dossier .updates.'))); });
  }
  let current = await health();
  if (process.argv.includes('--stop')) {
    if (current) {
      const response = await fetch(`${url}/api/state`, { signal: AbortSignal.timeout(3000) });
      if (!response.ok) throw new Error('Impossible de vérifier les opérations en cours.');
      const state = await response.json();
      if (state.runs.some(r => r.status === 'running') || state.cards.some(c => c.publishing) || state.times.some(t => t.runningSince || t.exportState === 'sending')) {
        throw new Error('Arrêtez les agents et chronomètres et attendez la fin des publications.');
      }
      for (const session of state.devServers || []) {
        const stopped = await fetch(`${url}/api/cards/${encodeURIComponent(session.cardId)}/dev-servers`, { method: 'DELETE', headers: { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, body: '{}' });
        if (!stopped.ok) throw new Error('Impossible d’arrêter les serveurs Front / Back.');
      }
      process.kill(current.pid);
      for (let i = 0; i < 30 && await health(); i++) await delay(200);
      if (await health()) throw new Error('Le serveur ne s’est pas arrêté.');
    }
    console.log('Atelier est arrêté.');
  } else {
    if (!current) {
      mkdirSync(join(root, 'data'), { recursive: true });
      const out = openSync(join(root, 'data/server.stdout.log'), 'a');
      const err = openSync(join(root, 'data/server.stderr.log'), 'a');
      const child = spawn(process.execPath, ['server.mjs'], { cwd: root, detached: true, windowsHide: true, stdio: ['ignore', out, err] });
      child.on('error', error => { console.error(error.message); process.exitCode = 1; });
      child.unref();
      closeSync(out);
      closeSync(err);
      for (let i = 0; i < 40 && !current; i++) { await delay(200); current = await health(); }
      if (!current) throw new Error('Démarrage impossible. Consultez data/server.stderr.log.');
    }
    console.log(`Atelier Kanban : ${url}\nUtilisez ${process.platform === 'win32' ? 'Stop.cmd' : 'Stop.command'} pour fermer le serveur.`);
    if (!process.argv.includes('--no-browser')) {
      const browser = spawn(process.platform === 'win32' ? 'explorer.exe' : process.platform === 'darwin' ? 'open' : 'xdg-open', [url], { windowsHide: true, detached: true, stdio: 'ignore' });
      browser.on('error', () => console.log(`Ouvrez ${url} dans votre navigateur.`));
      browser.unref();
    }
  }
} catch (error) { console.error(error.message); process.exitCode = 1; }
