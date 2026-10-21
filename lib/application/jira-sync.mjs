import { fail } from '../domain/validation.mjs';
import { statuses, reviewRequirements } from '../domain/tasks.mjs';

export function createJiraSync({ store, config, configFor, token, requestJira, fetchIssues, transitionToStatus, plainText, busy, publishing, changed, timestamp, invalidateReviews, issueId }) {
  let syncing = false;
  async function syncCardStatus(id) {
    const card = store.card(id), settings = configFor(card), target = settings.jiraStatusMap?.[card.status];
    if (!card.ticket || !settings.jiraAutoTransition || !target) return card;
    publishing.add(id); changed();
    const sync = { localStatus: card.status, target, checkedAt: timestamp() };
    try {
      const status = await transitionToStatus(card, settings, token(), target, requestJira);
      const current = store.card(id);
      current.ticket.status = status; current.ticketSync = { ...sync, state: 'synced' }; store.putCard(current);
    } catch (error) {
      const current = store.card(id); current.ticketSync = { ...sync, state: 'error', message: error.message }; store.putCard(current);
    } finally { publishing.delete(id); changed(); }
    return store.card(id);
  }


  async function sync(reset = false) {
      if (syncing) fail('Synchronisation en cours.', 'conflict');
      syncing = true;
      try {
        const settings = config();
        const issues = await fetchIssues(settings, token(), requestJira);
        store.transaction(() => {
          if (reset) {
            const keys = new Set(issues.map(issue => issue.key));
            const base = settings.jiraUrl.replace(/\/$/, '');
            for (const card of store.cards()) {
              if (card.ticket?.base?.replace(/\/$/, '') === base && !keys.has(card.ticket.key)) {
                store.putCard({ ...card, ticketOutOfScope: true });
              }
            }
          }
          const startedCards = new Set(store.runs().map(run => run.cardId));
          for (const issue of issues) {
            const id = issueId(settings.jiraUrl, issue.id);
            const old = store.card(id);
            if (old) old.ticketOutOfScope = false;
            if (old && !old.repo && !old.worktree && !old.agentExplicit && !old.model && !old.effort && !startedCards.has(id)) old.agent = settings.agent;
            const projectConfig = configFor({ repo: old?.repo || settings.repo });
            const jiraStatus = issue.fields.status;
            const columns = projectConfig.jiraAutoTransition ? statuses.filter(column => {
              const target = projectConfig.jiraStatusMap?.[column];
              return target && (target === jiraStatus?.id || target.toLocaleLowerCase() === jiraStatus?.name?.toLocaleLowerCase());
            }) : [];
            const status = columns.length === 1 ? columns[0] : jiraStatus?.statusCategory?.key === 'done' ? 'done' : (old?.status || 'todo');
            store.putCard({ ...old, id, title: issue.fields.summary, description: plainText(issue.fields.description), repo: old?.repo || settings.repo, agent: old?.agent || projectConfig.agent, status: busy(id) ? old.status : status, createdAt: old?.createdAt || timestamp(), updatedAt: timestamp(), ticket: { connector: 'jira', id: String(issue.id), key: issue.key, base: settings.jiraUrl, status: jiraStatus?.name || '', url: `${settings.jiraUrl}/browse/${encodeURIComponent(issue.key)}` } });
            if (old && JSON.stringify(reviewRequirements(old)) !== JSON.stringify(reviewRequirements(store.card(id)))) invalidateReviews(id, 'Le besoin Jira a changé. Relancez la review.');
          }
        });
        return { count: issues.length };
      } finally { syncing = false; }

  }
  return { sync, syncCardStatus, busy: () => syncing };
}
