export function settingsRoute(settings, method, path, body) {
  if (path === '/api/settings' && method === 'PUT') return settings.update(body);
  if (path.startsWith('/api/settings/tokens/') && method === 'DELETE') return settings.removeToken(path.slice('/api/settings/tokens/'.length));
}
