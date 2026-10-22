import { agentName, fillAgentSelect, agentSidebarMarkup } from './agent-options.mjs';
import { scopedCards as cardsInScope } from './ui-helpers.mjs';
import { overviewMarkup } from './views/overview.mjs';
import { removeWorktree, createWorkspaceWorkflows } from './views/workspace-workflows.mjs';
import { createCardView } from './views/card.mjs';
import { createBrainstormView, createBrainstormProposals } from './views/brainstorm.mjs';
import { createTimeView } from './views/time.mjs';
import { createTaskEditor } from './views/task-editor.mjs';
import { confirmAction as showConfirmation, loadVersion, notifyCompletions } from './ui-actions.mjs';
import { messageText, msg, markupText, locale, initializeLanguage, localizeModule } from './i18n.mjs';
import { connectorSupports } from './connector-availability.mjs';
import { createSettingsView } from './views/settings.mjs';
import { createProjectSettings } from './views/project-settings.mjs';
import { createProcessesView, bindProcessActions } from './views/processes.mjs';
import { sameRepository } from '../lib/domain/paths.mjs';
import { esc, cardPath, duration, agentState } from './view-helpers.mjs';
import { createBoardView } from './views/board.mjs';
import { createProjectsView } from './views/projects.mjs';
import { taskTemplates, matchesFocus, shortcutAction, paletteItems, filterTimes, agentTime } from './ui-helpers.mjs';
import { usageText, usageTotals } from './agent-usage.mjs';
import { initUpdates } from './updates.mjs';
import { localFetch, eventsUrl } from './local-session.mjs';
initializeLanguage(document);
const $ = s => document.querySelector(s);
let state = { projects: [], cards: [], runs: [], settings: {} }, refreshing = false;
let currentView = ['settings', 'overview', 'board', 'brainstorm', 'agents', 'processes', 'worktrees', 'time', 'archives'].includes(new URLSearchParams(location.hash.slice(1)).get('view')) ? new URLSearchParams(location.hash.slice(1)).get('view') : 'overview', worktreeChecks = [], panelSignature = '';
const launchWorktrees = {};
let focusFilter = 'all', commandResults = [], commandIndex = 0;
let livePending = false;
const boardView = createBoardView({ document, window, api, action, confirmAction, refresh, openCard, toast, rerender: board });
const processView = createProcessesView({ getState: () => state, getSelection: () => launchWorktrees, api, refresh, refreshProcesses });
const { devRunning, processCards, launchProjects, projectManagedPorts, projectLaunchBlocked, processesMarkup } = processView;
const projectSettingsView = createProjectSettings({ document, getState: () => state, api, action, refresh, toast, devRunning });
const { openProjectSettings, openLaunchSettings } = projectSettingsView;
const settingsView = createSettingsView({ document, api, action, toast, refresh, confirmAction });
const projectsView = createProjectsView({ document, location, api, action, confirmAction, refresh, toast, openProjectSettings,
  onSelect(id, clearSearch = false) {
    $('#project-filter').value = id; currentView = 'board'; focusFilter = 'all';
    if (clearSearch) $('#search').value = '';
    board(); $('#projects-dialog').close();
  },
});

const taskEditor = createTaskEditor({ document, getState: () => state, localStorage, addCriterion: (...args) => cardView.addCriterion(...args) });
const { taskAgentFields, attachDraft, clearDraft } = taskEditor;
const { renderBrainstorm } = createBrainstormProposals({ document, getState: () => state, api, action, refresh, openCard, toast });
const brainstormView = createBrainstormView({ document, getState: () => state, localStorage, taskAgentFields, api, action, refresh, openCard, toast, renderBrainstorm });
const timeView = createTimeView({ document, getState: () => state, api, action, confirmAction, refresh, toast, board, scopedCards, connectorName, setView: view => { currentView = view; }, getSelectedRun: () => cardView.selectedRun });
const { openTime, updateClocks, renderTimerBanner } = timeView;
const cardView = createCardView({ document, getState: () => state, api, action, confirmAction, refresh, toast, connectorName, ticketSupports, taskAgentFields, attachDraft, clearDraft, openTime, devRunning, updateClocks, renderBrainstorm });
const { updateRun, updateAutomation, updateDevServers, prMarkup, bindPrActions } = cardView;
function openCard(id) { return cardView.openCard(id); }
function confirmAction(...args) { return showConfirmation(document, ...args); }
function renderBrainstormPage() { return brainstormView.renderBrainstormPage(); }
const { cleanupMarkup, queueMarkup, bindWorkflowActions } = createWorkspaceWorkflows({ document, getState: () => state, getWorktreeChecks: () => worktreeChecks, clearWorktreeChecks: () => { worktreeChecks = []; }, api, action, confirmAction, refresh, toast, devRunning, scopedCards, bindPrActions });
taskEditor.bind(); timeView.bind(); cardView.bind();

