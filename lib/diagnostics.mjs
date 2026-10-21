import { runCommand, azureRepository, azureRequest } from './pull-requests.mjs';

export async function checkConnections({ only, repo, settings, token, azureToken, tempoToken, requestJira, requestTempo, probe, run = runCommand, requestAzure = azureRequest }) {
  const checks = [
    ['Git', async () => {
      const version = await run('git', ['--version']);
      if (!repo) return version + ' · choisis un projet pour vérifier son dépôt';
      await run('git', ['rev-parse', '--show-toplevel'], repo);
      return version + ' · dépôt accessible';
    }],
    ...['codex', 'claude'].map(agent => [agent === 'codex' ? 'Codex' : 'Claude Code', async () => {
      const result = await probe(agent);
      if (!result.installed || !result.connected) throw new Error(result.installed ? 'Connexion CLI nécessaire.' : 'CLI introuvable.');
      return result.version;
    }]),
    ['Jira', async () => { const user = await requestJira(settings, token, `/rest/api/${settings.jiraType === 'server' ? '2' : '3'}/myself`); return `Connecté : ${user.displayName || 'compte authentifié'}`; }],
    ['Azure Repos', async () => {
      if (!repo) throw new Error('Sélectionne un projet avec un remote origin Azure Repos.');
      const remote = await run('git', ['remote', 'get-url', '--push', 'origin'], repo);
      const repository = azureRepository(remote);
      await requestAzure(repository, azureToken);
      return repository.name + ' · API accessible (droits de push non vérifiés)';
    }],
    ['Tempo Cloud', async () => {
      if (settings.jiraType !== 'cloud') throw new Error('Tempo Cloud nécessite Jira Cloud.');
      const user = await requestJira(settings, token, '/rest/api/3/myself');
      if (!user.accountId) throw new Error('Compte Jira Cloud introuvable.');
      const date = new Date().toISOString().slice(0, 10);
      await requestTempo(tempoToken, `/worklogs/user/${encodeURIComponent(user.accountId)}?from=${date}&to=${date}&limit=1`);
      return 'Lecture autorisée · aucun temps envoyé';
    }],
  ];
  return Promise.all(checks.filter(([name]) => !only || only.includes(name)).map(async ([name, check]) => {
    try { return { name, ok: true, message: await check() }; }
    catch (error) { return { name, ok: false, message: error.message }; }
  }));
}

export async function inspectWorktrees(cards, run = runCommand) {
  const result = [];
  // Inspect sequentially to avoid launching hundreds of Git processes at once.
  for (const card of cards.filter(c => c.worktree)) {
    try {
      const path = card.worktree.path;
      const branch = await run('git', ['branch', '--show-current'], path);
      const status = await run('git', ['status', '--short'], path);
      result.push({ cardId: card.id, branch, status, clean: !status, coherent: branch === card.worktree.branch });
    } catch { result.push({ cardId: card.id, error: 'Worktree inaccessible. Vérifie son dossier et son association Git.' }); }
  }
  return result;
}
