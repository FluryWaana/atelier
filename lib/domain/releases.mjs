export function releaseVersion(value) {
  if (typeof value !== 'string' || !/^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)) throw new Error('Version stable invalide.');
  const version = value.replace(/^v/, '');
  if (version.split('.').some(part => !Number.isSafeInteger(Number(part)))) throw new Error('Version trop grande.');
  return version;
}

export function isNewer(candidate, installed) {
  const left = releaseVersion(candidate).split('.').map(Number), right = releaseVersion(installed).split('.').map(Number);
  for (let i = 0; i < 3; i++) if (left[i] !== right[i]) return left[i] > right[i];
  return false;
}

export function githubRepository(value) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9-]{0,38}\/[A-Za-z0-9_.-]+$/.test(value) || value.endsWith('/..') || value.endsWith('/.')) throw new Error('Dépôt GitHub invalide (propriétaire/nom).');
  return value;
}

export function packageName(version, platform = 'win32', arch = 'x64') {
  if (!['win32', 'linux', 'darwin'].includes(platform) || !['x64', 'arm64'].includes(arch)) throw new Error('Plateforme non prise en charge.');
  return `atelier-kanban-${releaseVersion(version)}-${{ win32: 'windows', linux: 'linux', darwin: 'macos' }[platform]}-${arch}.${platform === 'win32' ? 'zip' : 'tar.gz'}`;
}
