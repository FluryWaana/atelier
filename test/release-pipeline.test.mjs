import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkRelease, checkReleaseArtifacts } from '../scripts/check-release.mjs';

function temporaryDirectory(t) {
  const directory = mkdtempSync(join(tmpdir(), 'atelier-pipeline-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test('release commands execute through a linked parent directory', t => {
  const root = temporaryDirectory(t), linked = join(root, 'linked');
  symlinkSync(fileURLToPath(new URL('../', import.meta.url)), linked, process.platform === 'win32' ? 'junction' : 'dir');
  const options = { encoding: 'utf8', windowsHide: true, env: { ...process.env, GITHUB_REF_TYPE: 'branch', RELEASE_TARGET: '' } };
  const check = spawnSync(process.execPath, [join(linked, 'scripts', 'check-release.mjs')], options);
  assert.equal(check.status, 0, check.stderr);
  assert.match(check.stdout, /Release .* vérifiée/);
  const update = spawnSync(process.execPath, [join(linked, 'scripts', 'update-worker.mjs'), root], options);
  assert.equal(update.status, 1, 'The worker must execute and reject an invalid stage, not silently exit.');
  assert.match(update.stderr, /Dossier de mise à jour invalide/);
});

test('release preflight rejects inconsistent versions, tags and runtimes', t => {
  const root = temporaryDirectory(t);
  writeFileSync(join(root, 'package.json'), JSON.stringify({ version: '1.2.3' }));
  const lock = { version: '1.2.3', packages: { '': { version: '1.2.3' } } };
  const writeLock = () => writeFileSync(join(root, 'package-lock.json'), JSON.stringify(lock));
  writeLock();
  writeFileSync(join(root, '.node-version'), process.versions.node + '\n');
  const target = { win32: 'windows', linux: 'linux', darwin: 'macos' }[process.platform] + '-' + process.arch;
  assert.equal(checkRelease(root, { tag: 'v1.2.3', target }), '1.2.3');
  assert.throws(() => checkRelease(root, { tag: 'v1.2.4' }), /tag/);
  assert.throws(() => checkRelease(root, { target: 'wrong-platform' }), /plateforme/);
  assert.throws(() => checkRelease(root, { nodeVersion: '0.0.0' }), /Node.js/);
  lock.version = '1.2.2'; writeLock();
  assert.throws(() => checkRelease(root), /lockfile/);
  lock.version = '1.2.3'; lock.packages[''].version = '1.2.2'; writeLock();
  assert.throws(() => checkRelease(root), /lockfile/);
  for (const version of ['v1.2.3', '1.2.3-beta.1']) {
    writeFileSync(join(root, 'package.json'), JSON.stringify({ version }));
    assert.throws(() => checkRelease(root), /[Vv]ersion/);
  }
});

test('publication requires all portable archives, installers and their valid checksums', t => {
  const root = temporaryDirectory(t);
  const archives = ['windows-x64.zip', 'linux-x64.tar.gz', 'linux-arm64.tar.gz', 'macos-x64.tar.gz', 'macos-arm64.tar.gz', 'windows-x64-setup.exe', 'linux-x64.deb', 'linux-x64.rpm', 'linux-arm64.deb', 'linux-arm64.rpm']
    .map(suffix => 'atelier-kanban-1.2.3-' + suffix);
  for (const name of archives) {
    const bytes = Buffer.from(name);
    writeFileSync(join(root, name), bytes);
    writeFileSync(join(root, name + '.sha256'), `${createHash('sha256').update(bytes).digest('hex')}  ${name}\n`);
  }
  assert.doesNotThrow(() => checkReleaseArtifacts(root, '1.2.3'));
  assert.throws(() => checkReleaseArtifacts(root, '1.2.4'), /installateurs/);
  const archive = join(root, archives[0]), checksum = archive + '.sha256';
  const originalChecksum = readFileSync(checksum);
  rmSync(checksum);
  assert.throws(() => checkReleaseArtifacts(root, '1.2.3'), /installateurs/);
  writeFileSync(checksum, originalChecksum);
  writeFileSync(join(root, 'unexpected.txt'), 'unexpected');
  assert.throws(() => checkReleaseArtifacts(root, '1.2.3'), /supplémentaire/);
  rmSync(join(root, 'unexpected.txt'));
  writeFileSync(archive, 'corrupted');
  assert.throws(() => checkReleaseArtifacts(root, '1.2.3'), /Empreinte/);
  writeFileSync(archive, '');
  assert.throws(() => checkReleaseArtifacts(root, '1.2.3'), /vide/);
});
