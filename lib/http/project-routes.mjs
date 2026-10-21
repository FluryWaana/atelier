export function projectRoute(projects, method, path, body) {
  if (path === '/api/projects' && method === 'POST') return projects.create(body);
  if (path === '/api/projects/archive-tasks' && method === 'POST') return projects.archiveTasks(body.repo);
  const detection = path.match(/^\/api\/projects\/([^/]+)\/detect-launch$/);
  if (detection && ['GET', 'POST'].includes(method)) return projects.detect(detection[1], body, method === 'POST');
  const project = path.match(/^\/api\/projects\/([^/]+)$/);
  if (project && method === 'PATCH') return projects.update(project[1], body);
  if (project && method === 'DELETE') return projects.remove(project[1], body);
}
