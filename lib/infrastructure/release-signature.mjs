import { verify } from 'node:crypto';
import { githubRepository, releaseVersion } from '../domain/releases.mjs';
import { releasePublicKey } from './release-public-key.mjs';

export function signaturePayload(checksum, name, repository, version) {
  githubRepository(repository); releaseVersion(version);
  const match = checksum.trim().match(/^([a-fA-F0-9]{64})\s+\*?([^\r\n]+)$/);
  if (!match || match[2] !== name) throw new Error('Empreinte de signature invalide.');
  return Buffer.from(JSON.stringify({ format: 'atelier-release-v1', repository, version, name, sha256: match[1].toLowerCase() }));
}

export function verifyReleaseSignature(checksum, signature, name, repository, version, publicKey = releasePublicKey) {
  const encoded = signature.trim();
  if (!/^[A-Za-z0-9+/]{86}==$/.test(encoded) || !verify(null, signaturePayload(checksum, name, repository, version), publicKey, Buffer.from(encoded, 'base64'))) throw new Error('Signature de publication invalide. Aucun fichier remplacé.');
}
