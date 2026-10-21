import { messageText, msg } from './i18n.mjs';
import { blockers } from '../lib/domain/tasks.mjs';
export { blockers } from '../lib/domain/tasks.mjs';

export function nextAction(card, runs, cards) {
  if (card.status === 'done') return '';
  const blocked = blockers(card, cards);
  if (blocked.length) return msg("Attendre : ") + blocked.map(c => c.title).join(', ');
  if (card.ticketSync?.state === 'error') return msg("Réessayer la mise à jour Jira dans la fiche.");
  const flow = card.automation;
  if (flow?.state === 'blocked') return flow.acceptance?.notCovered || flow.acceptance?.unverifiable ? msg("Compléter le besoin ou vérifier les critères, puis relancer la review.") : msg("Consulter la review et corriger les constats bloquants.");
  if (['error', 'interrupted', 'cancelled'].includes(flow?.state)) return ({ tests: msg("Consulter la sortie des vérifications, corriger puis reprendre l’étape."), publish: msg("Vérifier Git et la connexion au fournisseur de dépôt, puis reprendre l’étape."), review: msg("Consulter la review et son journal, puis reprendre l’étape.") })[flow.step] || msg("Consulter le journal et les instructions, puis reprendre l’étape.");
  const run = runs.find(r => r.cardId === card.id);
  if (run?.attention || ['failed', 'interrupted', 'cancelled'].includes(run?.status)) return messageText(run.attention) || msg("Consulter le journal, adapter les instructions puis relancer.");
  if (card.pullRequest?.status === 'completed' && !card.pullRequest.refreshError && card.status !== 'done') return msg("PR fusionnée : vérifier les critères puis terminer la tâche.");
  if (card.status === 'standby') return msg("Préciser la raison du blocage dans les instructions avant de reprendre.");
  return '';
}

export function prLabel(pr) {
  if (pr.refreshError) return msg("PR : actualisation en échec");
  if (!pr.checkedAt) return msg("PR : état distant à actualiser");
  if (pr.status === 'completed') return msg("PR fusionnée");
  if (pr.status === 'abandoned') return msg("PR abandonnée");
  if (['conflicts', 'failure', 'rejectedByPolicy'].includes(pr.mergeStatus)) return msg("PR bloquée");
  if (pr.checks?.some(c => ['failed', 'error'].includes(c.state)) || pr.reviewers?.some(r => r.vote < 0)) return msg("PR : contrôles ou review en échec");
  if (pr.checksError) return msg("PR : contrôles indisponibles");
  if (pr.checks?.some(c => c.state === 'pending')) return msg("PR : contrôles en cours");
  return pr.draft ? msg("PR brouillon") : msg("PR ouverte");
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
  const list = values => values.length ? values.map(v => '- ' + v).join('\n') : msg("- Aucun");
  const seconds = times.filter(t => ids.has(t.cardId) && t.date === day).reduce((n, t) => n + t.seconds, 0);
  return msg("Bilan du {0}\n\nTâches travaillées ce jour et actuellement terminées\n{1}\n\nExécutions terminées ce jour\n{2}\n\nPR des tâches travaillées\n{3}\n\nÀ reprendre / blocages actuels\n{4}\n\nTemps humain enregistré : {5} h {6} min (hors portion active des chronomètres).", day, list(worked.filter(c => c.status === 'done').map(label)), list(finished.map(r => `${label(cards.find(c => c.id === r.cardId))} : ${{ completed: msg("réussie, à vérifier"), failed: msg("échec"), interrupted: 'interrompue', cancelled: msg("arrêtée") }[r.status] || r.status}`)), list(worked.filter(c => c.pullRequest).map(c => `${label(c)} : ${c.pullRequest.url} (${prLabel(c.pullRequest)})`)), list(cards.filter(c => !c.archived && c.status !== 'done').map(c => { const next = nextAction(c, runs, allCards); return next ? `${label(c)} : ${next}` : ''; }).filter(Boolean)), Math.floor(seconds / 3600), Math.floor(seconds % 3600 / 60));
}
