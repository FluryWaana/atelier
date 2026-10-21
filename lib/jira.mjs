export function jiraBase(value) {
  const url = new URL(value);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) throw new Error('Adresse Jira HTTPS requise, sans identifiants ni paramètres.');
  return url.href.replace(/\/$/, '');
}

export function plainText(node) {
  if (typeof node === 'string') return node;
  if (!node) return '';
  if (node.type === 'hardBreak') return '\n';
  return (node.text || '') + (node.content || []).map(plainText).join('') + (['paragraph', 'heading', 'listItem'].includes(node.type) ? '\n' : '');
}

export async function jiraRequest(config, token, path, body, fetcher = fetch) {
  if (!config.jiraUrl || !token) throw new Error('Renseigne la connexion Jira dans les paramètres.');
  const base = jiraBase(config.jiraUrl);
  const options = {
    method: body ? 'POST' : 'GET', redirect: 'error', signal: AbortSignal.timeout(20000),
    headers: { Authorization: config.jiraType === 'server' ? `Bearer ${token}` : `Basic ${Buffer.from(`${config.jiraEmail}:${token}`).toString('base64')}`, 'Content-Type': 'application/json', Accept: 'application/json' },
    ...(body ? { body: JSON.stringify(body) } : {}),
  };
  let response = await fetcher(base + path, options);
  // Scoped Cloud API tokens require Atlassian's gateway rather than the site URL.
  if (response.status === 401 && config.jiraType !== 'server' && /^https:\/\/[a-z0-9-]+\.atlassian\.net$/i.test(base)) {
    const tenant = await fetcher(base + '/_edge/tenant_info', {
      redirect: 'error', signal: AbortSignal.timeout(20000),
      headers: { Accept: 'application/json' },
    });
    if (tenant.ok) {
      const { cloudId } = await tenant.json();
      if (typeof cloudId === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(cloudId)) {
        response = await fetcher(`https://api.atlassian.com/ex/jira/${cloudId}` + path, {
          ...options, signal: AbortSignal.timeout(20000),
        });
      }
    }
  }
  if (!response.ok) throw new Error(`Jira : erreur HTTP ${response.status}. Vérifie l’adresse, les droits et le jeton.`);
  return response.status === 204 ? {} : response.json();
}

export async function fetchIssues(config, token, request = jiraRequest) {
  const issues = [];
  let nextPageToken;
  for (let page = 0; page < 100; page++) {
    const cloud = config.jiraType !== 'server';
    const result = await request(config, token, cloud ? '/rest/api/3/search/jql' : '/rest/api/2/search', {
      jql: config.jql, maxResults: 100, fields: ['summary', 'description', 'status', 'priority'],
      ...(cloud ? (nextPageToken ? { nextPageToken } : {}) : { startAt: issues.length }),
    });
    issues.push(...(result.issues || []));
    if (cloud ? !result.nextPageToken || result.isLast : !result.issues?.length || issues.length >= result.total) return issues;
    nextPageToken = result.nextPageToken;
  }
  throw new Error('Recherche trop large : limite la requête JQL à moins de 10 000 tickets.');
}

export async function linkPullRequestToJira(card, config, token, request = jiraRequest) {
  if (!card.ticket || !card.pullRequest) throw new Error('Ticket Jira et PR nécessaires.');
  if (card.ticket.base !== config.jiraUrl) throw new Error('Reconnecte le site Jira de cette carte.');
  const pr = card.pullRequest;
  return request(config, token, `/rest/api/${config.jiraType === 'server' ? '2' : '3'}/issue/${encodeURIComponent(card.ticket.key)}/remotelink`, {
    globalId: `atelier:pull-request:${pr.url}`, relationship: 'pull request',
    object: { url: pr.url, title: `${card.ticket.key} · PR #${pr.number} — ${pr.title}` },
  });
}

export async function transitionToStatus(card, settings, token, target, request = jiraRequest) {
  if (!card.ticket || card.ticket.base !== settings.jiraUrl) throw new Error('Reconnecte le site Jira associé à cette carte.');
  const base = `/rest/api/${settings.jiraType === 'server' ? '2' : '3'}/issue/${encodeURIComponent(card.ticket.key)}`;
  const matches = status => String(status?.id) === target || status?.name?.toLocaleLowerCase() === target.toLocaleLowerCase();
  const issue = await request(settings, token, base + '?fields=status');
  if (matches(issue.fields?.status)) return issue.fields.status.name;
  const result = await request(settings, token, base + '/transitions?expand=transitions.fields');
  const candidates = (result.transitions || []).filter(t => matches(t.to));
  if (candidates.length !== 1) throw new Error(candidates.length ? 'Plusieurs transitions mènent à ce statut. Choisis la transition manuellement dans la fiche.' : `Le statut « ${target} » n’est pas accessible depuis le statut actuel du ticket.`);
  const transition = candidates[0];
  if (Object.values(transition.fields || {}).some(field => field.required && !field.hasDefaultValue)) throw new Error('Cette transition exige des champs supplémentaires. Effectue-la dans Jira.');
  await request(settings, token, base + '/transitions', { transition: { id: transition.id } });
  const updated = await request(settings, token, base + '?fields=status');
  if (!matches(updated.fields?.status)) throw new Error('Transition envoyée, mais statut non confirmé. Vérifie Jira puis réessaie.');
  return updated.fields.status.name;
}
