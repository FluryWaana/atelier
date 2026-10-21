export function taskRoute(tasks, method, path, body) {
  if (['/api/cards', '/api/brainstorms'].includes(path) && method === 'POST') return tasks.create(body, path === '/api/brainstorms');
  if (path === '/api/templates' && method === 'POST') return tasks.createTemplate(body);
  const template = path.match(/^\/api\/templates\/([^/]+)$/);
  if (template && method === 'DELETE') return tasks.removeTemplate(template[1]);
  const card = path.match(/^\/api\/cards\/([^/]+)(?:\/(archive|duplicate))?$/);
  if (!card) return;
  if (card[2] === 'archive' && method === 'POST') return tasks.archive(card[1], body);
  if (card[2] === 'duplicate' && method === 'POST') return tasks.duplicate(card[1]);
  if (!card[2] && method === 'PATCH') return tasks.update(card[1], body);
  if (!card[2] && method === 'DELETE') return tasks.remove(card[1]);
}
