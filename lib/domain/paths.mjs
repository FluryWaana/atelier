export function sameRepository(a, b, platform) {
  const key = value => platform === 'win32' ? (value || '').replaceAll('\\', '/').toLowerCase() : (value || '');
  return key(a) === key(b);
}