async function api(path, method = 'GET', body) {
  const response = await localFetch(path, { method, headers: method === 'GET' ? {} : { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  const data = await response.json(); if (response.status === 404 && data.error === 'Route introuvable.') throw new Error(msg("Fonction indisponible : redémarrez Atelier avec son lanceur, puis rechargez la page.")); if (!response.ok) throw new Error(messageText(data.error) || msg("Erreur du serveur.")); return data;
}
let toastTimer;
function toast(message, error = false) { clearTimeout(toastTimer); const host = [...document.querySelectorAll('dialog[open]')].at(-1) || document.body; host.append($('#toast'));  $('#toast').textContent = message; $('#toast').className = error ? 'error' : ''; $('#toast').hidden = false; toastTimer = setTimeout(() => $('#toast').hidden = true, error ? 15000 : 6000); }
async function action(fn) { try { await fn(); } catch (error) { toast(messageText(error.message), true); } }
function board() {
  renderWorkspace();
  const query = $('#search').value.toLowerCase();
  const project = state.projects.find(p => p.id === $('#project-filter').value);
  const sameRepo = (a, b) => sameRepository(a, b, state.settings.platform);
  const cards = state.cards.filter(c => !c.brainstormSession && !c.ticketOutOfScope && matchesFocus(c, state.runs, focusFilter, state.cards) && (!project || sameRepo(c.repo, project.repo)) && `${c.title} ${c.description} ${c.ticket?.key || ''}`.toLowerCase().includes(query));
  $('#total').textContent = cards.length;
  $('#focus-bar').hidden = currentView !== 'board' || !state.cards.length;
  const scoped = scopedCards();
  $('#review-count').textContent = scoped.filter(c => matchesFocus(c, state.runs, 'review', state.cards)).length;
  $('#attention-count').textContent = scoped.filter(c => matchesFocus(c, state.runs, 'attention', state.cards)).length;
  document.querySelectorAll('[data-focus]').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.focus === focusFilter)));
  $('#welcome-project-label').textContent = state.projects.length ? msg("{0} projet(s) prêt(s) · gérer", state.projects.length) : msg("Choisir un projet");
  renderTimerBanner();
  $('#repo-label').textContent = project ? `${project.name} · ${project.repo}` : state.settings.repo || msg("Tous vos projets · choisissez un projet pour cibler le tableau");
  const scopedIds = new Set(scopedCards().map(c => c.id));
  const running = state.runs.filter(r => r.status === 'running' && scopedIds.has(r.cardId)).length;
  $('#activity').textContent = running ? msg("{0} agent{1} au travail", running, running > 1 ? 's' : '') : msg("À votre rythme");
  $('#welcome').hidden = scopedCards().length > 0 || !['board', 'overview'].includes(currentView);
  boardView.render({ cards, state, query, focusFilter });
  updateClocks();
}
async function refresh() {
  const previousRuns = new Map(state.runs.map(run => [run.id, run.status]));
  const previousFlows = new Map(state.cards.map(card => [card.id, card.automation?.state]));
  const firstLoad = !state.settings.platform;
  state = await api('/api/state');
  document.querySelectorAll('select[name=agent]').forEach(select => fillAgentSelect(select, state.settings));
  if (state.connectors) state.connectors.modules = state.connectors.modules.map(localizeModule);
  $('#sync-button').hidden = !connectorSupports(state.connectors, 'tickets', 'sync');
  resetJiraButton.hidden = !connectorSupports(state.connectors, 'tickets', 'reset');
  $('#sync-button').textContent = msg("↻ Synchroniser · ") + connectorName('tickets');
  if (firstLoad && currentView === 'settings') settings(); renderTemplates(); renderProjects(); board(); updateDevServers(); updateAutomation();
  notifyCompletions(state, previousRuns, previousFlows, { document, toast, currentView, brainstormSession: brainstormView.session });
}
function connectorName(capability, id = state.connectors?.providers?.[capability]) {
  return state.connectors?.modules.find(module => module.id === id)?.name || id || msg("le service connecté");
}
function ticketSupports(card, operation) {
  const id = card.ticket?.connector || (card.ticket ? 'jira' : state.connectors?.providers?.tickets);
  return Boolean(state.connectors?.modules.find(module => module.id === id)?.operations?.tickets?.includes(operation));
}
function settings(section = 'general', providerId = '') {
  currentView = 'settings';
  const form = $('#settings-form');
  for (const key of ['repo', 'agent', 'baseBranch']) form.elements[key].value = state.settings[key] || (key === 'agent' ? 'codex' : '');
  board();
  action(() => settingsView.open(typeof section === 'string' ? section : 'general', providerId));
}
$('#settings-button').onclick = () => settings(); $('#welcome-settings').onclick = () => settings();
const newAgentFields = document.createElement('details'); newAgentFields.className = 'disclosure model-options';
newAgentFields.innerHTML = `<summary>${markupText("Modèle & réflexion (facultatif)")}</summary><div data-agent-fields></div>`;
$('#new-form .notice-line').before(newAgentFields);
document.querySelectorAll('select[name=agent]').forEach(select => fillAgentSelect(select, state.settings));
taskAgentFields($('#new-form'));
$('#new-dialog').addEventListener('toggle', () => { if ($('#new-dialog').open) taskAgentFields($('#new-form')); });
$('#settings-form').onsubmit = event => { event.preventDefault(); action(async () => {
  const button = event.target.querySelector('.primary'); button.disabled = true;
  try { await api('/api/settings', 'PUT', Object.fromEntries(new FormData(event.target))); await refresh(); toast(msg("Paramètres généraux enregistrés.")); }
  finally { button.disabled = false; }
}); };
$('#new-form').onsubmit = e => { e.preventDefault(); action(async () => { const button = e.target.querySelector('button.primary'); button.disabled = true; let card; try { card = await api('/api/cards', 'POST', { ...Object.fromEntries(new FormData(e.target)), status: e.target.dataset.status }); } finally { button.disabled = false; } clearDraft('new'); $('#new-dialog').close(); await refresh(); focusFilter = 'all'; $('#search').value = ''; currentView = 'board'; $('#project-filter').value = state.projects.find(p => sameRepository(p.repo, card.repo, state.settings.platform))?.id || ''; board(); await openCard(card.id); }); };
document.querySelectorAll('[data-close]').forEach(button => button.onclick = () => button.closest('dialog').close());
$('#search').oninput = board;
const resetJiraButton = document.createElement('button');
resetJiraButton.type = 'button';
resetJiraButton.textContent = msg("Actualiser le périmètre des tickets");
resetJiraButton.title = msg("Réconcilier les tickets avec le fournisseur sélectionné.");
$('#sync-button').after(resetJiraButton);
resetJiraButton.onclick = () => action(async () => {
  resetJiraButton.disabled = true;
  try {
    await api('/api/tickets/reset', 'POST', {});
    await refresh();
    toast(msg("Périmètre des tickets actualisé."));
  } finally { resetJiraButton.disabled = false; }
});
$('#sync-button').onclick = () => action(async () => {
  if ((!state.settings.connectorProviders?.tickets || state.settings.connectorProviders.tickets === 'jira') && (!state.settings.jiraUrl || !state.settings.tokenSet)) { settings('jira'); toast(msg("Renseignez la connexion Jira avant de synchroniser.")); return; }
  $('#sync-button').disabled = true;
  try { const result = await api('/api/tickets/sync', 'POST', {}); await refresh(); toast(msg("{0} ticket(s) synchronisé(s).", result.count)); } finally { $('#sync-button').disabled = false; }
});
async function loadAgents() {
  const agents = await api('/api/agents');
  $('#agents').innerHTML = agentSidebarMarkup(agents);
  $('#agents').querySelectorAll('[data-agent-account]').forEach(button => button.onclick = () => settings('agents', button.dataset.agentAccount));
}
action(() => loadVersion(document, api));
initUpdates({ api, confirm: confirmAction });
await action(refresh); action(loadAgents);
async function syncLive() {
  livePending = true;
  if (refreshing) return;
  refreshing = true;
  try {
    while (livePending) { livePending = false; await refresh(); await updateRun(); }
  } catch { $('#live-connection').textContent = msg("Actualisation interrompue · nouvelle tentative…"); setTimeout(syncLive, 1000); }
  finally { refreshing = false; }
}
const events = new EventSource(eventsUrl());
events.onmessage = () => { $('#live-connection').textContent = msg("● En direct"); $('#live-connection').className = 'connected'; syncLive(); };
events.onerror = () => { $('#live-connection').textContent = msg("Direct interrompu · reconnexion…"); $('#live-connection').className = ''; };
setInterval(updateClocks, 1000);
setInterval(() => action(loadAgents), 60000);

