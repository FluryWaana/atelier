import { timeFields } from './domain/time.mjs';

export function createTempoExport({ store, config, jiraToken, tempoToken, requestJira, requestTempo, findTempoEntry, changed }) {
  const exporting = new Set();
  const save = entry => { store.putTime(entry); changed(); return entry; };
  const fail = message => { throw new Error(message); };
  async function execute(action, id, body) {
    const entry = store.time(id); if (!entry) fail('Entr\u00e9e de temps introuvable.');
    if (exporting.has(id)) fail('Envoi en cours.');
    if (!['export', 'reconcile'].includes(action)) fail('Action inconnue.');
    if (entry.exportState === 'exported') return entry;
    if (entry.runningSince) fail('Arrête le chronomètre avant l’envoi.');
    const settings = config(), card = store.card(entry.cardId);
    if (settings.jiraType !== 'cloud') fail('L’envoi Tempo est disponible pour Jira Cloud. Le suivi local reste disponible.');
    if (!card?.ticket || card.ticket.base !== settings.jiraUrl) fail('Reconnecte le Jira associé à ce ticket.');
    if (!tempoToken()) fail('Renseigne le jeton Tempo Cloud dans les réglages.');
    if (action === 'export' && (body.confirmed !== true || entry.exportState !== 'draft')) fail('Valide cet envoi ou vérifie le précédent dans Tempo.');
    exporting.add(entry.id);
    try {
      if (action === 'reconcile') {
        if (entry.exportState !== 'uncertain' || entry.exportJira !== settings.jiraUrl) fail('Aucun envoi incertain pour ce site Jira.');
        const found = await findTempoEntry(tempoToken(), entry, requestTempo);
        if (!found) return save({ ...entry, error: 'Aucune écriture retrouvée. Vérifie Tempo avant de débloquer un nouvel envoi.' });
        return save({ ...entry, exportState: 'exported', tempoId: found.tempoWorklogId, error: undefined });
      }
      timeFields(entry);
      const [user, issue] = await Promise.all([
        requestJira(settings, jiraToken(), '/rest/api/3/myself'),
        requestJira(settings, jiraToken(), `/rest/api/3/issue/${encodeURIComponent(card.ticket.key)}?fields=summary`),
      ]);
      if (!user.accountId || !/^\d+$/.test(String(issue.id))) fail('Identifiants Jira manquants.');
      entry.exportPayload = { issueId: Number(issue.id), authorAccountId: user.accountId, timeSpentSeconds: entry.seconds, startDate: entry.date, startTime: entry.startTime, description: `${entry.description}\n[atelier:${entry.id}]` };
      entry.exportJira = settings.jiraUrl; entry.exportState = 'sending'; save(entry);
      try {
        const result = await requestTempo(tempoToken(), '/worklogs', entry.exportPayload);
        if (!Number.isSafeInteger(result.tempoWorklogId)) throw new Error('Réponse Tempo inattendue. Vérifie l’envoi.');
        return save({ ...entry, exportState: 'exported', tempoId: result.tempoWorklogId, error: undefined });
      } catch (error) {
        const rejected = error.status >= 400 && error.status < 500 && error.status !== 408;
        return save({ ...entry, exportState: rejected ? 'draft' : 'uncertain', error: error.message });
      }
    } finally { exporting.delete(entry.id); }
  }
  return { execute, busy: id => id === undefined ? exporting.size > 0 : exporting.has(id) };
}
