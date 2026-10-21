import { msg, messageText } from './i18n.mjs';

export function confirmAction(document, message, title, label) {
  const $ = selector => document.querySelector(selector);
  const dialog = $('#confirm-dialog');
  $('#confirm-title').textContent = title;
  $('#confirm-message').textContent = message;
  $('#confirm-action').textContent = label;
  dialog.returnValue = '';
  return new Promise(resolve => {
    dialog.addEventListener('close', () => resolve(dialog.returnValue === 'confirm'), { once: true });
    dialog.showModal();
  });
}
export async function loadVersion(document, api) {
  const $ = selector => document.querySelector(selector);
  const { version } = await api('/api/health');
  $('#app-version').textContent = 'v' + version;
  $('#app-version').setAttribute('aria-label', msg("Version ") + version);
}

export function notifyCompletions(state, previousRuns, previousFlows, { document, toast, currentView, brainstormSession }) {
  const ended = state.cards.filter(card => previousFlows.get(card.id) === 'running' && card.automation?.state !== 'running');
  if (ended.length) { toast(ended.map(card => `${card.title} : ${messageText(card.automation.message)}`).join(' · '), ended.some(card => card.automation.state !== 'ready')); return; }
  const finished = state.runs.filter(run => previousRuns.get(run.id) === 'running' && ['completed', 'failed'].includes(run.status) && !(currentView === 'brainstorm' && run.kind === 'brainstorm' && run.cardId === brainstormSession));
  if (finished.length && !document.querySelector('dialog[open]')) toast(msg("{0} exécution(s) terminée(s). Résultats dans l’onglet Agents.", finished.length), finished.some(run => run.status === 'failed'));
}