$('#projects-button').onclick = () => { renderProjects(); $('#projects-dialog').showModal(); };
function renderProjects() { projectsView.render(state); }
$('#project-filter').onchange = () => { focusFilter = 'all'; $('#search').value = ''; board(); };
function scopedCards(includeArchived = false) { return cardsInScope(state, $('#project-filter').value, includeArchived); }
let refreshingProcesses = false;
async function refreshProcesses() {
  if (refreshingProcesses) return;
  refreshingProcesses = true;
  try { state.devServers = await api('/api/dev-servers'); renderWorkspace(); updateDevServers(); }
  finally { refreshingProcesses = false; }
}
setInterval(() => { if (currentView === 'processes' && !document.hidden) refreshProcesses().catch(() => {}); }, 3000);
function renderWorkspace() {
  const project = state.projects.find(p => p.id === $('#project-filter').value);
  const cards = currentView === 'archives' ? scopedCards(true).filter(c => c.archived) : ['time', 'agents', 'worktrees'].includes(currentView) ? scopedCards(true) : scopedCards(), ids = new Set(cards.map(c => c.id));
  const runs = state.runs.filter(r => ids.has(r.cardId)), times = filterTimes((state.times || []).filter(e => ids.has(e.cardId)), currentView === 'time' ? timeView.filters : {});
  history.replaceState(null, '', '#' + new URLSearchParams({ ...(project ? { project: project.id } : {}), view: currentView }));
  const viewCopy = { settings: [msg("Paramètres"), msg("Réglages généraux, modules et connexions.")], brainstorm: [msg("Brainstorming"), msg("Explorez une idée. Choisissez vos prochaines tâches. Jira reste facultatif.")], overview: [msg("Vue d’ensemble"), msg("Vos prochaines actions, en un coup d’œil.")], board: [msg("Tableau des tâches"), msg("Préparez, lancez, vérifiez. Une étape à la fois.")], agents: [msg("Activité des agents"), msg("Suivez les exécutions et retrouvez leurs résultats.")], processes: [msg("Processus"), msg("Lancez et pilotez les fronts et les backs de vos projets.")], worktrees: [msg("Branches & pull requests"), msg("Retrouvez les dossiers de travail, les branches et les PR.")], time: [msg("Temps passé"), msg("Relisez vos saisies avant leur envoi au service connecté.")], archives: [msg("Archives"), msg("Retrouvez l’historique et restaurez une tâche quand vous en avez besoin.")] };
  $('#workspace-title').textContent = viewCopy[currentView][0];
  $('#overview-button').classList.toggle('nav-active', currentView === 'overview');
  $('#overview-button').setAttribute('aria-current', currentView === 'overview' ? 'page' : 'false');
  $('#workspace-subtitle').textContent = viewCopy[currentView][1];
  $('#breadcrumb').textContent = project?.name || msg("Tous les projets");
  $('#board').hidden = currentView !== 'board' || cards.length === 0;
  $('#settings-page').hidden = currentView !== 'settings';
  $('#settings-button').classList.toggle('nav-active', currentView === 'settings');
  $('#settings-button').setAttribute('aria-current', currentView === 'settings' ? 'page' : 'false');
  $('#workspace-panel').hidden = currentView === 'settings' || currentView === 'brainstorm' || currentView === 'board' || currentView === 'overview' && !cards.length;
  $('#project-summary').hidden = !project || ['brainstorm', 'settings'].includes(currentView);
  $('#brainstorm-page').hidden = currentView !== 'brainstorm';
  for (const selector of ['.toolbar', '.toolbar-actions', '.board-meta', '#create-button']) $(selector).hidden = ['brainstorm', 'settings'].includes(currentView);
  $('#brainstorm-button').classList.toggle('nav-active', currentView === 'brainstorm');
  $('#brainstorm-button').setAttribute('aria-current', currentView === 'brainstorm' ? 'page' : 'false');
  if (currentView === 'brainstorm') { renderBrainstormPage(); return; }
  $('#search').hidden = currentView !== 'board';
  document.querySelectorAll('[data-view]').forEach(b => { b.classList.toggle('selected-tab', b.dataset.view === currentView); b.setAttribute('aria-pressed', String(b.dataset.view === currentView)); });
  if (project) $('#project-summary').innerHTML = `<span><strong>${cards.length}</strong> ${markupText("tâches")}</span><span><strong>${runs.filter(r => r.status === 'running').length}</strong> ${markupText("agents actifs")}</span><span><strong>${cards.filter(c => c.worktree).length}</strong> worktrees</span><span><strong>${cards.filter(c => c.pullRequest).length}</strong> PR</span>`;
  if (currentView === 'processes') document.querySelectorAll('[data-process-log]').forEach(log => { log.textContent = (state.devServers || []).find(session => session.cardId === log.dataset.processLog)?.log || ''; });
  const signature = JSON.stringify([currentView, project?.id, cards, runs, state.queue, timeView.filters, timeView.day, currentView === 'overview' ? state.times : times, worktreeChecks, currentView === 'processes' ? [processCards(project), state.projects, (state.devServers || []).map(({ log, ...session }) => session)] : null]);
  if (signature === panelSignature || ['board', 'settings'].includes(currentView)) return;
  panelSignature = signature;
  const cardButton = card => `<button data-open-card="${esc(card.id)}" aria-label="${markupText("Ouvrir {0}", esc(card.title))}">${markupText("Ouvrir la tâche ↗")}</button>`;
  let html = '';
  if (currentView === 'overview') html = overviewMarkup(cards, runs, state, scopedCards(true));
  if (currentView === 'processes') html = processesMarkup(project);
  if (currentView === 'archives') html = `<div class="panel-heading"><div><h2>${markupText("Votre historique")}</h2><p class="hint">${markupText("Les tâches archivées, avec leurs temps, branches et journaux. Restaurer les remet dans le tableau à leur étape précédente.")}</p></div></div>` + (cards.map(card => `<article class="workspace-row archive-row"><div><span class="section-kicker">${markupText("Tâche archivée")}</span><h3>${esc(card.ticket?.key || '')} ${esc(card.title)}</h3><p class="hint">${card.worktree ? msg("Worktree conservé · visible dans Worktrees") : msg("Historique conservé")}</p></div><div class="row-actions">${cardButton(card)}<button data-restore-card="${esc(card.id)}">${markupText("Restaurer")}</button></div></article>`).join('') || `<p class="empty">${markupText("Aucune tâche archivée.")}</p>`);
  if (currentView === 'agents') {
    html = `<div class="view-stats"><div><span>${markupText("En cours")}</span><strong>${runs.filter(run => run.status === 'running').length}</strong><small>${markupText("Agents au travail")}</small></div><div><span>${markupText("Terminées")}</span><strong>${runs.filter(run => run.status === 'completed').length}</strong><small>${markupText("Exécutions à relire")}</small></div><div><span>${markupText("À examiner")}</span><strong>${runs.filter(run => ['failed', 'cancelled', 'interrupted'].includes(run.status)).length}</strong><small>${markupText("Erreurs ou interruptions")}</small></div></div><div class="panel-heading"><div><h2>${markupText("Vos exécutions")}</h2><p class="hint">${markupText("Suivez le travail en cours ou ouvrez une tâche pour consulter son résultat.")}</p></div></div><details class="view-help"><summary>${markupText("Un agent ne répond plus ?")}</summary><p class="hint">${markupText("Le silence ne prouve pas un blocage. Après un refus de permission, adaptez les instructions puis relancez depuis la tâche.")}</p></details>`;
    const usage = usageTotals(runs);
    html += `<section class="worktree-box"><h3>${markupText("Coût API approximatif et temps agent")}</h3><p><strong>${esc(usage.costUsd.toLocaleString(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 4 }))}</strong> ${markupText("sur {0} exécution(s) avec coût · Temps cumulé :", usage.measured)} <strong>${duration(agentTime(runs).reduce((sum, group) => sum + group.seconds, 0))}</strong> ${markupText("sur {0} exécution(s).", runs.length)}</p><p class="hint">${markupText("{0} exécution(s) sans coût disponible. Périmètre affiché, toutes dates. Estimation API indicative ; la facturation de votre abonnement peut différer.", usage.missing)}</p></section>`;
    html += runs.map(run => {
      const card = cards.find(c => c.id === run.cardId);
      return `<article class="workspace-row agent-run" data-status="${esc(run.status)}"><div><span class="agent-pill ${esc(run.agent)}">${esc(agentName(state.settings, run.agent, run))}</span><h3>${esc(card.title)}</h3><p><span class="state-badge" data-agent-state="${run.id}">${esc(agentState(run))}</span> <span data-clock="${run.id}"></span></p><p class="hint">${esc(run.attention || run.activity || '')}</p><p class="hint">${esc(usageText(run))}</p><code>${esc(run.branch || run.repo)}</code></div><div class="row-actions">${cardButton(card)}${run.status === 'running' ? `<button data-stop-agent="${run.id}" class="danger">${markupText("Arrêter")}</button>` : ''}</div></article>`;
    }).join('') || `<p class="empty">${markupText("Aucun agent lancé pour ce projet.")}</p>`;
  }
  if (currentView === 'worktrees') {
    html = `<div class="panel-heading"><div><h2>${markupText("Dossiers de travail")}</h2><p class="hint">${markupText("Vos branches, leurs dossiers de travail et les pull requests associées.")}</p></div><div class="row-actions"><button id="refresh-prs">${markupText("Actualiser les PR")}</button><button id="inspect-worktrees">${markupText("Actualiser l’état Git")}</button></div></div>`;
    html += cleanupMarkup(cards);
    html += cards.filter(c => !c.worktree && c.pullRequest).map(card => `<article class="workspace-row"><div><h3>${esc(card.title)}</h3>${prMarkup(card)}</div><div class="row-actions">${cardButton(card)}</div></article>`).join('');
    html += cards.filter(c => c.worktree).map(card => {
      const check = worktreeChecks.find(w => w.cardId === card.id);
      const active = card.publishing || runs.some(r => r.cardId === card.id && r.status === 'running');
      return `<article class="workspace-row worktree-row"><div><span class="section-kicker">${esc(card.ticket?.key || msg("Branche locale"))}</span><h3>${esc(card.title)}</h3><code class="branch-label">${esc(card.branch)}</code><p class="path-text resource-location">${esc(card.worktree.path)}</p>${Object.entries(card.serviceWorktrees || {}).map(([role, worktree]) => `<p class="path-text resource-location">${markupText("Worktree {0} : {1} · {2}", esc(role), esc(worktree.path), esc(worktree.branch))}</p>`).join('')}<p class="hint">${esc(check ? check.error || (!check.coherent ? msg("Branche inattendue") : check.clean ? msg("Dossier propre") : msg("Modifications locales")) : msg("État Git non vérifié"))}</p>${check?.status ? `<pre class="log">${esc(check.status)}</pre>` : ''}${prMarkup(card)}</div><div class="row-actions">${cardButton(card)}<button class="danger" data-clean-worktree="${esc(card.id)}" ${active ? 'disabled' : ''}>${markupText("Retirer le worktree")}</button></div></article>`;
    }).join('') || `<p class="empty">${markupText("Aucun worktree. Le premier sera créé au lancement d’une tâche.")}</p>`;
  }
  if (currentView === 'time') html = timeView.timeMarkup(cards, runs, times);
  if (currentView === 'agents') html = queueMarkup() + html;
  if (['overview', 'time'].includes(currentView)) html += timeView.reportMarkup();
  const openJournals = new Set([...document.querySelectorAll('[data-process-journal][open]')].map(element => element.dataset.processJournal));
  $('#workspace-panel').innerHTML = html;
  bindWorkflowActions(); timeView.bindReport();
  document.querySelectorAll('[data-launch-project]').forEach(select => select.onchange = () => { launchWorktrees[select.dataset.launchProject] = select.value; panelSignature = ''; renderWorkspace(); });
  document.querySelectorAll('[data-launch-one]').forEach(button => button.onclick = () => action(() => launchProjects([button.dataset.launchOne])));
  if ($('#start-selected-projects')) {
    $('#start-selected-projects').disabled = !state.projects.some(item => (!project || item.id === project.id) && launchWorktrees[item.id] && !projectLaunchBlocked(item.id, launchWorktrees[item.id]));
    $('#start-selected-projects').onclick = () => action(() => launchProjects(state.projects.filter(item => !project || item.id === project.id).map(item => item.id)));
  }
  document.querySelectorAll('[data-project-settings]').forEach(button => button.onclick = () => openProjectSettings(button.dataset.projectSettings));
  document.querySelectorAll('[data-process-journal]').forEach(element => { element.open = openJournals.has(element.dataset.processJournal); });
  document.querySelectorAll('[data-launch-settings]').forEach(button => button.onclick = () => openLaunchSettings(button.dataset.launchSettings));
  if ($('#refresh-processes')) $('#refresh-processes').onclick = () => action(refreshProcesses);
  bindProcessActions({ document, api, action, refresh, refreshProcesses, processCards, devRunning }, project);
  document.querySelectorAll('[data-go-view]').forEach(b => b.onclick = () => { currentView = b.dataset.goView; if (b.dataset.focusTarget) { focusFilter = b.dataset.focusTarget; $('#search').value = ''; } board(); });
  document.querySelectorAll('[data-restore-card]').forEach(b => b.onclick = () => action(async () => { await api(cardPath(b.dataset.restoreCard) + '/archive', 'POST', { archived: false }); await refresh(); toast(msg("Tâche restaurée.")); }));
  document.querySelectorAll('[data-open-card]').forEach(b => b.onclick = () => action(() => openCard(b.dataset.openCard)));
  document.querySelectorAll('[data-stop-agent]').forEach(b => b.onclick = () => action(async () => { await api(`/api/runs/${b.dataset.stopAgent}/cancel`, 'POST', {}); await refresh(); }));
  if ($('#inspect-worktrees')) $('#inspect-worktrees').onclick = () => action(async () => { const button = $('#inspect-worktrees'); button.disabled = true; try { worktreeChecks = await api('/api/worktrees'); renderWorkspace(); } finally { if (button.isConnected) button.disabled = false; } });
  document.querySelectorAll('[data-clean-worktree]').forEach(b => b.onclick = () => action(async () => { if (!await removeWorktree(b.dataset.cleanWorktree, { api, confirmAction })) return; worktreeChecks = []; await refresh(); }));
  timeView.bindEntries(cards, times);
}
document.querySelectorAll('[data-view]').forEach(b => b.onclick = () => { currentView = b.dataset.view; board(); if (currentView === 'processes') action(refreshProcesses); });
$('#diagnostics-button').onclick = () => $('#diagnostics-dialog').showModal();
$('#test-connections').onclick = () => action(async () => {
  const button = $('#test-connections'); button.disabled = true; $('#diagnostics-results').textContent = msg("Vérification en cours…");
  try {
    const repo = state.projects.find(p => p.id === $('#project-filter').value)?.repo || state.settings.repo;
    const results = await api('/api/diagnostics', 'POST', { repo });
    $('#diagnostics-results').innerHTML = results.map(r => `<article class="diagnostic"><strong>${r.ok ? '✓' : '!' } ${esc(r.name)}</strong><p class="${r.ok ? 'hint' : 'inline-error'}">${esc(messageText(r.message))}</p></article>`).join('');
  } finally { button.disabled = false; }
});
window.addEventListener('hashchange', () => {
  const params = new URLSearchParams(location.hash.slice(1));
  currentView = ['overview', 'board', 'brainstorm', 'agents', 'processes', 'worktrees', 'time', 'archives'].includes(params.get('view')) ? params.get('view') : 'overview';
  $('#project-filter').value = state.projects.some(p => p.id === params.get('project')) ? params.get('project') : '';
  board();
});


