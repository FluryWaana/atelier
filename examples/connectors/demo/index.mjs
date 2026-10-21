// Copy this folder to connectors/demo, restart Atelier, then select it in Paramètres > Modules.
function sync(context) {
  context.tickets.upsert({
    id: 'first-ticket', key: 'DEMO-1', title: context.settings.title || 'Mon premier connecteur',
    description: 'Ce ticket a été importé par un module tiers, sans modifier le cœur d’Atelier.',
    url: 'https://example.org/tickets/1', status: 'Ouvert',
  });
  return { count: 1 };
}

export default {
  id: 'demo', name: 'Connecteur de démonstration', apiVersion: 1,
  description: 'Un exemple local et sans compte externe pour créer votre propre connecteur.',
  fields: [{ id: 'title', label: 'Titre du ticket de démonstration', type: 'text' }],
  capabilities: { tickets: { sync, reset: sync } },
  tabs: [{ id: 'diagnostic', label: 'Diagnostic démo', description: 'Cet onglet est déclaré par le module.', actions: [{ id: 'count', label: 'Compter les tickets importés' }] }],
  actions: { count: context => ({ tickets: context.tickets.list().length }) },
};
