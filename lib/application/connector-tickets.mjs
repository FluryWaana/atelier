import { text, fail } from '../domain/validation.mjs';
import { reviewRequirements } from '../domain/tasks.mjs';

export function createConnectorTickets({ connector, store, tasks, busy, changed, timestamp, invalidateReviews }) {
  const list = () => store.cards().filter(card => card.ticket?.connector === connector);
  function upsert(input, cardId) {
    const externalId = text(input.id, 200), title = text(input.title, 300);
    if (!externalId || !title) fail('Identifiant et titre du ticket obligatoires.');
    const description = text(input.description || '', 20000), key = text(input.key || externalId, 200), status = text(input.status || '', 100);
    let url; try { url = new URL(input.url); } catch { fail('Adresse du ticket invalide.'); }
    if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password) fail('Adresse du ticket invalide.');
    // shortcut: without instance, scope is the URL origin; shared-host tenants must supply their base URL.
    let instance;
    try { instance = new URL(input.instance === undefined ? url.origin : text(input.instance, 2000)); } catch { fail('Adresse de l’instance invalide.'); }
    if (instance.origin !== url.origin || instance.username || instance.password || instance.search || instance.hash) fail('Adresse de l’instance invalide.');
    instance = instance.href.replace(/\/+$/, '');
    const matches = ticket => ticket.connector === connector && ticket.id === externalId && (ticket.instance || new URL(ticket.url).origin) === instance;
    const imported = list().find(card => matches(card.ticket));
    const previous = cardId ? store.card(cardId) : imported;
    if (cardId && (!previous || imported && imported.id !== cardId || previous.ticket && !matches(previous.ticket))) fail('Cette carte ou ce ticket possède déjà une autre liaison.');
    if (previous && busy(previous.id)) fail('Une opération est en cours sur ce ticket.', 'conflict');
    let result;
    store.transaction(() => {
      const card = previous || tasks.create({ title, description });
      result = { ...card, title, description, updatedAt: timestamp(), ticket: { connector, instance, id: externalId, key, url: url.href, status } };
      store.putCard(result);
      if (previous && JSON.stringify(reviewRequirements(previous)) !== JSON.stringify(reviewRequirements(result))) invalidateReviews(result.id, 'Le besoin du ticket a changé. Relancez la review.');
    });
    changed();
    return result;
  }
  return { list, upsert: input => upsert(input), attach: (cardId, input) => upsert(input, cardId) };
}