$('#welcome-projects').onclick = () => $('#projects-button').click();
$('#welcome-create').onclick = () => $('#create-button').click();
document.querySelectorAll('[data-focus]').forEach(b => b.onclick = () => { focusFilter = b.dataset.focus; board(); });
document.querySelectorAll('[data-template]').forEach(b => b.onclick = () => {
  const template = taskTemplates[b.dataset.template], form = $('#new-form');
  if (form.elements.description.value.trim() && !Object.values(taskTemplates).some(t => t.description === form.elements.description.value)) { toast(msg("Le contexte déjà saisi est conservé. Videz-le pour appliquer un modèle.")); return; }
  form.elements.templateId.value = ''; form.elements.instructions.value = ''; $('#template-criteria').textContent = '';
  form.elements.description.value = template.description; form.elements.mode.value = template.mode;
  document.querySelectorAll('[data-template]').forEach(other => other.setAttribute('aria-pressed', String(other === b)));
  form.elements.title.focus();
});
document.addEventListener('keydown', event => {
  const editing = Boolean(event.target.closest?.('input, textarea, select, [contenteditable="true"]'));
  const command = shortcutAction(event, editing, Boolean(document.querySelector('dialog[open]')));
  if (!command) return;
  event.preventDefault();
  if (command === 'palette') openCommands();
  else if (command === 'new') $('#create-button').click();
  else { currentView = 'board'; board(); $('#search').focus(); $('#search').select(); }
});

