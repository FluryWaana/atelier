export async function updateRoute(updates, method, path, body) {
  if (path === '/api/updates' && method === 'GET') return updates.state();
  if (path === '/api/updates/check' && method === 'POST') return updates.check();
  if (path === '/api/updates/install' && method === 'POST') return updates.install(body.version);
  throw Object.assign(new Error('Route introuvable.'), { status: 404 });
}
