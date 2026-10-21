import { createConnectorTickets } from './lib/application/connector-tickets.mjs';
import { createConnectorTime } from './lib/application/connector-time.mjs';
import { createConnectors } from './lib/application/connectors.mjs';
import { connectorRoute } from './lib/http/connector-routes.mjs';
import { loadConnectors, loadBuiltinConnectors } from './lib/infrastructure/connector-loader.mjs';
import { createCardWorkflows } from './lib/application/card-workflows.mjs';
import { cardWorkflowRoute } from './lib/http/card-workflow-routes.mjs';
import { createSettings } from './lib/application/settings.mjs';
import { settingsRoute } from './lib/http/settings-routes.mjs';
import { createSettingsRepository } from './lib/infrastructure/settings-repository.mjs';
import { createRuns } from './lib/application/runs.mjs';
import { createJiraSync } from './lib/application/jira-sync.mjs';
import { createSyncRepository } from './lib/infrastructure/sync-repository.mjs';
import { samePath } from './lib/paths.mjs';
import { createServer } from 'node:http';
import { readFileSync, readdirSync, mkdirSync, realpathSync, statSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID, randomBytes } from 'node:crypto';
import { sameSecret, sessionProof, writeSession } from './lib/infrastructure/local-session.mjs';
import { openTokenVault } from './lib/token-vault.mjs';
import { createBackups } from './lib/backups.mjs';
import { projectSettings, settingsForProject } from './lib/project-settings.mjs';
import { openStore } from './lib/store.mjs';
import { agentStatus, startAgent, eventText, eventProgress } from './lib/agents.mjs';
import { recordAgentUsage } from './lib/agent-usage.mjs';
import { jiraBase, jiraRequest, fetchIssues, plainText, transitionToStatus, linkPullRequestToJira } from './lib/jira.mjs';
import { defaultBranch, prepareWorktree, prepareApplicationWorktrees, removeApplicationWorktrees, removeWorktree } from './lib/worktrees.mjs';
import { createDevServers } from './lib/dev-servers.mjs';
import { detectLaunch } from './lib/launch-detection.mjs';
import { previewPullRequest, publishPullRequest, refreshPullRequest } from './lib/pull-requests.mjs';
import { createTimeTracking } from './lib/time-tracking.mjs';
import { tempoRequest } from './lib/tempo.mjs';
import { checkConnections, inspectWorktrees } from './lib/diagnostics.mjs';
import { prepareReview, reviewPrompt, parseReview, correctionInstructions } from './lib/reviews.mjs';
import { checklistText } from './lib/checklist.mjs';
import { createAutomation } from './lib/automation.mjs';
import { runVerification } from './lib/verification.mjs';
import { chooseFolder } from './lib/folders.mjs';
import { blockers } from './lib/domain/tasks.mjs';
import { createProjects } from './lib/application/projects.mjs';
import { createTasks } from './lib/application/tasks.mjs';
import { createProjectRepository, createTaskRepository } from './lib/infrastructure/board-repositories.mjs';
import { projectRoute } from './lib/http/project-routes.mjs';
import { taskRoute } from './lib/http/task-routes.mjs';
import { createTaskQueue } from './lib/task-queue.mjs';
import { preparationPrompt, parsePreparation } from './lib/ticket-preparation.mjs';
import { brainstormPrompt, parseBrainstorm } from './lib/brainstorm.mjs';
import { createUpdates } from './lib/application/updates.mjs';
import { createGithubReleases } from './lib/infrastructure/github-releases.mjs';
import { createPackageUpdater } from './lib/infrastructure/package-updater.mjs';
import { dataDirectory } from './lib/infrastructure/installation.mjs';
import { updateRoute } from './lib/http/update-routes.mjs';
import { updateLock } from './scripts/update-worker.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const builtinConnectorErrors = [];
const builtinConnectorModules = await loadBuiltinConnectors(join(root, 'lib', 'connectors'), error => builtinConnectorErrors.push(error));
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
function fail(message, status = 400) { throw Object.assign(new Error(message), { status }); }
function text(value, limit = 20000) { if (typeof value !== 'string' || value.length > limit) fail('Texte invalide ou trop long.'); return value.trim(); }
function repository(value) {
  const repo = realpathSync(text(value, 2000));
  if (!statSync(repo).isDirectory()) fail('Le dépôt doit être un dossier.');
  return repo;
}

