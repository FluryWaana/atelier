import { jiraBase } from '../jira.mjs';

export default {
  id: 'jira', name: 'Jira', apiVersion: 1, builtIn: true,
  description: 'Import et synchronisation des tickets Jira Cloud ou Data Center.',
  check: (context, input) => context.builtins.check('Jira', input),
  fields: [
    { id: 'type', label: 'Version de Jira', type: 'select', default: 'cloud', credentialScope: true, options: [{ value: 'cloud', label: 'Jira Cloud' }, { value: 'server', label: 'Jira Data Center' }] },
    { id: 'url', label: 'Adresse de Jira', type: 'url' },
    { id: 'email', label: 'E-mail (Cloud)', type: 'text', credentialScope: true },
    { id: 'token', label: 'Jeton API ou personnel', type: 'password' },
    { id: 'jql', label: 'Tickets à importer (JQL)', type: 'textarea', default: 'assignee = currentUser() AND sprint in openSprints() ORDER BY updated DESC' },
    { id: 'auto-transition', label: 'Synchroniser les colonnes avec Jira', type: 'checkbox' },
    ...Object.entries({ todo: 'À faire', doing: 'En cours', review: 'À vérifier', done: 'Terminé', standby: 'Stand by' }).map(([id, label]) => ({ id: 'status-' + id, label: 'Statut Jira : ' + label, type: 'text' })),
  ],
  validateSettings(settings) {
    if (settings.url) settings.url = jiraBase(settings.url);
    if (settings['auto-transition'] && !Object.keys(settings).some(key => key.startsWith('status-') && settings[key]?.trim())) throw new Error('Renseignez au moins un statut Jira.');
  },
  capabilities: { tickets: {
    sync: (context, input) => context.builtins.syncTickets(false, input),
    reset: (context, input) => context.builtins.syncTickets(true, input),
    syncStatus: (context, input) => context.builtins.syncTicketStatus(input.card.id),
    create: (context, input) => context.builtins.ticketWorkflow(input.card.id, 'create-jira', input.body),
    transitions: (context, input) => context.builtins.ticketWorkflow(input.card.id, 'transitions', input.body),
    transition: (context, input) => context.builtins.ticketWorkflow(input.card.id, 'transition', input.body),
    linkPullRequest: (context, input) => context.builtins.linkPullRequest(input.card),
  } },
};
