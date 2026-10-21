export function timeRoute(service, method, path, body) {
  if (path === '/api/time' && method === 'POST') return service.execute('create', null, body);
  const match = path.match(/^\/api\/time\/([^/]+)(?:\/(stop|export|reconcile|reset-export))?$/);
  if (!match) throw new Error('Route de temps inconnue.');
  const action = !match[2] ? { PATCH: 'update', DELETE: 'remove' }[method] : method === 'POST' && match[2];
  if (!action) throw new Error('Méthode non autorisée.');
  return service.execute(action, match[1], body);
}
