export function connectorRoute(connectors, method, path, body) {
  if (path === '/api/connectors' && method === 'GET') return connectors.describe();
  if (path === '/api/connectors/providers' && method === 'PUT') return connectors.select(body);
  const match = path.match(/^\/api\/connectors\/([a-z][a-z0-9-]*)\/(settings|actions|secrets)(?:\/([a-z][a-z0-9-]*))?$/);
  if (!match) return;
  if (match[2] === 'settings' && !match[3] && method === 'PUT') return connectors.configure(match[1], body);
  if (match[2] === 'actions' && match[3] && method === 'POST') return connectors.action(match[1], match[3], body);
  if (match[2] === 'secrets' && match[3] && method === 'DELETE') return connectors.forget(match[1], match[3]);
}
