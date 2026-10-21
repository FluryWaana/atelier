export function fail(message, code = 'invalid') { throw Object.assign(new Error(message), { code }); }

export function text(value, limit = 20000) {
  if (typeof value !== 'string' || value.length > limit) fail('Texte invalide ou trop long.');
  return value.trim();
}
