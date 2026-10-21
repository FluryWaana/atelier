import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { signaturePayload, verifyReleaseSignature } from '../lib/infrastructure/release-signature.mjs';
import { createPackageUpdater } from '../lib/infrastructure/package-updater.mjs';

test('release signatures bind the checksum, asset, version and repository to the trusted key', () => {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const name = 'atelier-kanban-1.2.3-windows-x64.zip', repository = 'owner/repo', version = '1.2.3';
  const checksum = createHash('sha256').update('release').digest('hex') + '  ' + name;
  const signature = sign(null, signaturePayload(checksum, name, repository, version), privateKey).toString('base64');
  assert.doesNotThrow(() => verifyReleaseSignature(checksum, signature, name, repository, version, publicKey));
  for (const [sum, sig, file, repo, release, key] of [
    [checksum.replace(/^[a-f0-9]/, checksum[0] === 'a' ? 'b' : 'a'), signature, name, repository, version, publicKey],
    [checksum, '', name, repository, version, publicKey],
    [checksum, signature, name, 'other/repo', version, publicKey],
    [checksum, signature, name, repository, '1.2.4', publicKey],
    [checksum, signature, 'other.zip', repository, version, publicKey],
    [checksum, signature, name, repository, version, generateKeyPairSync('ed25519').publicKey],
  ]) assert.throws(() => verifyReleaseSignature(sum, sig, file, repo, release, key));
});

test('updater rejects a replaced checksum and signature before downloading the archive', async t => {
  const root = mkdtempSync(join(tmpdir(), 'atelier-signature-'));
  t.after(() => { assert.ok(resolve(root).startsWith(resolve(tmpdir()) + sep)); rmSync(root, { recursive: true, force: true }); });
  const name = 'atelier-kanban-1.2.3-windows-x64.zip';
  const checksum = createHash('sha256').update('tampered release').digest('hex') + '  ' + name;
  const untrusted = generateKeyPairSync('ed25519').privateKey;
  const signature = sign(null, signaturePayload(checksum, name, 'owner/repo', '1.2.3'), untrusted).toString('base64');
  const requested = [];
  t.mock.method(globalThis, 'fetch', async url => {
    requested.push(url);
    assert.ok(url.endsWith('.sha256') || url.endsWith('.sig'), 'Unverified archive must not be fetched');
    return new Response(url.endsWith('.sig') ? signature : checksum);
  });
  const updater = createPackageUpdater({ root, version: '1.2.2', repository: 'owner/repo', dataDir: join(root, 'data'), port: () => 4317,
    executable: join(root, 'runtime', process.platform === 'win32' ? 'node.exe' : 'node') });
  const url = 'https://github.com/owner/repo/releases/download/v1.2.3/' + name;
  await assert.rejects(updater.prepare({ name, version: '1.2.3', url, checksumUrl: url + '.sha256', signatureUrl: url + '.sha256.sig' }), /Signature/);
  assert.deepEqual(requested, [url + '.sha256', url + '.sha256.sig']);
});
