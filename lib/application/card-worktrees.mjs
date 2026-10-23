import { fail, text } from '../domain/validation.mjs';

export function createCardWorktrees({ store, busy, configFor, publishing, changed, devServers, prepareWorktree, prepareApplicationWorktrees, samePath, removeApplicationWorktrees, removeWorktree, inspectTaskFiles }) {
  async function execute(card, action, body) {
    if (['changes', 'files', 'file', 'file-diff'].includes(action)) return inspectTaskFiles(card, action, body);
    if (card.archived && ['dev-servers', 'worktree'].includes(action)) fail('Restaure cette t\u00e2che avant de reprendre le travail.', 'conflict');
    if (['dev-servers', 'stop-dev-servers'].includes(action)) {
      if (body.standalone !== undefined && typeof body.standalone !== 'boolean') fail('Option de lancement invalide.');
      const serviceId = body.serviceId === undefined ? '' : text(body.serviceId, 200);
      if (body.serviceId !== undefined && !serviceId || body.restart !== undefined && typeof body.restart !== 'boolean' || body.restart && (!serviceId || action === 'stop-dev-servers')) fail('Commande de service invalide.');
      if (action === 'stop-dev-servers') { await devServers.stop(card.id, serviceId); return { ok: true }; }
      if (card.archived) fail('Restaure cette tâche avant de lancer le projet.', 'conflict');
      if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
      publishing.add(card.id); changed();
      try {
        if (!card.worktree) fail('Prépare le worktree avant de lancer le projet.');
        await prepareWorktree(card, configFor(card));
        const project = store.projects().find(project => samePath(project.repo, card.repo));
        await prepareApplicationWorktrees(card, project, card => store.putCard(card));
        if (serviceId) {
          const session = devServers.state().find(session => session.cardId === card.id);
          if (!session || session.projectIds.some(id => !store.projects().some(project => project.id === id))) fail('Service ou projet introuvable : relance l’ensemble depuis cette carte.');
          if (body.restart) await devServers.stop(card.id, serviceId);
          return await devServers.startManagedService(card.id, serviceId);
        }
        const linkedProject = body.standalone ? null : store.projects().find(item => item.id === project?.linkedProjectId);
        return await devServers.startManaged(card, body.standalone && project ? { ...project, linkedProjectId: '' } : project, linkedProject);
      } finally { publishing.delete(card.id); changed(); }
    }
    if (['worktree', 'remove-worktree'].includes(action)) {
      if (devServers.running(card.id)) fail('Arrête les serveurs Front / Back avant de retirer le worktree.', 'conflict');
      if (busy(card.id)) fail('Arrête l’agent avant de gérer son worktree.', 'conflict');
      publishing.add(card.id); changed();
      try {
        if (action === 'worktree') { card.worktree = await prepareWorktree(card, configFor(card)); card.branch = card.worktree.branch; }
        else {
          if (body.discardChanges !== undefined && typeof body.discardChanges !== 'boolean') fail('Confirmation de suppression invalide.');
          await removeApplicationWorktrees(card, body.discardChanges === true, card => store.putCard(card));
          await removeWorktree(card, body.discardChanges === true); delete card.worktree;
        }
        store.putCard(card); return card;
      } finally { publishing.delete(card.id); changed(); }
    }
    fail('Action introuvable.', 'not-found');
  }
  return { execute };
}
