import { fail, text } from '../domain/validation.mjs';
import { reviewRequirements } from '../domain/tasks.mjs';
import { checklistText } from '../domain/checklist.mjs';

export function createCardWorkflows({ store, manualLaunch, startRun, busy, config, configFor, token, jiraBase, requestJira, publishing, changed, queue, refreshPR, invalidateReviews, automation, checkDependencies, devServers, prepareWorktree, prepareApplicationWorktrees, samePath, syncCardStatus, correctionInstructions, reviewPR, previewPR, publishPR, linkPullRequestToJira, removeApplicationWorktrees, removeWorktree, launchTask, newId, timestamp }) {
  async function execute(id, action, body) {
      const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');

      if (action === 'brainstorm') {
        manualLaunch();
        return startRun(card, { agent: card.agent, mode: 'analyze', instructions: text(body.reply ?? '') }, null, null, null, 'brainstorm');
      }
      if (action === 'brainstorm-draft') {
        if (card.archived || busy(card.id)) fail('Restaure la carte et attends la fin des opérations.', 'conflict');
        const run = store.run(body.runId), index = body.index;
        if (run?.cardId !== card.id || run.kind !== 'brainstorm' || run.status !== 'completed' || !Number.isInteger(index) || !run.proposals?.[index]) fail('Proposition introuvable.');
        const existing = store.cards().find(c => c.brainstormSource?.runId === run.id && c.brainstormSource.index === index);
        if (existing) return existing;
        const proposal = run.proposals[index];
        const draft = { id: newId(), title: proposal.title, description: proposal.description, checklist: proposal.checklist, instructions: proposal.questions.length ? 'Questions à trancher :\n' + proposal.questions.join('\n') : '', repo: card.repo, agent: card.agent, model: card.model || '', effort: card.effort || '', mode: 'edit', status: 'todo', brainstormSource: { runId: run.id, index }, createdAt: timestamp(), updatedAt: timestamp() };
        store.putCard(draft); return draft;
      }
      if (action === 'create-jira') {
        if (card.ticket) return card;
        if (card.archived || busy(card.id)) fail('Restaure la carte et attends la fin des opérations.', 'conflict');
        if (card.ticketCreation && !body.existingKey && body.confirmRetry !== true) fail('Envoi précédent incertain : vérifie Jira avant de réessayer.', 'conflict');
        const settings = config(), base = jiraBase(settings.jiraUrl), apiBase = `/rest/api/${settings.jiraType === 'server' ? '2' : '3'}`;
        if (!token('jira')) fail('Renseigne la connexion Jira dans les paramètres.');
        if (card.ticketCreation && card.ticketCreation.base !== base) fail('Reconnecte le site Jira de l’envoi précédent avant de continuer.');
        let fields;
        if (!body.existingKey) {
          const project = text(body.project, 100), issueType = text(body.issueType, 100);
          if (!/^[A-Z][A-Z0-9_]*$/.test(project) || !issueType) fail('Clé de projet et type de ticket Jira requis.');
          const description = (card.description || '') + checklistText(card.checklist) + (card.instructions ? '\n\n' + card.instructions : '');
          // shortcut: seuls les champs standards sont envoyés ; ajouter les métadonnées si un projet exige des champs personnalisés.
          fields = { project: { key: project }, issuetype: { name: issueType }, summary: card.title, description: settings.jiraType === 'server' ? description : { type: 'doc', version: 1, content: description.split('\n').map(line => ({ type: 'paragraph', content: line ? [{ type: 'text', text: line }] : [] })) } };
        } else if (!/^[A-Z][A-Z0-9_]*-\d+$/.test(body.existingKey)) fail('Clé Jira invalide.');
        publishing.add(card.id); changed();
        try {
          let issue;
          if (body.existingKey) issue = await requestJira(settings, token('jira'), apiBase + '/issue/' + encodeURIComponent(body.existingKey) + '?fields=status');
          else {
            card.ticketCreation = { state: 'uncertain', base, startedAt: timestamp() }; store.putCard(card);
            issue = await requestJira(settings, token('jira'), apiBase + '/issue', { fields });
          }
          if (!/^[A-Z][A-Z0-9_]*-\d+$/.test(issue.key)) fail('Réponse Jira incertaine : vérifie le ticket dans Jira.');
          card.ticket = { connector: 'jira', id: issue.id, key: issue.key, base, url: base + '/browse/' + issue.key, status: issue.fields?.status?.name || 'Créé' };
          delete card.ticketCreation; card.updatedAt = timestamp(); store.putCard(card); return card;
        } finally { publishing.delete(card.id); changed(); }
      }
      if (['queue', 'remove-queue'].includes(action)) {
        if (action === 'remove-queue') return queue.remove(card.id);
        if (card.archived || card.status === 'done' || busy(card.id)) fail('Cette tâche ne peut pas être ajoutée à la file.', 'conflict');
        return queue.add(card.id);
      }
      if (action === 'refresh-pr') {
        if (!card.pullRequest) fail('Aucune PR enregistrée.');
        if (busy(card.id)) fail('Attends la fin des opérations avant d’actualiser la PR.', 'conflict');
        publishing.add(card.id); changed();
        try {
          const status = await refreshPR(card, token('azure'));
          const current = store.card(card.id); current.pullRequest = { ...current.pullRequest, ...status }; delete current.pullRequest.refreshError; store.putCard(current);
          if (store.runs().some(r => r.cardId === card.id && r.kind === 'pr-review' && r.reviewContext && (status.sourceCommit && r.reviewContext.source !== status.sourceCommit || status.targetCommit && r.reviewContext.target !== status.targetCommit))) invalidateReviews(card.id, 'Les commits distants ont changé : relance la review.');
        } catch (error) {
          const current = store.card(card.id); current.pullRequest.refreshError = error.message; store.putCard(current);
        } finally { publishing.delete(card.id); changed(); }
        return store.card(card.id);
      }
      if (['automation', 'resume-automation', 'stop-automation'].includes(action)) {
        if (action === 'stop-automation') { automation.stop(card.id); return { ok: true }; }
        manualLaunch(); checkDependencies(card);
        if (card.archived) fail('Restaure cette tâche avant de reprendre le travail.', 'conflict');
        if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
        return automation.start(card, { agent: card.agent, mode: 'edit', instructions: card.instructions || '' }, action === 'resume-automation');
      }
      if (['dev-servers', 'stop-dev-servers'].includes(action)) {
        if (body.standalone !== undefined && typeof body.standalone !== 'boolean') fail('Option de lancement invalide.');
        const serviceId = body.serviceId === undefined ? '' : text(body.serviceId, 200);
        if (body.serviceId !== undefined && !serviceId || body.restart !== undefined && typeof body.restart !== 'boolean' || body.restart && (!serviceId || action === 'stop-dev-servers')) fail('Commande de service invalide.');
        if (action === 'stop-dev-servers') { await devServers.stop(card.id, serviceId); return { ok: true }; }
        if (card.archived) fail('Restaure cette tâche avant de lancer le projet.', 'conflict');
        if (!card.worktree) fail('Prépare le worktree avant de lancer le projet.');
        prepareWorktree(card, configFor(card));
        const project = store.projects().find(project => samePath(project.repo, card.repo));
        prepareApplicationWorktrees(card, project, card => store.putCard(card));
        if (serviceId) {
          const session = devServers.state().find(session => session.cardId === card.id);
          if (!session || session.projectIds.some(id => !store.projects().some(project => project.id === id))) fail('Service ou projet introuvable : relance l’ensemble depuis cette carte.');
          if (body.restart) await devServers.stop(card.id, serviceId);
          return devServers.startManagedService(card.id, serviceId);
        }
        const linkedProject = body.standalone ? null : store.projects().find(item => item.id === project?.linkedProjectId);
        return devServers.startManaged(card, body.standalone && project ? { ...project, linkedProjectId: '' } : project, linkedProject);
      }
      if (card.archived && ['run', 'review', 'fix-review', 'check-review', 'sync-status', 'transition', 'pull-request', 'worktree'].includes(action)) fail('Restaure cette tâche avant de reprendre le travail.', 'conflict');
      if (action === 'sync-status') {
        if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
        if (!card.ticket || !configFor(card).jiraAutoTransition || !configFor(card).jiraStatusMap?.[card.status]) fail('Configure la correspondance Jira de cette colonne dans les réglages.');
        return syncCardStatus(card.id);
      }
      if (['fix-review', 'check-review'].includes(action)) {
        if (action === 'fix-review') { manualLaunch(); checkDependencies(card); }
        if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
        const report = store.run(body.runId);
        if (!report || report.cardId !== card.id || report.kind !== 'pr-review' || report.status !== 'completed' || !report.review) fail('Review introuvable ou incomplète.');
        const instructions = action === 'fix-review' ? correctionInstructions(report, body.indexes) : '';
        publishing.add(card.id); changed();
        try {
          const context = await reviewPR(card, token('azure'));
          if (context.source !== report.reviewContext.source || context.target !== report.reviewContext.target) throw new Error('Les commits source ou cible ont changé. Relancez la review.');
          if (report.reviewContext.requirements && JSON.stringify(report.reviewContext.requirements) !== JSON.stringify(reviewRequirements(card))) throw new Error('Le besoin ou les critères ont changé. Relancez la review.');
        } catch (error) {
          report.reviewStaleReason = error.message; store.putRun(report);
          if (action === 'fix-review') throw error;
        } finally { publishing.delete(card.id); changed(); }
        if (action === 'check-review') return store.run(report.id);
        return startRun(store.card(card.id), { agent: card.agent, mode: 'edit', instructions }, null, { runId: report.id, indexes: body.indexes });
      }
      if (action === 'prepare-ticket') {
        manualLaunch();
        return startRun(card, { agent: card.agent, mode: 'analyze' }, null, null, null, true);
      }
      if (action === 'review') {
        manualLaunch(); checkDependencies(card);
        if (busy(card.id)) fail('Une opération est déjà en cours sur cette carte.', 'conflict');
        publishing.add(card.id); changed();
        let context;
        try { context = await reviewPR(card, token('azure')); }
        finally { publishing.delete(card.id); changed(); }
        return startRun(store.card(card.id), { agent: card.agent, mode: 'analyze' }, context);
      }
      if (['pr-preview', 'pull-request', 'link-pr'].includes(action)) {
        if (busy(card.id)) fail('Une exécution ou une publication est en cours sur cette carte.', 'conflict');
        if (action === 'pr-preview') {
          const preview = await previewPR(card, token('azure'));
          preview.body += checklistText(card.checklist);
          const run = store.runs().find(r => r.cardId === card.id && r.status === 'completed' && !['pr-review', 'ticket-preparation', 'brainstorm'].includes(r.kind) && r.summary);
          if (run) { preview.body += `\n\n## Compte rendu de ${run.agent} (${run.endedAt})\n${run.summary}\n\nÀ relire : ce compte rendu décrit la dernière exécution, pas les modifications ultérieures.`; preview.summaryRun = run.id; }
          return preview;
        }
        publishing.add(card.id); changed();
        try {
          if (action === 'pull-request') {
            const pr = await publishPR(card, body, token('azure'));
            const current = store.card(card.id); current.pullRequest = pr; store.putCard(current); invalidateReviews(card.id, 'Une publication a eu lieu depuis cette review. Relancez l’analyse de la PR.');
          }
          const current = store.card(card.id);
          if (!current.pullRequest) fail('Crée d’abord une PR.');
          if (current.ticket) {
            try {
              const linked = await linkPullRequestToJira(current, config(), token('jira'), requestJira);
              current.pullRequest.ticketLinked = linked !== false; delete current.pullRequest.ticketError;
            } catch (error) {
              current.pullRequest.ticketLinked = false; current.pullRequest.ticketError = error.message;
            }
          }
          store.putCard(current); return current;
        } finally { publishing.delete(card.id); changed(); }
      }
      if (['worktree', 'remove-worktree'].includes(action)) {
        if (devServers.running(card.id)) fail('Arrête les serveurs Front / Back avant de retirer le worktree.', 'conflict');
        if (busy(card.id)) fail('Arrête l’agent avant de gérer son worktree.', 'conflict');
        if (action === 'worktree') { card.worktree = prepareWorktree(card, configFor(card)); card.branch = card.worktree.branch; }
        else {
          if (body.discardChanges !== undefined && typeof body.discardChanges !== 'boolean') fail('Confirmation de suppression invalide.');
          removeApplicationWorktrees(card, body.discardChanges === true, card => store.putCard(card));
          removeWorktree(card, body.discardChanges === true); delete card.worktree;
        }
        store.putCard(card); return card;
      }
      if (action === 'run') return launchTask(card, { agent: body.agent || card.agent, model: body.model, effort: body.effort, mode: body.mode || card.mode || 'edit', instructions: body.instructions ?? card.instructions ?? '' });
      if (action === 'transitions' || action === 'transition') {
        if (!card.ticket) fail('Cette carte n’est pas liée à Jira.');
        if (card.ticket.base !== config().jiraUrl) fail('Reconnecte le site Jira de cette carte.');
        const base = `/rest/api/${config().jiraType === 'server' ? '2' : '3'}/issue/${encodeURIComponent(card.ticket.key)}`;
        if (action === 'transitions') return requestJira(config(), token('jira'), base + '/transitions');
        if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
        const id = text(body.transitionId, 100);
        publishing.add(card.id); changed();
        try {
          const allowed = await requestJira(config(), token('jira'), base + '/transitions');
          const transition = allowed.transitions?.find(t => t.id === id); if (!transition) fail('Transition Jira indisponible.');
          await requestJira(config(), token('jira'), base + '/transitions', { transition: { id } });
          card.ticket.status = transition.to.name; store.putCard(card); return card;
        } finally { publishing.delete(card.id); changed(); }
      }

    fail('Action introuvable.', 'not-found');
  }
  return { execute };
}
