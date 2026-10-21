import { createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { signaturePayload, verifyReleaseSignature } from '../lib/infrastructure/release-signature.mjs';
import { releasePublicKey } from '../lib/infrastructure/release-public-key.mjs';
import { releaseArtifacts } from './check-release.mjs';
import { verifyChecksum } from '../lib/infrastructure/package-updater.mjs';

export function signRelease(directory, version, repository, privateKey, publicKey = releasePublicKey) {
  const key = createPrivateKey(privateKey);
  if (key.asymmetricKeyType !== 'ed25519' || !createPublicKey(key).equals(createPublicKey(publicKey))) throw new Error('La clé de signature ne correspond pas à la clé publique embarquée.');
  const signatures = releaseArtifacts(version).map(name => {
    const checksum = readFileSync(join(directory, name + '.sha256'), 'utf8');
    verifyChecksum(readFileSync(join(directory, name)), checksum, name);
    const signature = sign(null, signaturePayload(checksum, name, repository, version), key).toString('base64');
    verifyReleaseSignature(checksum, signature, name, repository, version, publicKey);
    return [name, signature];
  });
  for (const [name, signature] of signatures) writeFileSync(join(directory, name + '.sha256.sig'), signature + '\n', { flag: 'wx' });
}

if (import.meta.main) {
  const { version } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  if (!process.env.RELEASE_SIGNING_KEY) throw new Error('Secret RELEASE_SIGNING_KEY requis pour publier.');
  signRelease(process.argv[2], version, process.env.GITHUB_REPOSITORY, process.env.RELEASE_SIGNING_KEY);
  console.log('Archives et installateurs signés.');
}
