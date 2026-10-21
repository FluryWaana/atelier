import { blockers } from '../lib/domain/tasks.mjs';
export { blockers } from '../lib/domain/tasks.mjs';

export function nextAction(card, runs, cards) {
  if (card.status === 'done') return '';
  const blocked = blockers(card, cards);
  if (blocked.length) return 'Attendre : ' + blocked.map(c => c.title).join(', ');
  if (card.ticketSync?.state === 'error') return 'Réessayer la mise à jour Jira dans la fiche.';
  const flow = card.automation;
  if (flow?.state === 'blocked') return flow.acceptance?.notCovered || flow.acceptance?.unverifiable ? 'Compléter le besoin ou vérifier les critères, puis relancer la review.' : 'Consulter la review et corriger les constats bloquants.';
  if (['error', 'interrupted', 'cancelled'].includes(flow?.state)) return ({ tests: 'Consulter la sortie des vérifications, corriger puis reprendre l’étape.', publish: 'Vérifier Git et la connexion au fournisseur de dépôt, puis reprendre l’étape.', review: 'Consulter la review et son journal, puis reprendre l’étape.' })[flow.step] || 'Consulter le journal et les instructions, puis reprendre l’étape.';
  const run = runs.find(r => r.cardId === card.id);
  if (run?.attention || ['failed', 'interrupted', 'cancelled'].includes(run?.status)) return run.attention || 'Consulter le journal, adapter les instructions puis relancer.';
  if (card.pullRequest?.status === 'completed' && !card.pullRequest.refreshError && card.status !== 'done') return 'PR fusionnée : vérifier les critères puis terminer la tâche.';
  if (card.status === 'standby') return 'Préciser la raison du blocage dans les instructions avant de reprendre.';
  return '';
}

export function prLabel(pr) {
  if (pr.refreshError) return 'PR : actualisation en échec';
  if (!pr.checkedAt) return 'PR : état distant à actualiser';
  if (pr.status === 'completed') return 'PR fusionnée';
  if (pr.status === 'abandoned') return 'PR abandonnée';
  if (['conflicts', 'failure', 'rejectedByPolicy'].includes(pr.mergeStatus)) return 'PR bloquée';
  if (pr.checks?.some(c => ['failed', 'error'].includes(c.state)) || pr.reviewers?.some(r => r.vote < 0)) return 'PR : contrôles ou review en échec';
  if (pr.checksError) return 'PR : contrôles indisponibles';
  if (pr.checks?.some(c => c.state === 'pending')) return 'PR : contrôles en cours';
  return pr.draft ? 'PR brouillon' : 'PR ouverte';
}

export function cleanupCandidate(card) {
  return Boolean(card.worktree && (card.status === 'done' || card.archived || card.pullRequest?.status === 'completed' && !card.pullRequest.refreshError));
}

export function workReport(cards, runs, times, day, allCards = cards) {
  const ids = new Set(cards.map(c => c.id));
  const label = c => `${c.ticket?.key ? c.ticket.key + ' — ' : ''}${c.title}`;
  const dayOf = value => { const d = new Date(value); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
  const finished = runs.filter(r => ids.has(r.cardId) && r.endedAt && dayOf(r.endedAt) === day);
  const worked = cards.filter(c => finished.some(r => r.cardId === c.id) || times.some(t => t.cardId === c.id && t.date === day) || dayOf(c.updatedAt) === day);
  const list = values => values.length ? values.map(v => '- ' + v).join('\n') : '- Aucun';
  const seconds = times.filter(t => ids.has(t.cardId) && t.date === day).reduce((n, t) => n + t.seconds, 0);
  return `Bilan du ${day}\n\nTâches travaillées ce jour et actuellement terminées\n${list(worked.filter(c => c.status === 'done').map(label))}\n\nExécutions terminées ce jour\n${list(finished.map(r => `${label(cards.find(c => c.id === r.cardId))} : ${{ completed: 'réussie, à vérifier', failed: 'échec', interrupted: 'interrompue', cancelled: 'arrêtée' }[r.status] || r.status}`))}\n\nPR des tâches travaillées\n${list(worked.filter(c => c.pullRequest).map(c => `${label(c)} : ${c.pullRequest.url} (${prLabel(c.pullRequest)})`))}\n\nÀ reprendre / blocages actuels\n${list(cards.filter(c => !c.archived && c.status !== 'done').map(c => { const next = nextAction(c, runs, allCards); return next ? `${label(c)} : ${next}` : ''; }).filter(Boolean))}\n\nTemps humain enregistré : ${Math.floor(seconds / 3600)} h ${Math.floor(seconds % 3600 / 60)} min (hors portion active des chronomètres).`;
}
