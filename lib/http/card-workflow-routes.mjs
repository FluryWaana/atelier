const commands = {
  GET: { 'pr-preview': 'pr-preview', transitions: 'transitions' },
  POST: Object.fromEntries(['brainstorm', 'brainstorm-draft', 'create-jira', 'queue', 'refresh-pr', 'automation', 'resume-automation', 'stop-automation', 'dev-servers', 'sync-status', 'fix-review', 'check-review', 'prepare-ticket', 'review', 'pull-request', 'link-pr', 'worktree', 'run', 'transition'].map(name => [name, name])),
  DELETE: { queue: 'remove-queue', 'dev-servers': 'stop-dev-servers', worktree: 'remove-worktree' },
};

export function cardWorkflowRoute(workflows, method, path, body) {
  const match = path.match(/^\/api\/cards\/([^/]+)\/([^/]+)$/);
  if (!match) return;
  const command = Object.hasOwn(commands, method) && Object.hasOwn(commands[method], match[2]) ? commands[method][match[2]] : null;
  if (command) return workflows.execute(match[1], command, body);
  if (['pr-preview', 'pull-request', 'link-pr'].includes(match[2])) throw Object.assign(new Error('Méthode non autorisée.'), { status: 405 });
}
