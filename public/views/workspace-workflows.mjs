import { msg, markupText, messageText } from '../i18n.mjs';
import { esc, cardPath } from '../view-helpers.mjs';
import { cleanupCandidate } from '../workflow.mjs';

export async function removeWorktree(cardId, { api, confirmAction }) {
  if (!(await confirmAction(msg("Retirer le worktree principal et les worktrees Front / Back associés, et supprimer leurs fichiers locaux, y compris les modifications non commitées et les fichiers non suivis ? Cette perte est irréversible. Les branches et leurs commits seront conservés. Aucun ticket Jira ni autre service ne sera modifié."), msg("Retirer le worktree ?"), msg("Retirer et supprimer les fichiers")))) return false;
  await api(cardPath(cardId) + '/worktree', 'DELETE', { discardChanges: true });
  return true;
}

export function createWorkspaceWorkflows({ document, getState, getWorktreeChecks, clearWorktreeChecks, api, action, confirmAction, refresh, toast, devRunning, scopedCards, bindPrActions }) {
  const $ = selector => document.querySelector(selector);
function cleanupMarkup(cards) {
  const candidates = cards.filter(cleanupCandidate);
  return `<details class="disclosure"><summary>${markupText("Nettoyer après livraison")} <span class="count">${candidates.length}</span></summary><p class="hint">${markupText("Tâches terminées, archivées ou PR fusionnées. Actualise l’état Git avant de choisir. Le retrait conserve les branches et refuse les modifications locales ; les worktrees associés sont également contrôlés.")}</p>${candidates.map(c => {
    const check = getWorktreeChecks().find(w => w.cardId === c.id), unavailable = c.publishing || getState().runs.some(r => r.cardId === c.id && r.status === 'running') || devRunning(c.id);
    return `<article class="workspace-row"><div><h3>${esc(c.title)}</h3><p class="hint">${esc(check ? check.error || (!check.coherent ? msg("Branche inattendue") : check.clean ? msg("Dossier principal propre") : msg("Modifications locales à conserver")) : msg("État Git non vérifié"))}</p></div><button class="danger" data-clean-candidate="${esc(c.id)}" ${unavailable ? 'disabled' : ''}>${markupText("Retirer sans forcer")}</button></article>`;
  }).join('') || `<p class="hint">${markupText("Aucun dossier candidat au nettoyage.")}</p>`}</details>`;
}
function queueMarkup() {
  const queue = getState().queue || { ids: [], paused: true };
  return `<section class="worktree-box"><h3>${markupText("File d’exécution globale")}</h3><p>${markupText("{0} · {1} tâche(s) · un seul agent à la fois", queue.paused ? msg("En pause") : msg("Active"), queue.ids.length)}</p><p class="hint">${markupText("Tous projets. Les réglages enregistrés de chaque carte sont utilisés à son démarrage. La pause laisse finir la tâche active ; un échec arrête la file.")}</p>${queue.message ? `<p role="status">${esc(messageText(queue.message))}</p>` : ''}<button data-queue-pause="${!queue.paused}" ${!queue.ids.length ? 'disabled' : ''}>${queue.paused ? msg("Démarrer / reprendre la file") : msg("Mettre la file en pause")}</button>${queue.ids.map((id, index) => { const card = getState().cards.find(c => c.id === id); return `<div class="workspace-row"><span>${index + 1}. ${esc(card?.title || msg("Tâche introuvable"))}${queue.current === id ? msg(" · en cours") : ''}</span><div class="row-actions"><button data-open-card="${esc(id)}">${markupText("Ouvrir la tâche")}</button><button data-dequeue="${esc(id)}" ${queue.current === id ? 'disabled' : ''}>${markupText("Retirer de la file")}</button></div></div>`; }).join('')}</section>`;
}
function bindWorkflowActions() {
  bindPrActions();
  if ($('#refresh-prs')) $('#refresh-prs').onclick = () => action(async () => {
    const button = $('#refresh-prs'); button.disabled = true; let failures = 0;
    try {
      for (const card of scopedCards(true).filter(c => c.pullRequest && !c.publishing)) {
        try { const updated = await api(cardPath(card.id) + '/refresh-pr', 'POST', {}); if (updated.pullRequest.refreshError) failures++; } catch { failures++; }
      }
      await refresh(); toast(failures ? msg("{0} PR non actualisée(s) : consulter leur message.", failures) : msg("États des PR actualisés."), Boolean(failures));
    } finally { if (button.isConnected) button.disabled = false; }
  });
  document.querySelectorAll('[data-clean-candidate]').forEach(button => button.onclick = () => action(async () => {
    if (!(await confirmAction(msg("Retirer le worktree principal et ses worktrees associés sans forcer ? Les branches sont conservées. Des modifications locales feront refuser le retrait."), msg("Nettoyer ce dossier ?"), msg("Retirer sans forcer")))) return;
    button.disabled = true;
    try { await api(cardPath(button.dataset.cleanCandidate) + '/worktree', 'DELETE', {}); clearWorktreeChecks(); await refresh(); toast(msg("Worktrees retirés, branches conservées.")); }
    finally { if (button.isConnected) button.disabled = false; }
  }));
  document.querySelectorAll('[data-queue-pause]').forEach(button => button.onclick = () => action(async () => { await api('/api/queue', 'POST', { paused: button.dataset.queuePause === 'true' }); await refresh(); }));
  document.querySelectorAll('[data-dequeue]').forEach(button => button.onclick = () => action(async () => { await api(cardPath(button.dataset.dequeue) + '/queue', 'DELETE', {}); await refresh(); }));

}
  return { cleanupMarkup, queueMarkup, bindWorkflowActions };
}
