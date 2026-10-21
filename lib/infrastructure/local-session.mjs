import { createHmac, timingSafeEqual, randomBytes } from 'node:crypto';
import { lstatSync, fstatSync, openSync, closeSync, constants, readFileSync, writeFileSync, renameSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { windowsProtect } from '../token-vault.mjs';

export function sameSecret(actual, expected) {
  return typeof actual === 'string' && Buffer.byteLength(actual) === Buffer.byteLength(expected) && timingSafeEqual(Buffer.from(actual), Buffer.from(expected));
}

export function sessionProof(token, challenge, port) {
  return createHmac('sha256', token).update(`${port}:${challenge}`).digest('hex');
}

export async function authenticatedHealth(url, loadToken, request = fetch) {
  const address = /^http:\/\/127\.0\.0\.1:([0-9]{1,5})$/.exec(url), port = Number(address?.[1]);
  if (!address || port < 1 || port > 65535) throw new Error('Adresse du serveur local invalide.');
  url = `http://127.0.0.1:${port}`;
  const challenge = randomBytes(32).toString('hex');
  let response;
  try { response = await request(`${url}/api/health?challenge=${challenge}`, { headers: { Connection: 'close' }, signal: AbortSignal.timeout(2000), redirect: 'error' }); }
  catch (error) { if (['ECONNREFUSED', 'ECONNRESET', 'UND_ERR_SOCKET'].includes(error.cause?.code)) return null; throw error; }
  const data = await response.json();
  // Startup writes the session after binding the port; read it after the server responds.
  const token = loadToken();
  if (!response.ok || !token || !sameSecret(data.proof, sessionProof(token, challenge, port))) throw new Error('Identité du serveur local non vérifiée. Ferme le programme qui utilise ce port.');
  response = await request(`${url}/api/health`, { headers: { Authorization: 'Bearer ' + token, Connection: 'close' }, signal: AbortSignal.timeout(2000), redirect: 'error' });
  if (!response.ok) throw new Error('Session locale refusée. Rouvre Atelier avec son lanceur.');
  return response.json();
}

export function readSession(dataDir) {
  const path = join(dataDir, 'local-session');
  let stat;
  try { stat = lstatSync(path); } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  if (!stat.isFile() || stat.isSymbolicLink() || process.platform !== 'win32' && (stat.uid !== process.getuid() || stat.mode & 0o077)) throw new Error('Permissions de la session locale invalides.');
  // codeql[js/file-system-race] Read through the descriptor only after fstat confirms the checked inode and permissions.
  const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0));
  let stored;
  try {
    const opened = fstatSync(fd);
    if (opened.dev !== stat.dev || opened.ino !== stat.ino || process.platform !== 'win32' && (opened.uid !== process.getuid() || opened.mode & 0o077)) throw new Error('La session locale a changé pendant sa lecture.');
    stored = readFileSync(fd, 'utf8');
  } finally { closeSync(fd); }
  const token = process.platform === 'win32' ? windowsProtect(stored, true) : stored;
  if (!/^[a-f0-9]{64}$/.test(token)) throw new Error('Session locale invalide.');
  return token;
}

export function writeSession(dataDir, token) {
  writePrivate(join(dataDir, 'local-session'), process.platform === 'win32' ? windowsProtect(token) : token);
}

function writePrivate(path, contents) {
  const temporary = path + '.' + randomBytes(16).toString('hex');
  writeFileSync(temporary, contents, { mode: 0o600, flag: 'wx' });
  renameSync(temporary, path);
}

export function browserAddress(dataDir, url, token, platform = process.platform) {
  const address = url + '/#session=' + token;
  if (platform === 'win32') return address;
  // POSIX process arguments are readable by other users; only pass a private file path.
  const path = join(dataDir, 'local-browser.html');
  writePrivate(path, '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0;url=' + address + '">');
  return pathToFileURL(path).href;
}
