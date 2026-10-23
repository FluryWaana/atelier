import { fail, text } from '../domain/validation.mjs';
import { reviewRequirements } from '../domain/tasks.mjs';
import { checklistText } from '../domain/checklist.mjs';

export function createCardWorkflows({ store, manualLaunch, startRun, busy, publishing, changed, queue, refreshPR, invalidateReviews, automation, checkDependencies, samePath, syncCardStatus, correctionInstructions, reviewPR, previewPR, publishPR, linkTicketPullRequest, launchTask, worktrees, newId, timestamp }) {
  async function execute(id, action, body) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    if (['dev-servers', 'stop-dev-servers', 'worktree', 'remove-worktree', 'changes', 'files', 'file', 'file-diff'].includes(action)) return worktrees.execute(card, action, body);

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
      if (run.brainstormRepo !== undefined && run.brainstormRepo !== (card.repo || '') && !samePath(run.brainstormRepo, card.repo)) fail('Le projet a changé. Demande un nouveau découpage avant de créer cette tâche.', 'conflict');
      const proposal = run.proposals[index];
      const draft = { id: newId(), title: proposal.title, description: proposal.description, checklist: proposal.checklist, instructions: proposal.questions.length ? 'Questions à trancher :\n' + proposal.questions.join('\n') : '', repo: card.repo, agent: card.agent, model: card.model || '', effort: card.effort || '', mode: 'edit', status: 'todo', brainstormSource: { runId: run.id, index }, createdAt: timestamp(), updatedAt: timestamp() };
      store.putCard(draft); return draft;
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
        const status = await refreshPR(card);
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
    if (card.archived && ['run', 'review', 'fix-review', 'check-review', 'sync-status', 'transition', 'pull-request', 'worktree'].includes(action)) fail('Restaure cette tâche avant de reprendre le travail.', 'conflict');
    if (action === 'sync-status') {
      if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
      return syncCardStatus(card.id, true);
    }
    if (['fix-review', 'check-review'].includes(action)) {
      if (action === 'fix-review') { manualLaunch(); checkDependencies(card); }
      if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
      const report = store.run(body.runId);
      if (!report || report.cardId !== card.id || report.kind !== 'pr-review' || report.status !== 'completed' || !report.review) fail('Review introuvable ou incomplète.');
      const instructions = action === 'fix-review' ? correctionInstructions(report, body.indexes) : '';
      publishing.add(card.id); changed();
      try {
        const context = await reviewPR(card);
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
      try { context = await reviewPR(card); }
      finally { publishing.delete(card.id); changed(); }
      return startRun(store.card(card.id), { agent: card.agent, mode: 'analyze' }, context);
    }
    if (['pr-preview', 'pull-request', 'link-pr'].includes(action)) {
      if (busy(card.id)) fail('Une exécution ou une publication est en cours sur cette carte.', 'conflict');
      if (action === 'pr-preview') {
        const preview = await previewPR(card);
        preview.body += checklistText(card.checklist);
        const run = store.runs().find(r => r.cardId === card.id && r.status === 'completed' && !['pr-review', 'ticket-preparation', 'brainstorm'].includes(r.kind) && r.summary);
        if (run) { preview.body += `\n\n## Compte rendu de ${run.agent} (${run.endedAt})\n${run.summary}\n\nÀ relire : ce compte rendu décrit la dernière exécution, pas les modifications ultérieures.`; preview.summaryRun = run.id; }
        return preview;
      }
      publishing.add(card.id); changed();
      try {
        if (action === 'pull-request') {
          const pr = await publishPR(card, body);
          const current = store.card(card.id); current.pullRequest = pr; store.putCard(current); invalidateReviews(card.id, 'Une publication a eu lieu depuis cette review. Relancez l’analyse de la PR.');
        }
        const current = store.card(card.id);
        if (!current.pullRequest) fail('Crée d’abord une PR.');
        if (current.ticket) {
          try {
            const linked = await linkTicketPullRequest(current);
            current.pullRequest.ticketLinked = linked !== false; delete current.pullRequest.ticketError;
          } catch (error) {
            current.pullRequest.ticketLinked = false; current.pullRequest.ticketError = error.message;
          }
        }
        const latest = store.card(card.id); latest.pullRequest = current.pullRequest;
        store.putCard(latest); return latest;
      } finally { publishing.delete(card.id); changed(); }
    }
    if (action === 'run') return launchTask(card, { agent: body.agent || card.agent, model: body.model, effort: body.effort, mode: body.mode || card.mode || 'edit', instructions: body.instructions ?? card.instructions ?? '' });

    fail('Action introuvable.', 'not-found');
  }
  return { execute };
}