export function createApp({ builtinModules = builtinConnectorModules, connectorModules = [], connectorErrors = [], dataDir = dataDirectory(root), launch = startAgent, requestJira = jiraRequest, probe = agentStatus, previewPR = previewPullRequest, publishPR = publishPullRequest, refreshPR = refreshPullRequest, pickFolder = chooseFolder, reviewPR = prepareReview, requestTempo = tempoRequest, verify = runVerification, protectTokens, updateOptions, shutdownForUpdate, shutdown } = {}) {
  const authToken = randomBytes(32).toString('hex');
  mkdirSync(dataDir, { recursive: true, mode: 0o700 });
  const store = openStore(join(dataDir, 'kanban.sqlite'));
  const backups = createBackups(store, dataDir);
  const active = new Map();
  const publishing = new Set();
  const subscribers = new Set();
  let notification;
  function changed() {
    if (notification) return;
    notification = setTimeout(() => {
      notification = null;
      for (const res of subscribers) {
        // A stalled browser reconnects and reloads persisted state and logs.
        if (!res.write('data: changed\n\n')) { subscribers.delete(res); res.destroy(); }
      }
    }, 75);
    notification.unref();
  }
  const tokenVault = openTokenVault(store.db, protectTokens);
  const devServers = createDevServers(changed);
  let savedTokens = {};
  try { savedTokens = tokenVault.load(); }
  catch { console.warn('Jetons enregistrés illisibles avec ce compte système. Renseigne-les à nouveau dans les paramètres.'); }
  const settingsService = createSettings({ store: createSettingsRepository(store), vault: tokenVault, repository, jiraBase, platform: process.platform,
    initialTokens: { ...savedTokens, ...(!store.settings().credentialsReset ? { jira: process.env.JIRA_TOKEN || savedTokens.jira || '', azure: process.env.AZURE_DEVOPS_PAT || savedTokens.azure || '', tempo: process.env.TEMPO_TOKEN || savedTokens.tempo || '' } : {}) },
  });
  let closing = false;
  let requestsInFlight = 0;
  let choosingFolder = false;
  let integrationCache;
  for (const run of store.runs().filter(r => r.status === 'running')) {
    run.status = 'interrupted'; run.endedAt = run.lastEventAt || run.startedAt; store.putRun(run);
    const card = store.card(run.cardId);
    if (card?.status === 'doing' && !['ticket-preparation', 'brainstorm'].includes(run.kind)) { card.status = 'todo'; store.putCard(card); }
    store.log(run.id, 'system', 'Serveur arrêté pendant cette exécution. Vérifie le dépôt avant de relancer.');
  }
  const config = () => {
    const value = settingsService.config(), jira = value.connectorSettings?.jira;
    if (!jira) return value;
    return { ...value, jiraType: jira.type || 'cloud', jiraUrl: jira.url || '', jiraEmail: jira.email || '', jql: jira.jql || '',
      jiraAutoTransition: Boolean(jira['auto-transition']), jiraStatusMap: Object.fromEntries(['todo', 'doing', 'review', 'done', 'standby'].map(status => [status, jira['status-' + status] || ''])) };
  };
  const configFor = card => settingsForProject(config(), store.projects(), card.repo);
  const publicConfig = () => ({ ...settingsService.publicConfig(), ...config(), tokenSet: Boolean(settingsService.token('jira')), azureTokenSet: Boolean(settingsService.token('azure')), tempoTokenSet: Boolean(settingsService.token('tempo')) });
  const nativeRepositories = { preview: previewPR, publish: publishPR, refresh: refreshPR, review: reviewPR };
  const repositoryHandlers = Object.fromEntries(['preview', 'publish', 'refresh', 'review'].map(operation => [operation, (card, body) => operation === 'publish'
    ? nativeRepositories[operation](card, body, settingsService.token('azure')) : nativeRepositories[operation](card, settingsService.token('azure'))]));
  const connectors = createConnectors({ modules: [...builtinModules, ...connectorModules],
    errors: [...(builtinModules === builtinConnectorModules ? builtinConnectorErrors : []), ...connectorErrors],
    projectRepositoryProviders: () => store.projects().map(project => project.repositoryConnector).filter(Boolean),
    pullRequestProviders: () => store.cards().filter(card => card.pullRequest).map(card => card.pullRequest.connector || 'azure'),
    repositoryProviderFor: repo => configFor({ repo }).repositoryConnector,
    settings: settingsService.config, saveSettings: value => store.saveSettings(value), getSecret: settingsService.token, saveSecrets: settingsService.saveSecrets,
    context: connector => ({
      tasks, projects, config, changed,
      tickets: createConnectorTickets({ connector, store: createSyncRepository(store), tasks, busy, changed, timestamp, invalidateReviews }),
      builtins: { check: async (name, input = {}) => {
          const [result] = await checkConnections({ only: [name], repo: input.repo ? repository(input.repo) : config().repo, settings: config(), token: settingsService.token('jira'), azureToken: settingsService.token('azure'), tempoToken: settingsService.token('tempo'), requestJira, requestTempo, probe });
          if (!result.ok) throw new Error(result.message); return result.message;
        }, syncTicketStatus: id => ticketSync.syncCardStatus(id), ticketWorkflow: (...args) => cardWorkflows.execute(...args), linkPullRequest: card => linkPullRequestToJira(card, config(), settingsService.token('jira'), requestJira), syncTickets: reset => ticketSync.sync(reset), repositories: repositoryHandlers,
        time: (operation, input) => operation === 'preview' ? store.time(input.id) : tracking.execute(operation === 'send' ? 'export' : operation, input.id, input.body) },
      time: { get: id => store.time(id), save: entry => { store.putTime(entry); changed(); } },
    }),
  });
  const providerFor = card => card.pullRequest?.connector || (card.pullRequest ? 'azure' : configFor(card).repositoryConnector || connectors.selected('repositories'));
  const connectorTime = createConnectorTime({ store, connectors, changed });
  previewPR = card => connectors.invoke('repositories', 'preview', { card }, providerFor(card));
  reviewPR = card => connectors.invoke('repositories', 'review', { card }, providerFor(card));
  refreshPR = card => connectors.invoke('repositories', 'refresh', { card }, providerFor(card));
  publishPR = async (card, body) => {
    const provider = providerFor(card);
    return { ...await connectors.invoke('repositories', 'publish', { card, body }, provider), connector: provider };
  };
  const timestamp = () => new Date().toISOString();
  function checkDependencies(card) {
    const blocked = blockers(card, store.cards());
    if (blocked.length) fail('Tâche bloquée par : ' + blocked.map(c => c.title).join(', '), 409);
  }
  function manualLaunch() {
    if (queue.state().current || !queue.state().paused && queue.state().ids.length) fail('Mets la file en pause et attends la fin de la tâche active avant un lancement manuel.', 409);
  }
  const busy = id => automation.busy(id) || publishing.has(id) || [...active.values()].some(run => run.cardId === id);
  const tracking = createTimeTracking({ store, config, jiraToken: () => settingsService.token('jira'), tempoToken: () => settingsService.token('tempo'), requestJira, requestTempo, changed });
  function invalidateReviews(cardId, reason) {
    for (const run of store.runs().filter(r => r.cardId === cardId && r.kind === 'pr-review')) { run.reviewStaleReason = reason; store.putRun(run); }
    const card = store.card(cardId);
    if (['ready', 'blocked'].includes(card?.automation?.state)) {
      card.automation = { ...card.automation, state: 'interrupted', step: 'tests', message: reason }; store.putCard(card);
    }
  }
  const runs = createRuns({ store, active, publishing, automationBusy: id => automation.busy(id), checkDependencies, configFor, repository, samePath, prepareWorktree,
    brainstormDirectory() { const path = join(dataDir, 'brainstorm'); mkdirSync(path, { recursive: true }); return path; },
    newId: randomUUID, timestamp, launch, changed, invalidateReviews, brainstormPrompt, preparationPrompt, reviewPrompt, checklistText, parseBrainstorm, parsePreparation, parseReview, recordAgentUsage, eventProgress, eventText,
  });
  const startRun = runs.start;

  const ticketSync = createJiraSync({ store: createSyncRepository(store), config, configFor, token: () => settingsService.token('jira'), requestJira, fetchIssues, transitionToStatus, plainText, busy, publishing, changed, timestamp, invalidateReviews,
    issueId: (base, id) => 'jira-' + Buffer.from(base).toString('base64url') + '-' + id,
  });
  const ticketProvider = card => card.ticket?.connector || 'jira';
  const syncCardStatus = id => {
    const card = store.card(id), provider = ticketProvider(card);
    return card.ticket && connectors.supports(provider, 'tickets', 'syncStatus') ? connectors.invoke('tickets', 'syncStatus', { card }, provider) : card;
  };
  const linkTicketPullRequest = card => connectors.supports(ticketProvider(card), 'tickets', 'linkPullRequest')
    ? connectors.invoke('tickets', 'linkPullRequest', { card }, ticketProvider(card)) : false;

  const automation = createAutomation({
    store, settingsFor: configFor, changed, syncStatus: syncCardStatus, verify,
    preview: card => previewPR(card, settingsService.token('azure')),
    reviewContext: card => reviewPR(card, settingsService.token('azure')),
    async publish(card, options) {
      const pr = await publishPR(card, options, settingsService.token('azure'));
      const current = store.card(card.id); current.pullRequest = pr; store.putCard(current);
      invalidateReviews(card.id, 'Une publication a eu lieu depuis cette review. Relancez l’analyse de la PR.');
      if (current.ticket) {
        try { const linked = await linkTicketPullRequest(current); current.pullRequest.ticketLinked = linked !== false; }
        catch (error) { current.pullRequest.ticketError = error.message; }
        // Keep the workflow state saved while the remote request was in progress.
        const latest = store.card(card.id); latest.pullRequest = current.pullRequest; store.putCard(latest);
      }
    },
    executeAgent: (card, options, context, signal, onStarted) => new Promise((resolve, reject) => {
      let run;
      const stop = () => { const entry = run && active.get(run.id); if (entry) { entry.cancelled = true; entry.stop(); } };
      signal.addEventListener('abort', stop, { once: true });
      (async () => { try {
        signal.throwIfAborted();
        run = await startRun(store.card(card.id), { ...options, signal }, context, null, result => { signal.removeEventListener('abort', stop); resolve(result); });
        onStarted(run);
        if (signal.aborted) stop();
      } catch (error) { signal.removeEventListener('abort', stop); reject(error); } })();
    }),
  });
  function launchTask(card, options, queued = false) {
    if (!queued) manualLaunch();
    checkDependencies(card);
    if (busy(card.id)) fail('Une opération est en cours sur cette carte.', 409);
    if (card.archived) fail('Restaure cette tâche avant de lancer un agent.', 409);
    if (!['codex', 'claude'].includes(options.agent) || !['edit', 'analyze'].includes(options.mode)) fail('Agent ou mode invalide.');
    options.instructions = text(options.instructions || '');
    return configFor(card).automationEnabled && options.mode === 'edit' ? automation.start(card, options, queued && ['error', 'interrupted', 'cancelled'].includes(card.automation?.state)) : startRun(card, options);
  }

  const queue = createTaskQueue({ store, changed,
    busy: () => Boolean(automation.size() || active.size || publishing.size || ticketSync.busy() || updates.busy() || updateLock(root)),
    launch: id => {
      const card = store.card(id);
      if (!card || card.archived || card.status === 'done') fail('Tâche absente, archivée ou déjà terminée : retire-la de la file.', 409);
      return launchTask(card, { agent: card.agent, mode: card.mode || 'edit', instructions: card.instructions || '' }, true);
    },
    result: (id, execution) => execution.automation ? store.card(id)?.automation?.state === 'ready' : store.run(execution.id)?.status === 'completed' && !store.run(execution.id)?.attention,
  });

  const releaseConfig = existsSync(join(root, 'release.json')) ? JSON.parse(readFileSync(join(root, 'release.json'), 'utf8')) : {};
  const updates = createUpdates({ version, repository: releaseConfig.repository,
    releases: releaseConfig.repository ? createGithubReleases(releaseConfig.repository) : null,
    installer: shutdownForUpdate && releaseConfig.repository ? createPackageUpdater({ root, dataDir, version, repository: releaseConfig.repository, port: () => server.address().port }) : null,
    assertIdle() {
      if (requestsInFlight || choosingFolder || queue.state().current || !queue.state().paused && queue.state().ids.length || automation.size() || active.size || publishing.size || ticketSync.busy() || connectors.busy() || tracking.busy() || devServers.state().some(session => devServers.running(session.cardId)) || store.times().some(e => e.runningSince || ['sending', 'uncertain'].includes(e.exportState))) fail('Mets la file en pause, arrête les agents, les serveurs Front / Back et les chronomètres, puis termine ou vérifie les opérations en cours.', 409);
    },
    backup: () => backups.create(), changed, ...updateOptions,
  });

  const projects = createProjects({ store: createProjectRepository(store), repository, projectSettings, detectLaunch, busy, devServers,
    repositoryConnectorAvailable: id => connectors.supports(id, 'repositories', 'preview'),
    samePath,
    backup: backups.create, invalidateReviews, newId: randomUUID, timestamp,
  });
  const tasks = createTasks({ store: createTaskRepository(store), repository, config, configFor, busy,
    queuedIds: () => queue.state().ids, launchTask, syncCardStatus, invalidateReviews, newId: randomUUID, timestamp,
  });

  const cardWorkflows = createCardWorkflows({ store, manualLaunch, startRun, busy, config, configFor, token: settingsService.token, jiraBase, requestJira, publishing, changed, queue, refreshPR, invalidateReviews, automation, checkDependencies, devServers, prepareWorktree, prepareApplicationWorktrees, samePath, syncCardStatus, correctionInstructions, reviewPR, previewPR, publishPR, linkPullRequestToJira: linkTicketPullRequest, removeApplicationWorktrees, removeWorktree, launchTask, newId: randomUUID, timestamp });

  async function route(req, path, body) {
    const ticketOperation = path.match(/^\/api\/cards\/([^/]+)\/(create-ticket|transitions|transition)$/);
    if (ticketOperation && (ticketOperation[2] === 'transitions' ? req.method === 'GET' : req.method === 'POST')) {
      const card = store.card(ticketOperation[1]); if (!card) fail('Carte introuvable.', 404);
      if (busy(card.id) || card.archived) fail('Restaure la carte et attends la fin des opérations.', 409);
      const operation = ticketOperation[2] === 'create-ticket' ? 'create' : ticketOperation[2];
      return connectors.invoke('tickets', operation, { card, body }, card.ticket ? ticketProvider(card) : connectors.selected('tickets'));
    }
    const connectorResult = connectorRoute(connectors, req.method, path, body);
    if (connectorResult !== undefined) return connectorResult;
    if (['/api/tickets/sync', '/api/tickets/reset'].includes(path) && req.method === 'POST') return connectors.invoke('tickets', path.endsWith('/reset') ? 'reset' : 'sync', body);
    const timeExport = path.match(/^\/api\/time\/([^/]+)\/(export|reconcile)$/);
    if (timeExport && req.method === 'POST') return connectorTime.execute(timeExport[2] === 'export' ? 'send' : 'reconcile', timeExport[1], body);
    const timePreview = path.match(/^\/api\/time\/([^/]+)\/export-preview$/);
    if (timePreview && req.method === 'GET') {
      if (!store.time(timePreview[1])) fail('Entrée de temps introuvable.', 404);
      return connectors.invoke('time', 'preview', { id: timePreview[1], body });
    }
    const projectResult = projectRoute(projects, req.method, path, body);
    if (projectResult !== undefined) return projectResult;
    const taskResult = taskRoute(tasks, req.method, path, body);
    if (taskResult !== undefined) return taskResult;
    if (path === '/api/updates' || path.startsWith('/api/updates/')) {
      const result = await updateRoute(updates, req.method, path, body);
      const receipt = join(dataDir, 'update-result.json');
      return { ...result, applying: Boolean(updateLock(root)), result: existsSync(receipt) ? JSON.parse(readFileSync(receipt, 'utf8')) : null };
    }
    if (path === '/api/queue' && req.method === 'POST') {
      if (typeof body.paused !== 'boolean') fail('État de pause invalide.');
      return queue.pause(body.paused);
    }
    if (path === '/api/backups' && req.method === 'GET') return { directory: backups.directory, backups: backups.list() };
    if (['/api/backups', '/api/backups/restore'].includes(path) && req.method === 'POST') {
      if (devServers.state().some(session => devServers.running(session.cardId))) fail('Arrête les serveurs Front / Back avant cette opération.', 409);
      if (queue.state().current || !queue.state().paused && queue.state().ids.length || automation.size() || active.size || publishing.size || ticketSync.busy() || connectors.busy() || tracking.busy() || store.times().some(e => e.runningSince || ['sending', 'uncertain'].includes(e.exportState))) fail('Mets la file en pause, arrête les agents et chronomètres, puis termine ou vérifie les envois Tempo avant cette opération.', 409);
      if (path === '/api/backups') return backups.create();
      if (body.confirmed !== true) fail('Confirme la restauration de cette sauvegarde.');
      const result = backups.restore(body.name); queue.pause(true); settingsService.clearTokens(); return result;
    }
    if (path === '/api/health' && req.method === 'GET') return { app: 'atelier-kanban', version, workspace: root, pid: process.pid };
    if (path === '/api/shutdown' && req.method === 'POST') {
      if (!shutdown) fail('Arrêt indisponible.', 409);
      if (active.size || automation.size() || publishing.size || ticketSync.busy() || connectors.busy() || tracking.busy() || requestsInFlight > 1 || queue.state().current || store.times().some(entry => entry.runningSince || entry.exportState === 'sending')) fail('Arrête les agents et chronomètres et attends la fin des opérations.', 409);
      closing = true;
      return { ok: true };
    }
    if (path === '/api/time' || path.startsWith('/api/time/')) return tracking.route(req, path, body);
    if (path === '/api/diagnostics' && req.method === 'POST') {
      const local = await checkConnections({ only: ['Git', 'Codex', 'Claude Code'], repo: body.repo ? repository(body.repo) : config().repo, settings: config(), token: settingsService.token('jira'), azureToken: settingsService.token('azure'), tempoToken: settingsService.token('tempo'), requestJira, requestTempo, probe });
      return [...local, ...await connectors.checkConnections(body)];
    }
    if (path === '/api/worktrees' && req.method === 'GET') return inspectWorktrees(store.cards());
    if (path === '/api/dev-servers' && req.method === 'GET') return devServers.refresh();
    if (path === '/api/folders/choose' && req.method === 'POST') {
      if (choosingFolder) fail('Un sélecteur de dossiers est déjà ouvert.', 409);
      const initial = body.initial ? repository(body.initial) : root;
      choosingFolder = true;
      try {
        const selected = await pickFolder(initial);
        return { path: selected ? repository(selected) : null };
      } finally { choosingFolder = false; }
    }
    if (path === '/api/state' && req.method === 'GET') return { queue: queue.state(), devServers: devServers.state(), times: store.times(), projects: store.projects(), brainstorms: store.cards().filter(card => card.brainstormSession), cards: store.cards().filter(card => !card.brainstormSession).map(card => ({ ...card, publishing: publishing.has(card.id) || automation.busy(card.id), suggestedBranch: defaultBranch(card) })), runs: store.runs(), settings: publicConfig(), connectors: connectors.describe() };
    if (path === '/api/agents' && req.method === 'GET') {
      if (!integrationCache || Date.now() - integrationCache.time > 30000) integrationCache = { time: Date.now(), value: await Promise.all(['codex', 'claude'].map(async agent => [agent, await probe(agent)])) };
      return Object.fromEntries(integrationCache.value);
    }
    const settingsResult = settingsRoute(settingsService, req.method, path, body);
    if (settingsResult !== undefined) return settingsResult;
    const cardResult = cardWorkflowRoute(cardWorkflows, req.method, path, body);
    if (cardResult !== undefined) return cardResult;
    const logsMatch = path.match(/^\/api\/runs\/([^/]+)\/logs$/);
    if (logsMatch && req.method === 'GET') {
      const after = Number(new URL(req.url, 'http://localhost').searchParams.get('after') || 0);
      if (!Number.isSafeInteger(after) || after < 0) fail('Curseur invalide.');
      return store.logs(logsMatch[1], after);
    }
    const cancelMatch = path.match(/^\/api\/runs\/([^/]+)\/cancel$/);
    if (cancelMatch && req.method === 'POST') {
      const entry = active.get(cancelMatch[1]); if (!entry) fail('Exécution déjà terminée.', 409);
      if (automation.busy(entry.cardId)) { automation.stop(entry.cardId); return { ok: true }; }
      entry.cancelled = true;
      const run = store.run(cancelMatch[1]); run.activity = 'Arrêt demandé…'; store.putRun(run);
      entry.stop(); return { ok: true };
    }
    if (['/api/jira/sync', '/api/jira/reset'].includes(path) && req.method === 'POST') return connectors.invoke('tickets', path.endsWith('/reset') ? 'reset' : 'sync', body, 'jira');
    fail('Route introuvable.', 404);
  }
  const assets = { "/connector-availability.mjs": ["connector-availability.mjs", "text/javascript"], '/views/settings.mjs': ['views/settings.mjs', 'text/javascript'], '/views/project-settings.mjs': ['views/project-settings.mjs', 'text/javascript'], '/views/processes.mjs': ['views/processes.mjs', 'text/javascript'], '/view-helpers.mjs': ['view-helpers.mjs', 'text/javascript'], '/views/board.mjs': ['views/board.mjs', 'text/javascript'], '/views/projects.mjs': ['views/projects.mjs', 'text/javascript'], '/lib/domain/agent-options.mjs': ['../lib/domain/agent-options.mjs', 'text/javascript'], '/lib/domain/tasks.mjs': ['../lib/domain/tasks.mjs', 'text/javascript'], '/lib/domain/paths.mjs': ['../lib/domain/paths.mjs', 'text/javascript'], '/lib/domain/validation.mjs': ['../lib/domain/validation.mjs', 'text/javascript'], '/updates.mjs': ['updates.mjs', 'text/javascript'], '/workflow.mjs': ['workflow.mjs', 'text/javascript'], '/agent-usage.mjs': ['agent-usage.mjs', 'text/javascript'], '/agent-options.mjs': ['agent-options.mjs', 'text/javascript'], '/ui-helpers.mjs': ['ui-helpers.mjs', 'text/javascript'], '/': ['index.html', 'text/html'], '/app.js': ['app.js', 'text/javascript'], '/style.css': ['style.css', 'text/css'] };
  assets['/i18n.mjs'] = ['i18n.mjs', 'text/javascript'];
  assets['/atelier-kanban.png'] = ['atelier-kanban.png', 'image/png'];
  assets['/local-session.mjs'] = ['local-session.mjs', 'text/javascript'];
  for (const name of readdirSync(join(root, 'public', 'locales'))) {
    if (/^[a-zA-Z0-9-]+\.mjs$/.test(name)) assets['/locales/' + name] = ['locales/' + name, 'text/javascript'];
  }
  const server = createServer(async (req, res) => {
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'");
    const send = (status, value) => { res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' }); res.end(JSON.stringify(value)); };
    let counted = false;
    try {
      const host = req.headers.host;
      const port = server.address()?.port;
      if (![ `127.0.0.1:${port}`, `localhost:${port}` ].includes(host)) fail('Hôte interdit.', 403);
      if (req.headers.origin && req.headers.origin !== `http://${host}`) fail('Origine interdite.', 403);
      const url = new URL(req.url, `http://${host}`);
      const path = decodeURIComponent(url.pathname);
      if (path === '/api/health' && req.method === 'GET' && /^[a-f0-9]{64}$/.test(url.searchParams.get('challenge') || '')) {
        send(200, { proof: sessionProof(authToken, url.searchParams.get('challenge'), port) }); return;
      }
      if (path.startsWith('/api/')) {
        const credential = req.headers.authorization?.replace(/^Bearer /, '') || (path === '/api/events' ? url.searchParams.get('session') : '');
        if (!sameSecret(credential, authToken)) fail('Session expirée ou absente. Rouvre Atelier avec son lanceur.', 401);
      }
      const updatePath = path === '/api/updates' || path.startsWith('/api/updates/');
      if (path.startsWith('/api/') && path !== '/api/health' && !updatePath && (updates.busy() || updateLock(root))) fail('Mise à jour en cours. Attends le redémarrage.', 409);
      if (path === '/api/events' && req.method === 'GET') {
        res.writeHead(200, { 'Content-Type': 'text/event-stream', Connection: 'keep-alive', 'X-Accel-Buffering': 'no' });
        res.write('retry: 1000\ndata: connected\n\n');
        subscribers.add(res);
        const heartbeat = setInterval(() => { if (!res.write(': heartbeat\n\n')) res.destroy(); }, 15000);
        heartbeat.unref();
        res.on('close', () => { clearInterval(heartbeat); subscribers.delete(res); });
        return;
      }
      if (!path.startsWith('/api/')) {
        if (req.method !== 'GET' || !assets[path]) fail('Page introuvable.', 404);
        const [file, mime] = assets[path]; res.writeHead(200, { 'Content-Type': mime + '; charset=utf-8' }); res.end(readFileSync(join(root, 'public', file))); return;
      }
      if (closing && req.method !== 'GET') fail('Atelier est en cours d’arrêt.', 409);
      if (!updatePath && path !== '/api/health') { requestsInFlight++; counted = true; }
      let body = {};
      if (req.method !== 'GET') {
        if (req.headers['x-local-request'] !== '1' || req.headers['content-type'] !== 'application/json') fail('Requête locale JSON requise.', 403);
        let raw = ''; for await (const chunk of req) { raw += chunk; if (raw.length > 100000) fail('Requête trop volumineuse.', 413); }
        body = JSON.parse(raw || '{}'); if (!body || typeof body !== 'object' || Array.isArray(body)) fail('Objet JSON requis.');
      }
      const result = await route(req, path, body);
      if (path === '/api/shutdown' && req.method === 'POST') res.once('finish', () => { shutdown().catch(error => console.error('Arrêt :', error)); });
      if (path === '/api/updates/install' && req.method === 'POST' && shutdownForUpdate) res.once('finish', () => { shutdownForUpdate().catch(error => console.error('Arrêt pour mise à jour :', error)); });
      send(200, result);
      if (req.method !== 'GET') changed();
    } catch (error) { send(error.status || ({ 'not-found': 404, conflict: 409 })[error.code] || 400, { error: error.message }); }
    finally { if (counted) requestsInFlight--; }
  });
  async function close() {
    closing = true;
    queue.close();
    await runs.close();
    await automation.close();
    await devServers.close();
    for (const entry of active.values()) { entry.cancelled = true; entry.stop(); }
    tracking.close();
    clearTimeout(notification);
    for (const res of subscribers) res.end();
    await new Promise(resolve => server.close(resolve));
    // Child close events must finish before the database is closed.
    const deadline = Date.now() + 5000;
    while (active.size && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 50));
    if (!active.size) store.db.close();
  }
  return { server, store, close, authToken };
}

if (import.meta.main) {
  const lock = updateLock(root);
  if (lock && lock.token !== process.env.ATELIER_UPDATE_TOKEN) throw new Error('Mise à jour en cours ou interrompue. Utilise le lanceur Atelier pour reprendre.');
  const connectorErrors = [];
  const connectorModules = await loadConnectors(join(root, 'connectors'), error => connectorErrors.push(error));
  const shutdown = async () => { await app.close(); process.exit(0); };
  const app = createApp({ connectorModules, connectorErrors, shutdownForUpdate: shutdown, shutdown });
  app.server.listen(Number(process.env.PORT || 4317), '127.0.0.1', () => {
    writeSession(dataDirectory(root), app.authToken);
    console.log(`Atelier Kanban : http://127.0.0.1:${app.server.address().port} · ouvre le lanceur pour te connecter.`);
  });
  app.server.on('error', error => { console.error(error.message); process.exitCode = 1; });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, async () => { await app.close(); process.exit(); });
}
