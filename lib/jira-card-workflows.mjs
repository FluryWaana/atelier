import { fail, text } from './domain/validation.mjs';
import { checklistText } from './domain/checklist.mjs';
import { sameTicket } from './domain/tickets.mjs';

export function createJiraCardWorkflows({ store, busy, config, token, jiraBase, requestJira, publishing, changed, timestamp }) {
  async function execute(id, action, body) {
    const card = store.card(id); if (!card) fail('Carte introuvable.', 'not-found');
    if (action === 'create-jira') {
      if (card.ticket) return card;
      if (card.archived || busy(card.id)) fail('Restaure la carte et attends la fin des opérations.', 'conflict');
      if (card.ticketCreation && !body.existingKey && body.confirmRetry !== true) fail('Envoi précédent incertain : vérifie Jira avant de réessayer.', 'conflict');
      const settings = config(), base = jiraBase(settings.jiraUrl), apiBase = `/rest/api/${settings.jiraType === 'server' ? '2' : '3'}`;
      if (!token('jira')) fail('Renseigne la connexion Jira dans les paramètres.');
      if (card.ticketCreation && card.ticketCreation.base !== base) fail('Reconnecte le site Jira de l’envoi précédent avant de continuer.');
      let fields;
      if (!body.existingKey) {
        const project = text(body.project, 100), issueType = text(body.issueType, 100);
        if (!/^[A-Z][A-Z0-9_]*$/.test(project) || !issueType) fail('Clé de projet et type de ticket Jira requis.');
        const description = (card.description || '') + checklistText(card.checklist) + (card.instructions ? '\n\n' + card.instructions : '');
        // shortcut: seuls les champs standards sont envoyés ; ajouter les métadonnées si un projet exige des champs personnalisés.
        fields = { project: { key: project }, issuetype: { name: issueType }, summary: card.title, description: settings.jiraType === 'server' ? description : { type: 'doc', version: 1, content: description.split('\n').map(line => ({ type: 'paragraph', content: line ? [{ type: 'text', text: line }] : [] })) } };
      } else if (!/^[A-Z][A-Z0-9_]*-\d+$/.test(body.existingKey)) fail('Clé Jira invalide.');
      publishing.add(card.id); changed();
      try {
        let issue;
        if (body.existingKey) issue = await requestJira(settings, token('jira'), apiBase + '/issue/' + encodeURIComponent(body.existingKey) + '?fields=status');
        else {
          card.ticketCreation = { state: 'uncertain', base, startedAt: timestamp() }; store.putCard(card);
          issue = await requestJira(settings, token('jira'), apiBase + '/issue', { fields });
        }
        if (!/^[A-Z][A-Z0-9_]*-\d+$/.test(issue.key)) fail('Réponse Jira incertaine : vérifie le ticket dans Jira.');
        if (store.cards().some(other => other.id !== card.id && sameTicket(other.ticket, 'jira', base, issue.id))) fail('Ce ticket Jira est déjà lié à une autre carte.', 'conflict');
        card.ticket = { connector: 'jira', id: issue.id, key: issue.key, base, url: base + '/browse/' + issue.key, status: issue.fields?.status?.name || 'Créé' };
        delete card.ticketCreation; card.updatedAt = timestamp(); store.putCard(card); return card;
      } finally { publishing.delete(card.id); changed(); }
    }
    if (action === 'transitions' || action === 'transition') {
      if (!card.ticket) fail('Cette carte n’est pas liée à Jira.');
      if (card.ticket.base !== config().jiraUrl) fail('Reconnecte le site Jira de cette carte.');
      const base = `/rest/api/${config().jiraType === 'server' ? '2' : '3'}/issue/${encodeURIComponent(card.ticket.key)}`;
      if (action === 'transitions') return requestJira(config(), token('jira'), base + '/transitions');
      if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 'conflict');
      const id = text(body.transitionId, 100);
      publishing.add(card.id); changed();
      try {
        const allowed = await requestJira(config(), token('jira'), base + '/transitions');
        const transition = allowed.transitions?.find(t => t.id === id); if (!transition) fail('Transition Jira indisponible.');
        await requestJira(config(), token('jira'), base + '/transitions', { transition: { id } });
        card.ticket.status = transition.to.name; store.putCard(card); return card;
      } finally { publishing.delete(card.id); changed(); }
    }
    fail('Action introuvable.', 'not-found');
  }
  return { execute };
}
