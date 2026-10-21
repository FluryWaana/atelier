import { sameLocation } from '../paths.mjs';
import { extractTarPackage } from './tar-package.mjs';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, copyFileSync, readFileSync, existsSync, lstatSync, openSync, closeSync, unlinkSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { managedEntries, dataVersion } from '../domain/release-package.mjs';
import { updateLock, writeJson, processExists } from '../../scripts/update-worker.mjs';

const exec = promisify(execFile);
const powershell = join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
export async function download(url, limit, request = fetch) {
  const signal = AbortSignal.timeout(180000);
  let response;
  for (let redirects = 0; redirects <= 5; redirects++) {
    const target = new URL(url);
    if (target.protocol !== 'https:' || !['github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'].includes(target.hostname) || target.port || target.username || target.password || target.hash) throw new Error('Adresse de téléchargement non autorisée.');
    response = await request(target.href, { signal, redirect: 'manual' });
    if (![301, 302, 303, 307, 308].includes(response.status)) break;
    await response.body?.cancel();
    const location = response.headers.get('location');
    if (!location || redirects === 5) throw new Error('Redirection de téléchargement invalide.');
    url = new URL(location, target).href;
  }
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw new Error('Téléchargement de la mise à jour impossible.');
  }
  if (Number(response.headers.get('content-length')) > limit) {
    await response.body.cancel();
    throw new Error('Package trop volumineux.');
  }
  let size = 0; const chunks = [];
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > limit) throw new Error('Téléchargement trop volumineux.');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}
export function verifyChecksum(bytes, checksum, name) {
  const match = checksum.trim().match(/^([a-fA-F0-9]{64})\s+\*?([^\r\n]+)$/);
  if (!match || match[2] !== name || createHash('sha256').update(bytes).digest('hex') !== match[1].toLowerCase()) throw new Error('Empreinte du package invalide. Aucun fichier remplacé.');
}
export function createPackageUpdater({ root, version, repository, port, dataDir, executable = process.execPath }) {
  const binary = process.platform === 'win32' ? 'node.exe' : 'node';
  if (!['win32', 'linux', 'darwin'].includes(process.platform) || !['x64', 'arm64'].includes(process.arch) || !sameLocation(executable, join(root, 'runtime', binary)) || !sameLocation(dataDir, join(root, 'data'))) return null;
  return {
    async prepare(release) {
      if (updateLock(root)) throw new Error('Une mise à jour attend déjà son redémarrage.');
      for (const path of [root, join(root, '.updates'), dataDir]) if (existsSync(path) && lstatSync(path).isSymbolicLink()) throw new Error('Installation automatique indisponible dans un dossier lié.');
      const stage = join(root, '.updates', 'update-' + randomUUID()); mkdirSync(stage, { recursive: true });
      const checksum = (await download(release.checksumUrl, 1024)).toString('utf8');
      const archive = await download(release.url, 150 * 1024 * 1024); verifyChecksum(archive, checksum, release.name);
      await writeFile(join(stage, 'release.zip'), archive);
      const payload = join(stage, 'payload');
      if (process.platform === 'win32') await exec(powershell, ['-NoProfile', '-NonInteractive', '-File', join(root, 'scripts', 'Expand-Update.ps1'), '-Archive', join(stage, 'release.zip'), '-Destination', payload], { windowsHide: true, timeout: 120000, maxBuffer: 1024 * 1024 });
      else extractTarPackage(archive, payload);
      const manifest = JSON.parse(readFileSync(join(payload, 'release.json'), 'utf8'));
      const pkg = JSON.parse(readFileSync(join(payload, 'package.json'), 'utf8'));
      if (manifest.repository !== repository || (manifest.platform || 'win32') !== process.platform || (manifest.arch || 'x64') !== process.arch || manifest.dataVersion !== dataVersion || manifest.version !== release.version || pkg.version !== release.version || pkg.name !== 'atelier-kanban') throw new Error('Package incompatible avec cette installation.');
      for (const name of managedEntries) if (!existsSync(join(payload, name))) throw new Error('Package incomplet : ' + name);
      for (const file of ['runtime/' + binary, 'scripts/update-worker.mjs', 'scripts/Expand-Update.ps1', 'lib/domain/release-package.mjs']) if (!existsSync(join(payload, file))) throw new Error('Package incomplet : ' + file);
      mkdirSync(join(stage, 'scripts')); mkdirSync(join(stage, 'lib', 'domain'), { recursive: true });
      copyFileSync(executable, join(stage, binary));
      copyFileSync(join(root, 'scripts', 'update-worker.mjs'), join(stage, 'scripts', 'update-worker.mjs'));
      copyFileSync(join(root, 'lib', 'domain', 'release-package.mjs'), join(stage, 'lib', 'domain', 'release-package.mjs'));
      return { stage, version: release.version };
    },
    async start(staged, rescue) {
      const { stage } = staged, id = stage.split(/[\\/]/).at(-1), lockPath = join(root, '.update-lock.json');
      writeJson(join(stage, 'config.json'), { version: staged.version, previousVersion: version, port: port(), backupName: rescue.name });
      const lock = { id, ownerPid: process.pid, pid: null, token: randomUUID() };
      const fd = openSync(lockPath, 'wx'); closeSync(fd); writeJson(lockPath, lock);
      const log = openSync(join(stage, 'update.log'), 'a');
      const child = spawn(join(stage, binary), [join(stage, 'scripts', 'update-worker.mjs'), stage], { cwd: stage, detached: true, windowsHide: true, stdio: ['ignore', log, log] });
      closeSync(log);
      let failure; child.on('error', error => { failure = error; }); child.unref();
      try {
        for (let i = 0; i < 100; i++) {
          if (failure) throw failure;
          if (existsSync(join(stage, 'ready'))) return;
          if (child.exitCode !== null) throw new Error('Le programme de mise à jour a échoué. Consulte ' + join(stage, 'update.log'));
          await delay(100);
        }
        throw new Error('Le programme de mise à jour ne répond pas.');
      } catch (error) {
        if (processExists(child.pid)) { child.kill(); for (let i = 0; i < 40 && processExists(child.pid); i++) await delay(100); }
        if (!processExists(child.pid) && existsSync(lockPath)) unlinkSync(lockPath);
        throw error;
      }
    },
  };
}
