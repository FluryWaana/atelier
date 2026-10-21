import { timeFields } from '../domain/time.mjs';

export function createTimeTracking({ store, config, jiraToken, tempoToken, requestJira, requestTempo, findTempoEntry, newId, changed }) {
  const exporting = new Set();
  const save = entry => { store.putTime(entry); changed(); return entry; };
  for (const entry of store.times()) {
    if (entry.runningSince) { delete entry.runningSince; entry.interrupted = true; }
    if (entry.exportState === 'sending') entry.exportState = 'uncertain';
    store.putTime(entry);
  }
  const tick = () => {
    for (const entry of store.times().filter(e => e.runningSince)) {
      const elapsed = Math.max(0, Math.floor((Date.now() - new Date(entry.runningSince)) / 1000));
      if (!elapsed) continue;
      entry.seconds = Math.min(86400, entry.seconds + elapsed);
      entry.runningSince = new Date(new Date(entry.runningSince).getTime() + elapsed * 1000).toISOString();
      if (entry.seconds === 86400) delete entry.runningSince;
      save(entry);
    }
  };
  const clock = setInterval(tick, 5000); clock.unref();
  const fail = message => { throw new Error(message); };
  async function execute(action, id, body) {
    if (action === 'create') {
      if (!store.card(body.cardId)) fail('Carte introuvable.');
      if (store.card(body.cardId).archived) fail('Restaure cette tâche avant de saisir du temps.');
      if (body.start !== undefined && typeof body.start !== 'boolean') fail('Action de chronomètre invalide.');
      const fields = timeFields(body);
      if (body.start && store.times().some(e => e.runningSince)) fail('Arrête le chronomètre actif avant d’en démarrer un autre.');
      return save({ id: newId(), cardId: body.cardId, ...fields, seconds: body.start ? 0 : fields.seconds, ...(body.start ? { runningSince: new Date().toISOString() } : {}), exportState: 'draft' });
    }
    const entry = store.time(id); if (!entry) fail('Entrée de temps introuvable.');
    if (exporting.has(entry.id)) fail('Envoi en cours.');
    if (action === 'stop') {
      tick(); const current = store.time(entry.id); delete current.runningSince; return save(current);
    }
    if (['update', 'remove'].includes(action)) {
      if (entry.runningSince || entry.exportState !== 'draft') fail('Arrête le chronomètre. Seules les entrées non envoyées sont modifiables.');
      if (action === 'remove') { store.deleteTime(entry.id); changed(); return { ok: true }; }
      return save({ ...entry, ...timeFields(body), error: undefined, interrupted: false });
    }
    if (action === 'reset-export') {
      if (entry.exportState !== 'uncertain' || body.confirmed !== true) fail('Vérifie d’abord dans Tempo que cette entrée n’existe pas.');
      return save({ ...entry, exportState: 'draft', exportPayload: undefined, exportJira: undefined, error: undefined });
    }
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
  return { execute, busy: () => exporting.size > 0, close() { clearInterval(clock); tick(); for (const entry of store.times().filter(e => e.runningSince)) { delete entry.runningSince; save(entry); } } };
}
