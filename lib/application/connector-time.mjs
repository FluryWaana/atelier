import { fail } from '../domain/validation.mjs';

export function createConnectorTime({ store, connectors, changed }) {
  const pending = new Set();
  async function execute(operation, id, body = {}) {
    const entry = store.time(id);
    if (!entry) fail('Entrée de temps introuvable.', 'not-found');
    if (pending.has(id)) fail('Envoi en cours.', 'conflict');
    if (entry.exportState === 'exported') return entry;
    if (entry.runningSince) fail('Arrêtez le chronomètre avant l’export.');
    if (operation === 'send' && (body.confirmed !== true || entry.exportState !== 'draft')) fail('Confirmez l’envoi d’une saisie locale non envoyée.');
    if (operation === 'reconcile' && entry.exportState !== 'uncertain') fail('Cette saisie ne nécessite pas de rapprochement.');
    const provider = operation === 'reconcile' ? entry.exportConnector || 'tempo' : connectors.selected('time');
    pending.add(id);
    try {
      store.putTime({ ...entry, exportConnector: provider });
      return await connectors.invoke('time', operation, { id, body }, provider);
    } catch (error) {
      const current = store.time(id);
      if (operation === 'send' && current.exportState === 'sending') store.putTime({ ...current, exportState: 'uncertain', error: error.message });
      throw error;
    } finally { pending.delete(id); changed(); }
  }
  return { execute };
}