function openCommands() {
  $('#command-query').value = ''; commandIndex = 0; renderCommands(); $('#command-dialog').showModal(); $('#command-query').focus();
}
function renderCommands() {
  commandResults = paletteItems(state, $('#command-query').value); commandIndex = Math.min(commandIndex, Math.max(0, commandResults.length - 1));
  $('#command-results').innerHTML = commandResults.map((item, index) => `<button type="button" class="command-result" data-command-index="${index}" aria-current="${index === commandIndex ? 'true' : 'false'}"><span class="command-kind">${item.kind === 'project' ? 'PROJET' : item.kind === 'card' ? msg("TÂCHE") : 'ACTION'}</span><span><strong>${esc(item.title)}</strong><small>${esc(item.detail)}</small></span><span>↵</span></button>`).join('') || `<p class="empty">${markupText("Aucun résultat. Essayez un titre, un ID Jira ou un nom de projet.")}</p>`;
  $('#command-results').querySelectorAll('button').forEach(button => button.onclick = () => action(() => executeCommand(Number(button.dataset.commandIndex))));
}
async function executeCommand(index) {
  const item = commandResults[index]; if (!item) return;
  $('#command-dialog').close();
  if (item.kind === 'card') return openCard(item.id);
  if (item.kind === 'project') { $('#project-filter').value = item.id; currentView = 'overview'; focusFilter = 'all'; $('#search').value = ''; board(); return; }
  if (item.id === 'new') $('#create-button').click();
  if (item.id === 'projects') $('#projects-button').click();
  if (item.id === 'diagnostics') $('#diagnostics-button').click();
}
$('#command-button').onclick = openCommands;
$('#overview-button').onclick = () => { currentView = 'overview'; board(); };
$('#command-query').oninput = () => { commandIndex = 0; renderCommands(); };
$('#command-query').onkeydown = event => {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault(); commandIndex = Math.max(0, Math.min(commandResults.length - 1, commandIndex + (event.key === 'ArrowDown' ? 1 : -1))); renderCommands();
    $('#command-results').querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  } else if (event.key === 'Enter') { event.preventDefault(); action(() => executeCommand(commandIndex)); }
};


