import { fail, text } from '../domain/validation.mjs';

export function createProjects({ store, repository, samePath, projectSettings, repositoryConnectorAvailable = () => false, detectLaunch, busy, devServers, backup, invalidateReviews, newId, timestamp }) {
  function archiveProjectCards(repo) {
    const cards = store.cards().filter(card => samePath(card.repo, repo));
    const ids = new Set(cards.map(card => card.id));
    if (cards.some(card => busy(card.id)) || store.times().some(entry => ids.has(entry.cardId) && (entry.runningSince || entry.exportState === 'sending'))) fail('Arrête les agents et chronomètres et attends la fin des envois avant de retirer ce projet.', 'conflict');
    store.transaction(() => { for (const card of cards) store.putCard({ ...card, archived: true, updatedAt: timestamp() }); });
    return cards.length;
  }
  function create(body) {
    const name = text(body.name, 100); if (!name) fail('Un nom de projet est nécessaire.');
    const repo = repository(body.repo);
    if (store.projects().some(project => samePath(project.repo, repo))) fail('Ce répertoire figure déjà dans les projets.', 'conflict');
    const project = { id: newId(), name, repo };
    store.putProject(project); return project;
  }
  function archiveTasks(value) {
    const repo = text(value, 2000); if (!repo) fail('Répertoire requis.');
    return { count: archiveProjectCards(repo) };
  }
  function detect(id, body, useDraft) {
    const project = store.projects().find(project => project.id === id);
    if (!project) fail('Projet introuvable.', 'not-found');
    const sources = { repo: project.repo, frontRepo: project.frontRepo, backRepo: project.backRepo };
    if (body.role !== undefined && !['front', 'back'].includes(body.role)) fail('Application à détecter invalide.');
    if (useDraft) for (const key of ['repo', 'frontRepo', 'backRepo']) if (body[key] !== undefined && (!body.role || key === 'repo' || key === body.role + 'Repo')) sources[key] = key === 'repo' || body[key] ? repository(body[key]) : '';
    const scans = new Map(), result = { candidates: [], warnings: [] };
    for (const role of ['front', 'back']) {
      if (body.role && body.role !== role) continue;
      const repo = sources[role + 'Repo'] || sources.repo;
      if (!scans.has(repo)) scans.set(repo, detectLaunch(repo));
      const detected = scans.get(repo);
      result.candidates.push(...detected.candidates.filter(candidate => sources[role + 'Repo'] || candidate.role === role).map(candidate => ({ ...candidate, role, sourceRepo: repo })));
      result.warnings.push(...detected.warnings);
    }
    result.warnings = [...new Set(result.warnings)];
    return result;
  }
  function update(id, body) {
    const project = store.projects().find(p => p.id === id);
    if (!project) fail('Projet introuvable.', 'not-found');
    if (store.cards().some(card => samePath(card.repo, project.repo) && busy(card.id))) fail('Attends la fin des opérations du projet avant de modifier ses réglages.', 'conflict');
    const options = projectSettings({ ...project, ...body });
    if (body.repositoryConnector !== undefined && options.repositoryConnector && !repositoryConnectorAvailable(options.repositoryConnector)) fail('Ce module Git est absent ou ne gère pas les dépôts.');
    for (const role of ['front', 'back']) {
      const key = role + 'Repo';
      if (options[key]) options[key] = repository(options[key]);
      if (!samePath(options[key] || '', project[key] || '') && store.cards().some(card => samePath(card.repo, project.repo) && card.serviceWorktrees?.[role])) fail(`Retire les worktrees associés au ${role === 'front' ? 'Front' : 'Back'} avant de changer son dépôt.`, 'conflict');
    }
    if (options.linkedProjectId && (options.linkedProjectId === project.id || !store.projects().some(p => p.id === options.linkedProjectId))) fail('Choisis un autre projet enregistré pour les tests.');
    const repo = body.repo === undefined ? project.repo : repository(body.repo);
    const moved = !samePath(repo, project.repo);
    const cards = store.cards().filter(card => samePath(card.repo, project.repo));
    if (moved && cards.some(card => card.worktree)) fail('Retire les worktrees de ce projet avant de modifier son emplacement. Aucun fichier ne sera déplacé automatiquement.', 'conflict');
    if (moved && devServers.state().some(session => session.projectIds.includes(project.id) && devServers.running(session.cardId))) fail('Arrête les applications de ce projet avant de modifier son emplacement.', 'conflict');
    if (moved && store.projects().some(item => item.id !== project.id && samePath(item.repo, repo))) fail('Ce répertoire figure déjà dans les projets.', 'conflict');
    const updated = { ...project, ...options, repo };
    if (!moved) { store.putProject(updated); return updated; }
    backup();
    store.transaction(() => {
      store.putProject(updated);
      for (const card of cards) {
        store.putCard({ ...card, repo, updatedAt: timestamp() });
        invalidateReviews(card.id, 'Emplacement du projet modifié : relancez les vérifications et la review.');
      }
    });
    return updated;
  }
  function remove(id, body) {
    const project = store.projects().find(p => p.id === id);
    if (!project) fail('Projet introuvable.', 'not-found');
    if (store.cards().some(card => samePath(card.repo, project.repo) && busy(card.id))) fail('Attends la fin des opérations du projet.', 'conflict');
    if (devServers.state().some(session => session.projectIds.includes(project.id) && devServers.running(session.cardId))) fail('Arrête les serveurs Front / Back de ce projet.', 'conflict');
    if (body.archiveTasks !== undefined && typeof body.archiveTasks !== 'boolean') fail('Option d’archivage invalide.');
    const archived = body.archiveTasks ? archiveProjectCards(project.repo) : 0;
    store.deleteProject(id); return { ok: true, archived };
  }
  return { create, archiveTasks, detect, update, remove };
}
