export function agentProviderRoute(providers, method, path, body) {
  if (path === '/api/agent-providers' && method === 'GET') return providers.describe();
  if (path === '/api/agent-providers' && method === 'POST') return providers.save(body);
  const update = path.match(/^\/api\/agent-providers\/([a-z][a-z0-9-]{0,63})\/update$/);
  if (update && method === 'GET') return providers.updateState();
  if (update && method === 'POST') return providers.update(update[1], body.version);
  const match = path.match(/^\/api\/agent-providers\/([a-z][a-z0-9-]{0,63})$/);
  if (match && method === 'DELETE') return providers.remove(match[1]);
}