async function renderBackups() {
  const result = await api('/api/backups');
  $('#backups-directory').textContent = msg("Dossier : ") + result.directory;
  $('#backup-list').innerHTML = result.backups.map(b => `<article class="workspace-row"><div><strong>${esc(new Date(b.date).toLocaleString(locale))}</strong><p class="hint">${markupText("{0} Ko", Math.ceil(b.bytes / 1024))}</p></div><button data-restore-backup="${esc(b.name)}">${markupText("Restaurer…")}</button></article>`).join('') || `<p class="hint">${markupText("Aucune sauvegarde. Créez votre premier point de restauration.")}</p>`;
  $('#backup-list').querySelectorAll('[data-restore-backup]').forEach(button => button.onclick = () => action(async () => {
    if (!(await confirmAction(msg("Remplacer les données locales par cette sauvegarde ? Une copie de secours sera créée avant. Les jetons devront être ressaisis. Les fichiers Git, Jira et Tempo restent inchangés."), msg("Restaurer la sauvegarde ?"), msg("Restaurer la sauvegarde")))) return;
    button.disabled = true;
    try { await api('/api/backups/restore', 'POST', { name: button.dataset.restoreBackup, confirmed: true }); projectsView.invalidate(); boardView.invalidate(); panelSignature = ''; await refresh(); await renderBackups(); toast(msg("Sauvegarde restaurée. Les jetons sont à ressaisir.")); }
    finally { button.disabled = false; }
  }));
}
$('#backups-button').onclick = () => action(async () => { await renderBackups(); $('#backups-dialog').showModal(); });
$('#create-backup').onclick = () => action(async () => { const button = $('#create-backup'); button.disabled = true; try { await api('/api/backups', 'POST', {}); await renderBackups(); toast(msg("Sauvegarde locale créée.")); } finally { button.disabled = false; } });

