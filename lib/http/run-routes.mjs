export function runRoute(runs, method, path, body) {
  const match = path.match(/^\/api\/runs\/([^/]+)\/reply$/);
  if (match && method === 'POST') return runs.reply(match[1], body);
}
