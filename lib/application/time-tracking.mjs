import { timeFields } from '../domain/time.mjs';

export function createTimeTracking({ store, newId, changed, exporting = () => false, now = Date.now }) {
  const save = entry => { store.putTime(entry); changed(); return entry; };
  for (const entry of store.times()) {
    if (entry.runningSince) { delete entry.runningSince; entry.interrupted = true; }
    if (entry.exportState === 'sending') entry.exportState = 'uncertain';
    store.putTime(entry);
  }
  const tick = () => {
    for (const entry of store.times().filter(e => e.runningSince)) {
      const elapsed = Math.max(0, Math.floor((now() - new Date(entry.runningSince)) / 1000));
      if (!elapsed) continue;
      entry.seconds = Math.min(86400, entry.seconds + elapsed);
      entry.runningSince = new Date(new Date(entry.runningSince).getTime() + elapsed * 1000).toISOString();
      if (entry.seconds === 86400) delete entry.runningSince;
      save(entry);
    }
  };
  const fail = message => { throw new Error(message); };
  async function execute(action, id, body) {
    if (action === 'create') {
      if (!store.card(body.cardId)) fail('Carte introuvable.');
      if (store.card(body.cardId).archived) fail('Restaure cette tâche avant de saisir du temps.');
      if (body.start !== undefined && typeof body.start !== 'boolean') fail('Action de chronomètre invalide.');
      const fields = timeFields(body);
      if (body.start && store.times().some(e => e.runningSince)) fail('Arrête le chronomètre actif avant d’en démarrer un autre.');
      return save({ id: newId(), cardId: body.cardId, ...fields, seconds: body.start ? 0 : fields.seconds, ...(body.start ? { runningSince: new Date(now()).toISOString() } : {}), exportState: 'draft' });
    }
    const entry = store.time(id); if (!entry) fail('Entrée de temps introuvable.');
    if (exporting(entry.id)) fail('Envoi en cours.');
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
    fail('Action inconnue.');
  }

  return { execute, tick, close() { tick(); for (const entry of store.times().filter(e => e.runningSince)) { delete entry.runningSince; save(entry); } } };
}