function renderTemplates() {
  const select = $('#personal-template'), value = select.value;
  select.innerHTML = `<option value="">${markupText("Sans modèle personnel")}</option>` + (state.settings.taskTemplates || []).map(t => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join('');
  select.value = value;
  $('#remove-template').hidden = !select.value;
}
$('#personal-template').onchange = () => action(async () => {
  const form = $('#new-form'), template = state.settings.taskTemplates?.find(t => t.id === form.elements.templateId.value);
  $('#remove-template').hidden = !template;
  if (!template) { $('#template-criteria').textContent = ''; return; }
  if ((form.elements.description.value.trim() || form.elements.instructions.value.trim()) && !(await confirmAction(msg("Remplacer la description, les instructions et le mode saisis par ce modèle ?"), msg("Appliquer le modèle ?"), msg("Appliquer")))) { form.elements.templateId.value = ''; $('#remove-template').hidden = true; return; }
  for (const key of ['description', 'instructions', 'mode']) form.elements[key].value = template[key];
  $('#template-criteria').textContent = msg("Critères repris à la création : ") + (template.checklist.map(c => c.text).join(' · ') || 'aucun');
  form.elements.description.dispatchEvent(new Event('input', { bubbles: true }));
});
$('#remove-template').onclick = () => action(async () => {
  if (!(await confirmAction(msg("Retirer ce modèle ? Les tâches créées restent conservées."), msg("Retirer le modèle ?"), msg("Retirer")))) return;
  await api('/api/templates/' + encodeURIComponent($('#personal-template').value), 'DELETE', {}); await refresh(); $('#template-criteria').textContent = '';
});
$('#template-form').onsubmit = e => { e.preventDefault(); action(async () => {
  const button = e.target.querySelector('.primary'); button.disabled = true;
  try { await api('/api/templates', 'POST', Object.fromEntries(new FormData(e.target))); await refresh(); $('#template-dialog').close(); toast(msg("Modèle enregistré. Disponible dans Nouvelle tâche.")); }
  finally { button.disabled = false; }
}); };
