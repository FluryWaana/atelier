import { spawn } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, renameSync, mkdirSync, copyFileSync, openSync, closeSync, unlinkSync, lstatSync, realpathSync } from 'node:fs';
import { dirname, join, resolve, basename } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { managedEntries } from '../lib/domain/release-package.mjs';
import { dataDirectory } from '../lib/infrastructure/installation.mjs';
import { authenticatedHealth, readSession } from '../lib/infrastructure/local-session.mjs';

export function processExists(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try { process.kill(pid, 0); return true; } catch (error) { return error.code !== 'ESRCH'; }
}
export function writeJson(path, value) {
  writeFileSync(path + '.tmp', JSON.stringify(value)); renameSync(path + '.tmp', path);
}
export function updateLock(root) {
  const path = join(root, '.update-lock.json');
  if (!existsSync(path)) return null;
  const lock = JSON.parse(readFileSync(path, 'utf8'));
  if (!/^update-[a-f0-9-]{36}$/.test(lock.id)) throw new Error('Verrou de mise à jour invalide.');
  return { ...lock, stage: join(root, '.updates', lock.id) };
}
function assertPlain(path) {
  if (existsSync(path) && lstatSync(path).isSymbolicLink()) throw new Error('Lien symbolique interdit pendant une mise à jour : ' + path);
}
export function swapFiles(root, stage) {
  const previous = join(stage, 'previous'), payload = join(stage, 'payload');
  mkdirSync(previous);
  const transaction = { existed: Object.fromEntries(managedEntries.map(name => { assertPlain(join(root, name)); return [name, existsSync(join(root, name))]; })), started: false };
  writeJson(join(stage, 'transaction.json'), transaction);
  for (const name of managedEntries) {
    if (transaction.existed[name]) renameSync(join(root, name), join(previous, name));
    renameSync(join(payload, name), join(root, name));
  }
  return transaction;
}
export function restoreFiles(root, stage) {
  const file = join(stage, 'transaction.json');
  if (!existsSync(file)) return;
  const transaction = JSON.parse(readFileSync(file, 'utf8'));
  const failed = join(stage, 'failed'); mkdirSync(failed, { recursive: true });
  for (const name of [...managedEntries].reverse()) {
    const old = join(stage, 'previous', name), current = join(root, name);
    if (existsSync(old) || !transaction.existed[name] && !existsSync(join(stage, 'payload', name))) {
      assertPlain(current);
      if (existsSync(current)) renameSync(current, join(failed, name));
      if (existsSync(old)) renameSync(old, current);
    }
  }
  if (transaction.started) {
    const { backupName } = JSON.parse(readFileSync(join(stage, 'config.json'), 'utf8'));
    if (!/^atelier-\d{13}-[a-f0-9-]{36}\.sqlite$/.test(backupName)) throw new Error('Sauvegarde de secours invalide.');
    const data = dataDirectory(root); assertPlain(data);
    const backup = join(data, 'backups', backupName);
    if (!existsSync(backup)) throw new Error('Sauvegarde de secours introuvable.');
    for (const suffix of ['', '-wal', '-shm']) {
      const file = join(data, 'kanban.sqlite' + suffix);
      const saved = join(failed, 'kanban.sqlite' + suffix);
      if (existsSync(file) && !existsSync(saved)) renameSync(file, saved);
      else if (suffix && existsSync(file)) unlinkSync(file);
    }
    copyFileSync(backup, join(data, 'kanban.sqlite'));
  }
}
function startServer(root, port, token) {
  const out = openSync(join(dataDirectory(root), 'server.stdout.log'), 'a'), err = openSync(join(dataDirectory(root), 'server.stderr.log'), 'a');
  const child = spawn(join(root, 'runtime', process.platform === 'win32' ? 'node.exe' : 'node'), [join(root, 'server.mjs')], { cwd: root, windowsHide: true, detached: true, stdio: ['ignore', out, err], env: { ...process.env, PORT: String(port), ATELIER_UPDATE_TOKEN: token } });
  closeSync(out); closeSync(err);
  child.on('error', () => {}); child.unref(); return child;
}
async function healthy(root, port, version, pid) {
  for (let i = 0; i < 60; i++) {
    try {
      const value = await authenticatedHealth(`http://127.0.0.1:${port}`, () => readSession(dataDirectory(root)));
      if (value?.app === 'atelier-kanban' && realpathSync(value.workspace) === realpathSync(root) && value.version === version && value.pid === pid) return;
    } catch {}
    await delay(500);
  }
  throw new Error('La version installée ne répond pas au contrôle de démarrage.');
}
export async function runUpdate(stage, recover = false) {
  stage = resolve(stage);
  const root = dirname(dirname(stage));
  if (basename(dirname(stage)) !== '.updates' || !/^update-[a-f0-9-]{36}$/.test(basename(stage))) throw new Error('Dossier de mise à jour invalide.');
  assertPlain(root); assertPlain(join(root, '.updates')); assertPlain(stage); assertPlain(dataDirectory(root));
  const config = JSON.parse(readFileSync(join(stage, 'config.json'), 'utf8'));
  const lockPath = join(root, '.update-lock.json'), lock = updateLock(root);
  if (!lock || lock.id !== basename(stage)) throw new Error('Verrou de mise à jour absent.');
  if (recover && (processExists(lock.pid) || processExists(lock.ownerPid))) throw new Error('La mise à jour est encore active.');
  if (recover && processExists(lock.serverPid)) throw new Error('Arrête le serveur de la mise à jour avant de récupérer la version précédente.');
  writeJson(lockPath, { id: lock.id, ownerPid: lock.ownerPid, pid: process.pid, token: lock.token });
  let child;
  try {
    if (recover) throw new Error('Mise à jour interrompue : restauration de la version précédente.');
    writeFileSync(join(stage, 'ready'), 'ready');
    for (let i = 0; i < 120 && processExists(lock.ownerPid); i++) await delay(500);
    if (processExists(lock.ownerPid)) throw new Error('Le serveur ne s’est pas arrêté ; aucun fichier remplacé.');
    const transaction = swapFiles(root, stage);
    transaction.started = true; writeJson(join(stage, 'transaction.json'), transaction);
    child = startServer(root, config.port, lock.token);
    writeJson(lockPath, { ...lock, pid: process.pid, serverPid: child.pid });
    await healthy(root, config.port, config.version, child.pid);
    writeJson(join(dataDirectory(root), 'update-result.json'), { version: config.version, success: true, message: `Mise à jour ${config.version} installée.`, date: new Date().toISOString() });
    unlinkSync(lockPath);
  } catch (error) {
    if (child && processExists(child.pid)) {
      child.kill();
      for (let i = 0; i < 40 && processExists(child.pid); i++) await delay(250);
      if (processExists(child.pid)) throw new Error('Le nouveau serveur refuse de s’arrêter. Restauration différée pour protéger les données.');
    }
    restoreFiles(root, stage);
    writeJson(join(dataDirectory(root), 'update-result.json'), { version: config.version, success: false, message: error.message, date: new Date().toISOString() });
    unlinkSync(lockPath);
    if (!processExists(lock.ownerPid)) {
      child = startServer(root, config.port, '');
      await healthy(root, config.port, config.previousVersion, child.pid);
    }
  }
}

if (import.meta.main) {
  runUpdate(process.argv[2], process.argv.includes('--recover')).catch(error => { console.error(error); process.exitCode = 1; });
}
