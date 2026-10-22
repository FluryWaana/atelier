import { fail, text } from '../domain/validation.mjs';
import { statuses } from '../domain/tasks.mjs';
import { agentOptions, agentProvider, agentProviders } from '../domain/agent-options.mjs';

export function createSettings({ store, vault, repository, jiraBase, initialTokens, platform }) {
  let tokens = { ...initialTokens };
  const config = () => ({ repo: '', agent: 'codex', baseBranch: 'HEAD', jiraType: 'cloud', jiraUrl: '', jiraEmail: '', jql: 'assignee = currentUser() AND sprint in openSprints() ORDER BY updated DESC', jiraAutoTransition: false, jiraStatusMap: {}, ...store.settings() });
  const publicConfig = () => ({ ...config(), platform, tokenSet: Boolean(tokens.jira), azureTokenSet: Boolean(tokens.azure), tempoTokenSet: Boolean(tokens.tempo) });
  function removeToken(service) {
    if (!['jira', 'azure', 'tempo'].includes(service)) fail('Service inconnu.', 'not-found');
    const next = { ...tokens, [service]: '' };
    vault.save(next); tokens = next;
    return publicConfig();
  }
  function update(body) {
    const supplied = body;
    const previous = config();
    const newMapping = body.jiraStatusMap !== undefined && statuses.some(status => (body.jiraStatusMap?.[status] || '') !== (previous.jiraStatusMap?.[status] || ''));
    body = { ...previous, ...body };
    const next = { ...previous, taskTemplates: previous.taskTemplates || [], taskQueue: previous.taskQueue, repo: body.repo ? repository(body.repo) : '', jiraType: body.jiraType, jiraUrl: body.jiraUrl ? jiraBase(text(body.jiraUrl, 2000)) : '', jiraEmail: text(body.jiraEmail, 300), jql: text(body.jql, 4000) };
    next.agent = body.agent ?? previous.agent ?? 'codex';
    agentProvider(previous, next.agent);
    for (const { id: agent, model, effort } of agentProviders(previous)) {
      const options = agentOptions(agent, { model: supplied[agent + 'Model'] ?? model, effort: supplied[agent + 'Effort'] ?? effort }, previous);
      next[agent + 'Model'] = options.model; next[agent + 'Effort'] = options.effort;
      if (previous.agentProviders) next.agentProviders = (next.agentProviders || previous.agentProviders).map(provider => provider.id === agent ? { ...provider, ...options } : provider);
    }
    next.baseBranch = text(body.baseBranch ?? previous.baseBranch, 180) || 'HEAD';
    next.jiraAutoTransition = body.jiraAutoTransition ?? previous.jiraAutoTransition;
    if (typeof next.jiraAutoTransition !== 'boolean') fail('Option de synchronisation invalide.');
    const mapping = body.jiraStatusMap ?? previous.jiraStatusMap;
    if (!mapping || typeof mapping !== 'object' || Array.isArray(mapping) || Object.keys(mapping).some(key => !statuses.includes(key))) fail('Correspondance Jira invalide.');
    next.jiraStatusMap = Object.fromEntries(statuses.map(status => [status, text(mapping[status] ?? '', 100)]));
    if (previous.jiraUrl && next.jiraUrl !== previous.jiraUrl && !newMapping) { next.jiraAutoTransition = false; next.jiraStatusMap = {}; }
    if (next.jiraAutoTransition && !Object.values(next.jiraStatusMap).some(Boolean)) fail('Renseigne au moins un statut Jira cible.');
    if (!['cloud', 'server'].includes(next.jiraType)) fail('Type de Jira invalide.');
    const credentials = { ...tokens };
    if (next.jiraUrl !== previous.jiraUrl || next.jiraEmail !== previous.jiraEmail || next.jiraType !== previous.jiraType) credentials.jira = '';
    for (const [service, field, clear] of [['jira', 'token', 'clearToken'], ['azure', 'azureToken', 'clearAzureToken'], ['tempo', 'tempoToken', 'clearTempoToken']]) {
      if (body[field]) credentials[service] = text(body[field], 4000);
      if (body[clear]) credentials[service] = '';
    }
    if (next.jiraUrl !== previous.jiraUrl && !body.tempoToken) credentials.tempo = '';
    store.transaction(() => { vault.save(credentials); store.saveSettings(next); });
    tokens = credentials;
    return publicConfig();
  }
  function saveSecrets(values, save = () => {}) {
    const next = { ...tokens, ...values };
    store.transaction(() => { vault.save(next); save(); });
    tokens = next;
  }
  return { config, publicConfig, update, removeToken, saveSecrets, token: service => tokens['connector:' + service + ':token'] ?? tokens[service] ?? '', clearTokens() { tokens = {}; } };
}
