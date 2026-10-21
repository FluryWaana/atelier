import assert from 'node:assert/strict';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { packageName, releaseVersion, installerNames } from '../lib/domain/releases.mjs';
import { verifyChecksum } from '../lib/infrastructure/package-updater.mjs';
import { verifyReleaseSignature } from '../lib/infrastructure/release-signature.mjs';

export function checkRelease(root, { tag, target, nodeVersion = process.versions.node } = {}) {
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  assert.equal(pkg.version, releaseVersion(pkg.version), 'Version stable X.Y.Z requise.');
  assert.equal(lock.version, pkg.version, 'La version du lockfile doit correspondre à package.json.');
  assert.equal(lock.packages[''].version, pkg.version, 'La version racine du lockfile doit correspondre à package.json.');
  assert.equal(nodeVersion, readFileSync(join(root, '.node-version'), 'utf8').trim(), 'Utiliser la version Node.js de .node-version.');
  if (tag) assert.equal(tag, 'v' + pkg.version, 'Le tag doit correspondre à package.json.');
  if (target) assert.equal(packageName(pkg.version, process.platform, process.arch), `atelier-kanban-${pkg.version}-${target}.${process.platform === 'win32' ? 'zip' : 'tar.gz'}`, 'Le runtime doit correspondre à la plateforme du job.');
  return pkg.version;
}

export function releaseArtifacts(version) {
  return [['win32', 'x64'], ['linux', 'x64'], ['linux', 'arm64'], ['darwin', 'x64'], ['darwin', 'arm64']]
    .flatMap(([platform, arch]) => [packageName(version, platform, arch), ...installerNames(version, platform, arch)]);
}

export function checkReleaseArtifacts(directory, version, repository = process.env.GITHUB_REPOSITORY, publicKey) {
  const archives = releaseArtifacts(version);
  const expected = archives.flatMap(name => [name, name + '.sha256', name + '.sha256.sig']).sort();
  assert.deepEqual(readdirSync(directory).sort(), expected, 'Les cinq archives portables, cinq installateurs, empreintes et signatures sont requis, sans fichier supplémentaire.');
  for (const name of expected) assert.ok(lstatSync(join(directory, name)).isFile(), 'Fichier de release invalide : ' + name);
  for (const name of archives) {
    const bytes = readFileSync(join(directory, name));
    assert.ok(bytes.length > 0, 'Paquet vide : ' + name);
    verifyChecksum(bytes, readFileSync(join(directory, name + '.sha256'), 'utf8'), name);
    verifyReleaseSignature(readFileSync(join(directory, name + '.sha256'), 'utf8'), readFileSync(join(directory, name + '.sha256.sig'), 'utf8'), name, repository, version, publicKey);
  }
}

if (import.meta.main) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const version = checkRelease(root, {
    tag: process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : undefined,
    target: process.env.RELEASE_TARGET,
  });
  if (process.argv[2]) checkReleaseArtifacts(resolve(process.argv[2]), version);
  console.log(`Release ${version} vérifiée${process.argv[2] ? ' : archives, installateurs, SHA256 et signatures valides' : ''}.`);
}
