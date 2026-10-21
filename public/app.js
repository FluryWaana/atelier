import { connectorSupports, repositoryProvider } from './connector-availability.mjs';
import { createSettingsView } from './views/settings.mjs';
import { createProjectSettings } from './views/project-settings.mjs';
import { createProcessesView } from './views/processes.mjs';
import { sameRepository } from '../lib/domain/paths.mjs';
import { names, runNames, esc, cardPath } from './view-helpers.mjs';
import { createBoardView } from './views/board.mjs';
import { createProjectsView } from './views/projects.mjs';
import { taskTemplates, matchesFocus, shortcutAction, paletteItems, filterTimes, timeCsv, localDay, dailyTime, agentTime } from './ui-helpers.mjs';
import { effortLevels } from './agent-options.mjs';
import { usageText, usageTotals } from './agent-usage.mjs';
import { blockers, nextAction, prLabel, cleanupCandidate, workReport } from './workflow.mjs';
import { initUpdates } from './updates.mjs';
const $ = s => document.querySelector(s);
let state = { projects: [], cards: [], runs: [], settings: {} }, selected = null, selectedRun = null, cursor = 0, logRun = null, refreshing = false, wasRunning = false;
let currentView = ['settings', 'overview', 'board', 'brainstorm', 'agents', 'processes', 'worktrees', 'time', 'archives'].includes(new URLSearchParams(location.hash.slice(1)).get('view')) ? new URLSearchParams(location.hash.slice(1)).get('view') : 'overview', timeEdit = null, tempoEntry = null, worktreeChecks = [], panelSignature = '';
let summaryDay = localDay();
let timeFilters = { from: '', to: '', status: 'all' };
const launchWorktrees = {};
let brainstormSession = '', brainstormInitialized = false, brainstormingBusy = false;
let focusFilter = 'all', commandResults = [], commandIndex = 0;
let livePending = false, prPreview = null, prCardId = null;
const boardView = createBoardView({ document, window, api, action, confirmAction, refresh, openCard, toast, rerender: board });
const processView = createProcessesView({ getState: () => state, getSelection: () => launchWorktrees, api, refresh, refreshProcesses });
const { devRunning, processCards, launchProjects, projectManagedPorts, projectLaunchBlocked, processesMarkup } = processView;
const projectSettingsView = createProjectSettings({ document, getState: () => state, api, action, refresh, toast, devRunning });
const { openProjectSettings, openLaunchSettings } = projectSettingsView;
const settingsView = createSettingsView({ document, api, action, toast, refresh });
const projectsView = createProjectsView({ document, location, api, action, confirmAction, refresh, toast, openProjectSettings,
  onSelect(id, clearSearch = false) {
    $('#project-filter').value = id; currentView = 'board'; focusFilter = 'all';
    if (clearSearch) $('#search').value = '';
    board(); $('#projects-dialog').close();
  },
});
function agentFields(agent, options = {}, prefix = '', inherited = {}, listId = prefix + 'Model-choices') {
  const modelName = prefix ? prefix + 'Model' : 'model', effortName = prefix ? prefix + 'Effort' : 'effort';
  const models = agent === 'claude' ? ['sonnet', 'opus', 'haiku'] : ['gpt-5.4', 'gpt-5.3-codex', 'gpt-5.1-codex-mini'];
  return `<div class="form-row"><label>Modèle<input name="${modelName}" list="${listId}" maxlength="180" value="${esc(options.model || '')}" placeholder="${esc(inherited.model || 'Configuration CLI')}"><datalist id="${listId}">${models.map(model => `<option value="${model}"></option>`).join('')}</datalist></label><label>Niveau de réflexion<select name="${effortName}"><option value="">${esc(inherited.effort ? 'Réglage général : ' + effortLevels[agent][inherited.effort] : 'Automatique (CLI)')}</option>${Object.entries(effortLevels[agent]).map(([value, label]) => `<option value="${value}" ${options.effort === value ? 'selected' : ''}>${label}</option>`).join('')}</select></label></div>`;
}
function taskAgentFields(form, options = {}) {
  const agent = form.elements.agent.value;
  form.querySelector('[data-agent-fields]').innerHTML = '<p class="section-kicker">Modèle & réflexion</p>' + agentFields(agent, options, '', { model: state.settings[agent + 'Model'], effort: state.settings[agent + 'Effort'] }, form.id + '-models');
  form.elements.agent.onchange = () => taskAgentFields(form);
}
async function api(path, method = 'GET', body) {
  const response = await fetch(path, { method, headers: method === 'GET' ? {} : { 'Content-Type': 'application/json', 'X-Local-Request': '1' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  const data = await response.json(); if (response.status === 404 && data.error === 'Route introuvable.') throw new Error('Fonction indisponible : redémarrez Atelier avec son lanceur, puis rechargez la page.'); if (!response.ok) throw new Error(data.error || 'Erreur du serveur.'); return data;
}
let toastTimer;
function toast(message, error = false) { clearTimeout(toastTimer); const host = [...document.querySelectorAll('dialog[open]')].at(-1) || document.body; host.append($('#toast'));  $('#toast').textContent = message; $('#toast').className = error ? 'error' : ''; $('#toast').hidden = false; toastTimer = setTimeout(() => $('#toast').hidden = true, error ? 15000 : 6000); }
async function action(fn) { try { await fn(); } catch (error) { toast(error.message, true); } }
function confirmAction(message, title, label) {
  const dialog = $('#confirm-dialog');
  $('#confirm-title').textContent = title;
  $('#confirm-message').textContent = message;
  $('#confirm-action').textContent = label;
  dialog.returnValue = '';
  return new Promise(resolve => {
    dialog.addEventListener('close', () => resolve(dialog.returnValue === 'confirm'), { once: true });
    dialog.showModal();
  });
}
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
  $('#welcome-project-label').textContent = state.projects.length ? `${state.projects.length} projet(s) prêt(s) · gérer` : 'Choisir un projet';
  renderTimerBanner();
  $('#repo-label').textContent = project ? `${project.name} · ${project.repo}` : state.settings.repo || 'Tous vos projets · choisissez un projet pour cibler le tableau';
  const scopedIds = new Set(scopedCards().map(c => c.id));
  const running = state.runs.filter(r => r.status === 'running' && scopedIds.has(r.cardId)).length;
  $('#activity').textContent = running ? `${running} agent${running > 1 ? 's' : ''} au travail` : 'À votre rythme';
  $('#welcome').hidden = scopedCards().length > 0 || !['board', 'overview'].includes(currentView);
  boardView.render({ cards, state, query, focusFilter });
  updateClocks();
}
function elapsed(start, end = Date.now()) {
  const seconds = Math.max(0, Math.floor((new Date(end) - new Date(start)) / 1000));
  return seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min ${String(seconds % 60).padStart(2, '0')} s`;
}
function updateClocks() {
  if ($('#human-total')) { const ids = new Set(scopedCards(true).map(c => c.id)); $('#human-total').textContent = duration(filterTimes((state.times || []).filter(e => ids.has(e.cardId)), timeFilters).reduce((sum, e) => sum + e.seconds + (e.runningSince ? Math.max(0, Math.floor((Date.now() - new Date(e.runningSince)) / 1000)) : 0), 0)); }
  const now = Date.now();
  const agentTimes = new Map(agentTime(state.runs, now).map(g => [g.cardId, g]));
  const periodTimes = $('#agent-total') ? new Map(agentTime(state.runs, now, timeFilters).map(g => [g.cardId, g])) : agentTimes;
  if ($('#agent-total')) { const ids = new Set(scopedCards(true).map(c => c.id)); $('#agent-total').textContent = duration([...periodTimes.values()].filter(g => ids.has(g.cardId)).reduce((sum, g) => sum + g.seconds, 0)); }
  document.querySelectorAll('[data-agent-time]').forEach(el => { el.textContent = duration((el.hasAttribute('data-agent-period') ? periodTimes : agentTimes).get(el.dataset.agentTime)?.seconds || 0); });
  const entries = new Map((state.times || []).map(e => [e.id, e]));
  const runs = new Map(state.runs.map(r => [r.id, r]));
  document.querySelectorAll('[data-time-entry]').forEach(el => { const entry = entries.get(el.dataset.timeEntry); if (entry) el.textContent = duration(entry.seconds + (entry.runningSince ? Math.max(0, Math.floor((now - new Date(entry.runningSince)) / 1000)) : 0)); });
  document.querySelectorAll('[data-agent-state]').forEach(el => { const run = runs.get(el.dataset.agentState); if (run) el.textContent = agentState(run); });
  document.querySelectorAll('[data-clock]').forEach(el => {
    const run = runs.get(el.dataset.clock);
    if (run) el.textContent = `Durée : ${duration(agentTime([run], now)[0]?.seconds || 0)}`;
  });
  const run = runs.get(selectedRun);
  if ($('#last-event') && run) $('#last-event').textContent = run.status === 'running' ? `Dernier signal il y a ${elapsed(run.lastEventAt || run.startedAt)}${Date.now() - new Date(run.lastEventAt || run.startedAt) > 30000 ? ' · En attente de nouvelles de l’agent' : ''}` : run.status === 'interrupted' ? `Dernier signal : ${new Date(run.lastEventAt || run.startedAt).toLocaleTimeString('fr-FR')} · durée estimée` : `Fin : ${new Date(run.endedAt).toLocaleTimeString('fr-FR')}`;
}
async function refresh() {
  const previousRuns = new Map(state.runs.map(run => [run.id, run.status]));
  const previousFlows = new Map(state.cards.map(card => [card.id, card.automation?.state]));
  const firstLoad = !state.settings.platform;
  state = await api('/api/state');
  $('#sync-button').hidden = !connectorSupports(state.connectors, 'tickets', 'sync');
  resetJiraButton.hidden = !connectorSupports(state.connectors, 'tickets', 'reset');
  $('#sync-button').textContent = '↻ Synchroniser · ' + connectorName('tickets');
  if (firstLoad && currentView === 'settings') settings(); renderTemplates(); renderProjects(); board(); updateDevServers(); updateAutomation();
  const ended = state.cards.filter(card => previousFlows.get(card.id) === 'running' && card.automation?.state !== 'running');
  if (ended.length) { toast(ended.map(card => `${card.title} : ${card.automation.message}`).join(' · '), ended.some(card => card.automation.state !== 'ready')); return; }
  const finished = state.runs.filter(run => previousRuns.get(run.id) === 'running' && ['completed', 'failed'].includes(run.status));
  if (finished.length && !document.querySelector('dialog[open]')) toast(`${finished.length} exécution(s) terminée(s). Résultats dans l’onglet Agents.`, finished.some(run => run.status === 'failed'));
}
function connectorName(capability, id = state.connectors?.providers?.[capability]) {
  return state.connectors?.modules.find(module => module.id === id)?.name || id || 'le service connecté';
}
function ticketSupports(card, operation) {
  const id = card.ticket?.connector || (card.ticket ? 'jira' : state.connectors?.providers?.tickets);
  return Boolean(state.connectors?.modules.find(module => module.id === id)?.operations?.tickets?.includes(operation));
}
function settings(section = 'general') {
  currentView = 'settings';
  const form = $('#settings-form');
  for (const key of ['repo', 'agent', 'baseBranch']) form.elements[key].value = state.settings[key] || (key === 'agent' ? 'codex' : '');
  $('#default-agent-fields').innerHTML = ['codex', 'claude'].map(agent => `<section class="agent-preset"><h4>${agent === 'codex' ? 'Codex' : 'Claude Code'}</h4>${agentFields(agent, { model: state.settings[agent + 'Model'], effort: state.settings[agent + 'Effort'] }, agent)}</section>`).join('');
  board();
  action(() => settingsView.open(typeof section === 'string' ? section : 'general'));
}
$('#settings-button').onclick = () => settings(); $('#welcome-settings').onclick = () => settings();
const newAgentFields = document.createElement('details'); newAgentFields.className = 'disclosure model-options';
newAgentFields.innerHTML = '<summary>Modèle & réflexion (facultatif)</summary><div data-agent-fields></div>';
$('#new-form .notice-line').before(newAgentFields);
taskAgentFields($('#new-form'));
$('#new-dialog').addEventListener('toggle', () => { if ($('#new-dialog').open) taskAgentFields($('#new-form')); });
$('#settings-form').onsubmit = event => { event.preventDefault(); action(async () => {
  const button = event.target.querySelector('.primary'); button.disabled = true;
  try { await api('/api/settings', 'PUT', Object.fromEntries(new FormData(event.target))); await refresh(); toast('Paramètres généraux enregistrés.'); }
  finally { button.disabled = false; }
}); };
$('#create-button').onclick = () => { $('#new-form').reset(); $('#new-form h2').textContent = 'Nouvelle tâche'; $('#new-dialog').setAttribute('aria-label', 'Nouvelle tâche'); $('#new-form button.primary').textContent = 'Créer la tâche'; $('#new-form .notice-line').textContent = 'Créer la tâche ne lance pas l’agent. Vous pourrez relire ses instructions avant de démarrer.'; $('#template-criteria').textContent = '';  $('#new-form').dataset.status = 'todo'; $('#new-form').elements.repo.value = state.projects.find(p => p.id === $('#project-filter').value)?.repo || (state.projects.length === 1 ? state.projects[0].repo : '') || state.settings.repo || ''; $('#new-project').value = $('#new-form').elements.repo.value; $('#new-form').elements.repo.closest('details').open = !$('#new-form').elements.repo.value && !state.projects.length; document.querySelectorAll('[data-template]').forEach(b => b.setAttribute('aria-pressed', 'false')); $('#new-form').elements.agent.value = state.projects.find(p => sameRepository(p.repo, $('#new-form').elements.repo.value, state.settings.platform))?.agent || state.settings.agent || 'codex'; attachDraft($('#new-form'), 'new', $('#new-dialog')); $('#new-dialog').showModal(); $('#new-form').elements.title.focus(); };
$('#new-form').onsubmit = e => { e.preventDefault(); action(async () => { const button = e.target.querySelector('button.primary'); button.disabled = true; let card; try { card = await api('/api/cards', 'POST', { ...Object.fromEntries(new FormData(e.target)), status: e.target.dataset.status }); } finally { button.disabled = false; } clearDraft('new'); $('#new-dialog').close(); await refresh(); focusFilter = 'all'; $('#search').value = ''; currentView = 'board'; $('#project-filter').value = state.projects.find(p => sameRepository(p.repo, card.repo, state.settings.platform))?.id || ''; board(); await openCard(card.id); }); };
document.querySelectorAll('[data-close]').forEach(button => button.onclick = () => button.closest('dialog').close());
$('#search').oninput = board;
const resetJiraButton = document.createElement('button');
resetJiraButton.type = 'button';
resetJiraButton.textContent = 'Actualiser le périmètre des tickets';
resetJiraButton.title = 'Réconcilier les tickets avec le fournisseur sélectionné.';
$('#sync-button').after(resetJiraButton);
resetJiraButton.onclick = () => action(async () => {
  resetJiraButton.disabled = true;
  try {
    await api('/api/tickets/reset', 'POST', {});
    await refresh();
    toast('Périmètre des tickets actualisé.');
  } finally { resetJiraButton.disabled = false; }
});
$('#sync-button').onclick = () => action(async () => {
  if ((!state.settings.connectorProviders?.tickets || state.settings.connectorProviders.tickets === 'jira') && (!state.settings.jiraUrl || !state.settings.tokenSet)) { settings('jira'); toast('Renseignez la connexion Jira avant de synchroniser.'); return; }
  $('#sync-button').disabled = true;
  try { const result = await api('/api/tickets/sync', 'POST', {}); await refresh(); toast(`${result.count} ticket(s) synchronisé(s).`); } finally { $('#sync-button').disabled = false; }
});
async function openCard(id) {
  selected = id; selectedRun = null; renderDetail(); if (!$('#card-dialog').open) $('#card-dialog').showModal(); await updateRun();
}
function renderDetail() {
  const card = state.cards.find(c => c.id === selected); if (!card) { $('#card-dialog').close(); return; }
  const effectiveSettings = projectConfig(card.repo);
  const runs = state.runs.filter(r => r.cardId === card.id);
  const busyCard = card.publishing || runs.find(r => r.status === 'running');
  const active = card.archived || busyCard;
  wasRunning = Boolean(active);
  $('#card-detail').innerHTML = `<div class="dialog-heading"><div><div class="eyebrow">${esc(card.ticket?.key || 'TÂCHE LOCALE')}</div><h2>${esc(card.title)}</h2></div><button id="close-card" aria-label="Fermer"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="m5 5 14 14M19 5 5 19"/></svg></button></div>
    ${card.ticket ? `<div class="detail-topline"><a href="${esc(card.ticket.url)}" target="_blank" rel="noopener noreferrer">Ouvrir dans ${esc(connectorName('tickets', card.ticket.connector || 'jira'))} ↗</a> · ${esc(card.ticket.status)}</div>` : ''}
    ${card.ticket && (!card.ticket.connector || card.ticket.connector === 'jira') ? `<p class="hint">${effectiveSettings.jiraAutoTransition && effectiveSettings.jiraStatusMap?.[card.status] ? `Jira automatique pour cette colonne → ${esc(effectiveSettings.jiraStatusMap[card.status])}` : 'Cette colonne reste locale : aucune transition Jira automatique configurée.'}</p>${card.ticketSync?.state === 'error' ? `<p class="inline-error">Avancement local enregistré, Jira à vérifier : ${esc(card.ticketSync.message)}</p><button type="button" id="retry-jira-status" ${active ? 'disabled' : ''}>Réessayer la mise à jour Jira</button>` : ''}` : ''}
    ${card.archived ? '<p class="archive-notice">Tâche archivée · restaurez-la pour reprendre le travail.</p>' : ''}
    <form id="card-form">${card.ticket ? `<div class="detail-description">${esc(card.description || 'Aucune description.')}</div>` : `<label>Titre<input name="title" required maxlength="300" value="${esc(card.title)}"></label><label>Description<textarea name="description" rows="3">${esc(card.description)}</textarea></label>`}
    <details class="disclosure" ${(card.checklist || []).length ? 'open' : ''}><summary>Critères de réussite <span class="count">${(card.checklist || []).filter(i => i.done).length}/${(card.checklist || []).length}</span></summary><p class="hint">Cochez ce que vous avez vérifié. Ces critères accompagnent les instructions de l’agent et le brouillon de PR.</p><p class="hint">${(card.checklist || []).filter(i => !i.done).length} critère(s) restent à vérifier. La validation est manuelle.</p><div id="criteria-list"></div><button type="button" id="add-criterion" ${active ? 'disabled' : ''}>＋ Ajouter un critère</button></details><div class="form-row"><label>Avancement local<select name="status" aria-describedby="status-help">${Object.entries(names).map(([key, value]) => `<option value="${key}" ${card.status === key ? 'selected' : ''}>${value}</option>`).join('')}</select><small id="status-help" class="hint">Changement enregistré immédiatement avec la carte. « En cours » lance l’agent.</small></label><label>Agent<select name="agent"><option value="codex" ${card.agent === 'codex' ? 'selected' : ''}>Codex</option><option value="claude" ${card.agent === 'claude' ? 'selected' : ''}>Claude Code</option></select></label></div><details class="disclosure"><summary>Dépôt et branche de travail</summary><label>Dépôt Git source<input list="project-paths" name="repo" value="${esc(card.repo)}" placeholder="Chemin absolu du dépôt Git" ${card.worktree ? 'readonly' : ''}></label><div class="form-row"><label>Branche de la tâche<input name="branch" value="${esc(card.branch || card.suggestedBranch)}" ${card.worktree ? 'readonly' : ''}></label><label>Partir de<input name="baseBranch" value="${esc(card.baseBranch || '')}" placeholder="${esc(effectiveSettings.baseBranch || 'HEAD')}" ${card.worktree ? 'readonly' : ''}></label></div></details><button type="submit" ${active ? 'disabled' : ''}>Enregistrer la carte</button></form>
    <section class="worktree-box"><h3>Worktree dédié</h3><code id="worktree-path">${esc(card.worktree?.path || 'Sera créé au lancement ou au passage en En cours.')}</code><p id="service-worktree-paths" class="hint path-text"></p><p class="hint">En cours lance automatiquement ${card.agent === 'claude' ? 'Claude' : 'Codex'} dans cette branche. Le worktree est réutilisé aux relances.</p><button id="prepare-worktree" ${active || card.worktree ? 'disabled' : ''}>Préparer le worktree</button> <button id="remove-worktree" class="danger" ${active || !card.worktree ? 'disabled' : ''}>Retirer le worktree</button></section><section class="run-box"><label>Instructions complémentaires<textarea id="instructions" name="instructions" form="card-form" rows="2" placeholder="Par exemple : commence par vérifier les tests existants.">${esc(card.instructions || '')}</textarea></label><div class="run-controls"><select id="mode" name="mode" form="card-form" aria-label="Mode de travail"><option value="analyze" ${card.mode === 'analyze' ? 'selected' : ''}>Analyser</option><option value="edit" ${card.mode !== 'analyze' ? 'selected' : ''}>Modifier le code</option></select><button id="launch" class="primary" ${active ? 'disabled' : ''}>Lancer l’agent →</button></div><p class="hint">Le lancement enregistre la carte. Les modifications se font dans le worktree dédié. ${state.projects.find(p => sameRepository(p.repo, card.repo, state.settings.platform))?.automationEnabled ? 'Le mode automatique publiera une PR brouillon après les vérifications.' : 'Aucune publication n’est demandée.'} Claude peut refuser les commandes qui exigent une permission interactive.</p></section>
    <section class="worktree-box"><h3>Pull request</h3>
    ${card.pullRequest ? `<p><a class="pr-link" href="${esc(card.pullRequest.url)}" target="_blank" rel="noopener noreferrer">PR #${card.pullRequest.number} · ${esc(card.pullRequest.title)} ↗</a></p><p class="hint">${card.ticket ? card.pullRequest.ticketLinked ? 'Liée au ticket Jira.' : esc(card.pullRequest.ticketError || 'Lien Jira en attente.') : 'Carte locale.'}</p>` : '<p class="hint">Publiez les changements de ce worktree et reliez la PR à votre ticket.</p>'}
    ${card.pullRequest ? `<button id="review-pr" ${active || !card.worktree ? 'disabled' : ''}>Review avec ${card.agent === 'claude' ? 'Claude' : 'Codex'}</button><p class="hint">Analyse du commit publié, worktree propre requis. Résultat local, sans commentaire publié dans le service de dépôt.</p><div id="pr-review-result"></div>` : ''}
    <button id="prepare-pr" ${active || !card.worktree ? 'disabled' : ''}>${card.pullRequest ? 'Publier les nouveaux changements' : 'Préparer la PR'}</button>
    ${card.ticket && card.pullRequest && ticketSupports(card, 'linkPullRequest') ? `<button id="link-pr" ${active ? 'disabled' : ''}>${card.pullRequest.ticketLinked ? 'Actualiser le lien Jira' : 'Réessayer le lien Jira'}</button>` : ''}</section>
    <section class="worktree-box"><h3>Temps passé</h3><p>Agents : <strong data-agent-time="${esc(card.id)}">${duration(agentTime(runs)[0]?.seconds || 0)}</strong></p><p class="hint">Cumul automatique, toutes dates. Les exécutions interrompues sont estimées au dernier signal reçu. Le temps humain se saisit séparément.</p><button id="card-time" ${card.archived ? 'disabled' : ''}>Saisir du temps / chronométrer</button></section>
    <section id="agent-summary" class="agent-summary" hidden><h3>Compte rendu de l’agent</h3><p class="hint" id="summary-context"></p><pre id="summary-text"></pre><p class="hint">À relire et à vérifier avant de valider la tâche.</p></section>
    <details class="disclosure" id="run-details" ${active ? 'open' : ''}><summary>Exécutions et journal <span class="count">${runs.length}</span></summary><select id="run-select" aria-label="Historique des exécutions">${runs.length ? runs.map(r => `<option value="${r.id}">${r.agent === 'codex' ? 'Codex' : 'Claude'} · ${new Date(r.startedAt).toLocaleString('fr-FR')} · ${runNames[r.status]}</option>`).join('') : '<option value="">Aucune exécution</option>'}</select><div class="detail-actions"><span id="run-status" class="run-status"></span><button id="stop-run" class="danger" hidden>Arrêter l’agent</button></div>
    <p id="run-usage" class="hint" aria-live="polite"></p><p class="hint">Coûts API indicatifs en USD ; la facturation de votre abonnement peut différer.</p>
    <section id="live-progress" class="live-progress" hidden><strong id="current-activity"></strong><div class="progress-times"><span id="run-clock"></span><span id="last-event"></span></div><pre id="live-text" hidden></pre></section>
    <label class="check"><input id="follow-log" type="checkbox" checked> Suivre les nouveaux messages</label><pre class="log" id="log" aria-label="Journal de l’agent">Les résultats apparaîtront ici.</pre></details>
    ${card.ticket && ticketSupports(card, 'transitions') && ticketSupports(card, 'transition') ? '<div class="jira-actions"><button id="load-transitions">Changer le statut distant…</button><select id="transitions" aria-label="Nouveau statut distant" hidden></select><button id="apply-transition" hidden>Appliquer à Jira</button></div>' : ''}
    <div class="card-utilities"><button id="duplicate-card">Dupliquer comme tâche locale</button><button id="archive-card" ${busyCard ? 'disabled' : ''}>${card.archived ? 'Restaurer la tâche' : 'Archiver la tâche'}</button></div><details class="disclosure"><summary>Retirer définitivement du tableau…</summary><div class="detail-actions"><button id="delete-card" class="danger" ${active ? 'disabled' : ''}>Retirer du tableau</button><span class="hint">${card.ticket ? 'Le ticket Jira sera conservé.' : 'La carte est sauvegardée sur votre PC.'}</span></div></details>`;
  const automationStatus = document.createElement('section'); automationStatus.id = 'automation-status'; automationStatus.className = 'worktree-box';
  $('#card-form').before(automationStatus); updateAutomation();
  compactCard();
  renderWorkflowDetail(card, Boolean(active));
  const fields = document.createElement('div'); fields.dataset.agentFields = '';
  $('#card-form').elements.agent.closest('.form-row').after(fields);
  taskAgentFields($('#card-form'), card);
  (card.checklist || []).forEach(item => addCriterion(item, Boolean(active)));
  const preparation = document.createElement('section'); preparation.className = 'worktree-box';
  preparation.innerHTML = `<h3>Préparer le ticket</h3><p class="hint">L’agent propose des critères et des questions à partir de la carte enregistrée, en lecture seule. Le lancement enregistre vos saisies et conserve la colonne actuelle. Consomme les quotas de votre agent.</p><button type="button" id="prepare-ticket" ${active ? 'disabled' : ''}>Proposer des critères avec l’agent</button><div id="ticket-proposal" aria-live="polite"></div>`;
  $('#card-form').after(preparation);
  renderTicketPreparation(card.id);
  setupBrainstorm(card, active, saveCard);
  $('#prepare-ticket').onclick = () => action(async () => {
    if (!$('#card-form').reportValidity()) return;
    $('#prepare-ticket').disabled = true;
    try {
      await saveCard(true);
      const run = await api(cardPath(card.id) + '/prepare-ticket', 'POST', {});
      await refresh(); renderDetail(); selectedRun = run.id; $('#run-select').value = run.id; await updateRun();
      toast('Préparation lancée. La proposition apparaîtra dans la fiche.');
    } finally { if ($('#prepare-ticket')) $('#prepare-ticket').disabled = state.cards.find(c => c.id === card.id)?.archived || state.runs.some(r => r.cardId === card.id && r.status === 'running'); }
  });
  $('#add-criterion').onclick = () => { if ($('#criteria-list').children.length >= 20) return toast('Maximum 20 critères.', true); addCriterion({ text: '', done: false }, false); $('#criteria-list').lastElementChild.querySelector('input[type=text]').focus(); };
  if (card.archived) { [...$('#card-form').elements].forEach(field => field.disabled = true); if ($('#load-transitions')) $('#load-transitions').disabled = true; }
  $('#duplicate-card').onclick = () => action(async () => { const copy = await api(cardPath(card.id) + '/duplicate', 'POST', {}); await refresh(); await openCard(copy.id); toast('Copie locale créée, sans branche ni liaison Jira.'); });
  if ($('#archive-card')) $('#archive-card').onclick = () => action(async () => { await api(cardPath(card.id) + '/archive', 'POST', { archived: !card.archived }); await refresh(); $('#card-dialog').close(); toast(card.archived ? 'Tâche restaurée.' : 'Tâche archivée. Retrouvez-la dans Archives.'); });
  if ($('#retry-jira-status')) $('#retry-jira-status').onclick = () => action(async () => { const result = await api(cardPath(card.id) + '/sync-status', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(result.ticketSync?.state === 'error' ? 'Jira à vérifier : ' + result.ticketSync.message : 'Statut Jira mis à jour.', result.ticketSync?.state === 'error'); });
  $('#card-time').onclick = () => openTime(card.id);
  if (!active) attachDraft($('#card-form'), card.id, $('#card-detail'));
  if (active) { $('#card-form').elements.model.disabled = true; $('#card-form').elements.effort.disabled = true; $('#card-form').elements.agent.disabled = true; }
  $('#close-card').onclick = () => $('#card-dialog').close();
  async function saveCard(launchSeparately = false) { const body = Object.fromEntries(new FormData($('#card-form'))); body.blockedBy = [...$('#blocked-by').selectedOptions].map(option => option.value); body.checklist = [...$('#criteria-list').children].map(row => ({ text: row.querySelector('input[type=text]').value, done: row.querySelector('input[type=checkbox]').checked })); if (launchSeparately) body.status = state.cards.find(c => c.id === card.id).status; const saved = await api(cardPath(card.id), 'PATCH', body); clearDraft(card.id); return saved; }
  let savingCard = false;
  const cardForm = $('#card-form'), statusSelect = cardForm.elements.status;
  async function persistCard(statusChanged = false) {
    if (savingCard) return;
    if (!cardForm.reportValidity()) { statusSelect.value = state.cards.find(c => c.id === card.id)?.status || card.status; return; }
    savingCard = true;
    try {
      const pending = saveCard();
      statusSelect.disabled = true; cardForm.querySelector('button[type=submit]').disabled = true;
      const saved = await pending; await refresh();
      if (selected === card.id && $('#card-dialog').open) { renderDetail(); await updateRun(); }
      toast(saved.ticketSync?.state === 'error' ? `Avancement local enregistré. Jira à vérifier : ${saved.ticketSync.message}` : statusChanged ? `Avancement enregistré : ${names[saved.status]}.${saved.ticketSync?.state === 'synced' && saved.ticketSync.localStatus === saved.status ? ' Jira : ' + saved.ticket.status + '.' : ''}` : 'Carte enregistrée.', saved.ticketSync?.state === 'error');
    } catch (error) {
      statusSelect.value = state.cards.find(c => c.id === card.id)?.status || card.status;
      throw error;
    } finally {
      savingCard = false;
      if (cardForm.isConnected) { statusSelect.disabled = Boolean(active); cardForm.querySelector('button[type=submit]').disabled = Boolean(active); }
    }
  }
  statusSelect.disabled = Boolean(active);
  statusSelect.onchange = () => action(() => persistCard(true));
  cardForm.onsubmit = e => { e.preventDefault(); action(() => persistCard()); };
  $('#launch').onclick = () => action(async () => {
    $('#launch').disabled = true;
    try { await saveCard(true); const run = await api(cardPath(card.id) + '/run', 'POST', { agent: $('#card-form').elements.agent.value, mode: $('#mode').value, instructions: $('#instructions').value }); await refresh(); renderDetail(); selectedRun = run.automation?.runId || run.id; await updateRun(); } finally { if ($('#launch')) $('#launch').disabled = state.cards.find(c => c.id === card.id)?.publishing || state.runs.some(r => r.cardId === card.id && r.status === 'running'); }
  });
  $('#prepare-worktree').onclick = () => action(async () => { await saveCard(true); await api(cardPath(card.id) + '/worktree', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast('Worktree prêt.'); });
  const devBox = document.createElement('details');
  devBox.className = 'disclosure'; devBox.open = devRunning(card.id);
  devBox.innerHTML = '<summary>Lancer le projet</summary><p class="hint">Commandes définies dans Mes projets → Réglages. Le projet de la carte tourne dans son worktree ; son éventuel projet lié tourne depuis son dépôt local, avec sa branche et ses fichiers actuels. Démarrage et arrêt communs. Les commandes avec {port} permettent à Atelier de fournir des ports distincts.</p><p id="dev-context" class="hint path-text"></p><button id="start-dev">Démarrer Front / Back</button> <button id="stop-dev">Arrêter Front / Back</button><p id="dev-status" class="hint" role="status"></p><pre id="dev-log" class="log" aria-label="Journal Front / Back"></pre>';
  $('#remove-worktree').closest('details').after(devBox);
  $('#start-dev').onclick = () => action(async () => { await api(cardPath(card.id) + '/dev-servers', 'POST', {}); await refresh(); updateDevServers(); });
  $('#stop-dev').onclick = () => action(async () => { await api(cardPath(card.id) + '/dev-servers', 'DELETE', {}); await refresh(); updateDevServers(); });
  updateDevServers();
  $('#remove-worktree').onclick = () => action(async () => { if (!(await confirmAction("Retirer le worktree principal et les worktrees Front / Back associés, et supprimer leurs fichiers locaux, y compris les modifications non commitées et les fichiers non suivis ? Cette perte est irréversible. Les branches et leurs commits seront conservés. Aucun ticket Jira ni autre service ne sera modifié.", "Retirer le worktree ?", "Retirer et supprimer les fichiers"))) return; await api(cardPath(card.id) + '/worktree', 'DELETE', { discardChanges: true }); await refresh(); renderDetail(); await updateRun(); toast('Worktrees retirés, branches conservées.'); });
  renderReviewReport(card.id);
  if ($('#review-pr')) $('#review-pr').onclick = () => action(async () => { $('#review-pr').disabled = true; try { const run = await api(cardPath(card.id) + '/review', 'POST', {}); await refresh(); renderDetail(); selectedRun = run.id; $('#run-select').value = run.id; await updateRun(); toast('Review démarrée. Les constats apparaîtront ici.'); } finally { if ($('#review-pr')) $('#review-pr').disabled = state.cards.find(c => c.id === card.id)?.publishing || state.runs.some(r => r.cardId === card.id && r.status === 'running'); } });
  $('#prepare-pr').onclick = () => action(async () => {
    $('#prepare-pr').disabled = true;
    try {
      const preview = await api(cardPath(card.id) + '/pr-preview');
      prPreview = preview; prCardId = card.id;
      const form = $('#pr-form'); form.reset();
      for (const key of ['title', 'base', 'body']) form.elements[key].value = preview[key];
      $('#pr-destination').textContent = preview.repository + ' · ' + preview.branch;
      $('#pr-files').textContent = preview.status || 'Aucun changement non commité.';
      $('#pr-commits').textContent = preview.commits || 'Aucun nouveau commit depuis le point de départ.';
      $('#pr-diff').textContent = preview.diff || 'Aucun diff local.';
      $('#pr-commit-label').hidden = !preview.dirty;
      form.elements.commitChanges.required = preview.dirty;
      $('#pr-dialog').showModal();
    } finally { if ($('#prepare-pr')) $('#prepare-pr').disabled = false; }
  });
  if ($('#link-pr')) $('#link-pr').onclick = () => action(async () => {
    $('#link-pr').disabled = true;
    try { const updated = await api(cardPath(card.id) + '/link-pr', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(updated.pullRequest.ticketLinked ? 'PR liée au ticket.' : updated.pullRequest.ticketError, !updated.pullRequest.ticketLinked); }
    finally { if ($('#link-pr')) $('#link-pr').disabled = false; }
  });
  $('#delete-card').onclick = () => action(async () => { if (!(await confirmAction("Retirer cette carte du tableau ?", "Supprimer la carte ?", "Supprimer localement"))) return; await api(cardPath(card.id), 'DELETE', {}); $('#card-dialog').close(); await refresh(); });
  $('#run-select').onchange = () => action(async () => { selectedRun = $('#run-select').value; await updateRun(); });
  $('#stop-run').onclick = () => action(async () => { $('#stop-run').disabled = true; try { await api(`/api/runs/${selectedRun}/cancel`, 'POST', {}); toast('Arrêt demandé…'); } finally { $('#stop-run').disabled = false; } });
  if (card.ticket && $('#load-transitions')) {
    $('#load-transitions').onclick = () => action(async () => { const result = await api(cardPath(card.id) + '/transitions'); $('#transitions').innerHTML = result.transitions.map(t => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join(''); $('#transitions').hidden = false; $('#apply-transition').hidden = !result.transitions.length; if (!result.transitions.length) toast('Aucune transition disponible.'); });
    $('#apply-transition').onclick = () => action(async () => { await api(cardPath(card.id) + '/transition', 'POST', { transitionId: $('#transitions').value }); await refresh(); renderDetail(); toast('Statut Jira mis à jour.'); });
  }
  cursor = 0; logRun = null;
}
async function updateRun() {
  if (!$('#card-dialog').open) return;
  updateAutomation();
  const select = $('#run-select');
  for (const run of state.runs.filter(r => r.cardId === selected).reverse()) {
    if (![...select.options].some(o => o.value === run.id)) select.prepend(new Option('', run.id));
  }
  if (select.options.length > 1) select.querySelector('option[value=""]')?.remove();
  selectedRun ||= $('#run-select').value;
  if (!selectedRun) return;
  const run = state.runs.find(r => r.id === selectedRun); if (!run) return;
  const active = state.cards.find(c => c.id === selected)?.archived || state.cards.find(c => c.id === selected)?.publishing || state.runs.some(r => r.cardId === selected && r.status === 'running');
  if (wasRunning && !active) $('#card-form').elements.status.value = state.cards.find(c => c.id === selected)?.status || 'todo';
  wasRunning = active;
  for (const option of $('#run-select').options) {
    const current = state.runs.find(r => r.id === option.value);
    if (current) option.textContent = `${current.agent === 'codex' ? 'Codex' : 'Claude'} · ${new Date(current.startedAt).toLocaleString('fr-FR')} · ${runNames[current.status]}${current.kind === 'pr-review' ? ' · Review PR' : current.kind === 'ticket-preparation' ? ' · Préparation du ticket' : current.kind === 'brainstorm' ? ' · Brainstorming' : ''}`;
  }
  $('#card-form').elements.status.disabled = Boolean(active);
  for (const field of ['agent', 'model', 'effort']) $('#card-form').elements[field].disabled = Boolean(active);
  $('#launch').disabled = active; $('#card-form button[type=submit]').disabled = active; $('#add-criterion').disabled = active; $('#delete-card').disabled = active;
  $('#prepare-ticket').disabled = active;
  renderTicketPreparation(selected);
  renderBrainstorm(selected);
  if ($('#brainstorm-ticket')) $('#brainstorm-ticket').disabled = active;
  if ($('#publish-jira')) $('#publish-jira').disabled = active;
  $('#agent-summary').hidden = !run.summary || Boolean(run.review);
  $('#summary-text').textContent = run.summary || '';
  $('#summary-context').textContent = `${run.agent === 'claude' ? 'Claude' : 'Codex'} · ${run.actualModel || run.model || 'Modèle CLI'} · ${effortLevels[run.agent]?.[run.effort] || 'Réflexion CLI'} · ${runNames[run.status]} · ${new Date(run.startedAt).toLocaleString('fr-FR')}`;
  $('#run-status').textContent = `${runNames[run.status]} · ${run.mode === 'edit' ? 'Modification' : 'Analyse'}`;
  $('#run-usage').textContent = usageText(run);
  $('#stop-run').hidden = run.status !== 'running';
  const currentCard = state.cards.find(c => c.id === selected);
  $('#worktree-path').textContent = currentCard?.worktree?.path || 'Sera créé au lancement.';
  $('#prepare-worktree').disabled = active || Boolean(currentCard?.worktree);
  $('#remove-worktree').disabled = active || !currentCard?.worktree || devRunning(selected);
  $('#prepare-pr').disabled = active || !currentCard?.worktree;
  if ($('#review-pr')) $('#review-pr').disabled = active || !currentCard?.worktree;
  renderReviewReport(selected);
  for (const field of ['repo', 'branch', 'baseBranch']) $('#card-form').elements[field].readOnly = active || Boolean(currentCard?.worktree);
  $('#live-progress').hidden = false;
  $('#current-activity').textContent = `${agentState(run)} · ${run.attention || run.activity || ''}`;
  $('#run-clock').dataset.clock = run.id;
  $('#live-text').hidden = !run.liveText || run.status !== 'running';
  $('#live-text').textContent = run.liveText || '';
  updateClocks();
  if (logRun !== selectedRun) { cursor = 0; logRun = selectedRun; $('#log').textContent = ''; }
  const requestedRun = selectedRun;
  const logElement = $('#log');
  let logs;
  do {
    logs = await api(`/api/runs/${requestedRun}/logs?after=${cursor}`);
    if (requestedRun !== selectedRun || !$('#card-dialog').open || logElement !== $('#log')) return;
    for (const entry of logs) { if (entry.seq <= cursor) continue; cursor = entry.seq; if (entry.text) logElement.append(document.createTextNode(entry.text + '\n')); }
    if (logs.length && $('#follow-log').checked) logElement.scrollTop = logElement.scrollHeight;
  } while (logs.length === 500);
}
async function loadAgents() {
  const agents = await api('/api/agents');
  $('#agents').innerHTML = Object.entries(agents).map(([key, value]) => `<div class="agent-row"><span class="agent-icon">${key === 'codex' ? '⌘' : '✳'}</span><div>${key === 'codex' ? 'Codex' : 'Claude Code'}<small>${value.connected ? 'Connecté' : value.installed ? 'Connexion nécessaire' : 'Non installé'}</small></div><span class="online-dot ${value.connected ? '' : 'offline'}" title="${esc(value.version)}"></span></div>`).join('');
}
async function loadVersion() {
  const { version } = await api('/api/health');
  $('#app-version').textContent = 'v' + version;
  $('#app-version').setAttribute('aria-label', 'Version ' + version);
}
action(loadVersion);
initUpdates({ api, confirm: confirmAction });
await action(refresh); action(loadAgents);
async function syncLive() {
  livePending = true;
  if (refreshing) return;
  refreshing = true;
  try {
    while (livePending) { livePending = false; await refresh(); await updateRun(); }
  } catch { $('#live-connection').textContent = 'Actualisation interrompue · nouvelle tentative…'; setTimeout(syncLive, 1000); }
  finally { refreshing = false; }
}
const events = new EventSource('/api/events');
events.onmessage = () => { $('#live-connection').textContent = '● En direct'; $('#live-connection').className = 'connected'; syncLive(); };
events.onerror = () => { $('#live-connection').textContent = 'Direct interrompu · reconnexion…'; $('#live-connection').className = ''; };
setInterval(updateClocks, 1000);
setInterval(() => action(loadAgents), 60000);

$('#pr-form').onsubmit = e => { e.preventDefault(); action(async () => {
  if (!prPreview || !prCardId) return;
  const button = $('#publish-pr'); button.disabled = true; button.textContent = 'Publication en cours…';
  try {
    const body = Object.fromEntries(new FormData(e.target)); body.commitChanges = e.target.elements.commitChanges.checked; body.fingerprint = prPreview.fingerprint;
    const updated = await api(cardPath(prCardId) + '/pull-request', 'POST', body);
    $('#pr-dialog').close(); await refresh();
    if ($('#card-dialog').open && selected === prCardId) { renderDetail(); await updateRun(); }
    toast(updated.pullRequest.ticketError ? 'PR publiée. Liaison au ticket à réessayer depuis la carte.' : 'PR publiée' + (updated.pullRequest.ticketLinked ? ' et liée à Jira.' : '.'));
  } finally { button.disabled = false; button.textContent = 'Pousser et créer la PR brouillon'; }
}); };


function renderProjects() { projectsView.render(state); }
$('#project-filter').onchange = () => { focusFilter = 'all'; $('#search').value = ''; board(); };
$('#new-project').onchange = e => { if (e.target.value) { $('#new-form').elements.repo.value = e.target.value; $('#new-form').elements.agent.value = state.projects.find(p => p.repo === e.target.value)?.agent || state.settings.agent || 'codex'; taskAgentFields($('#new-form')); $('#new-form').elements.repo.closest('details').open = false; } };
$('#projects-button').onclick = () => { renderProjects(); $('#projects-dialog').showModal(); };

function scopedCards(includeArchived = false) {
  const project = state.projects.find(p => p.id === $('#project-filter').value);
  return state.cards.filter(c => !c.brainstormSession && (includeArchived || !c.archived) && (!project || sameRepository(c.repo, project.repo, state.settings.platform)));
}
function duration(seconds) {
  seconds = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(seconds / 3600)} h ${String(Math.floor(seconds % 3600 / 60)).padStart(2, '0')} min ${String(seconds % 60).padStart(2, '0')} s`;
}
function agentState(run) {
  if (run.attention) return run.status === 'running' ? 'Attention requise' : 'Bloqué · consulter le journal';
  if (run.status !== 'running') return runNames[run.status];
  return Date.now() - new Date(run.lastEventAt || run.startedAt) > 60000 ? 'Sans signal · état à vérifier' : 'Travaille';
}
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
  const runs = state.runs.filter(r => ids.has(r.cardId)), times = filterTimes((state.times || []).filter(e => ids.has(e.cardId)), currentView === 'time' ? timeFilters : {});
  history.replaceState(null, '', '#' + new URLSearchParams({ ...(project ? { project: project.id } : {}), view: currentView }));
  const viewCopy = { settings: ['Paramètres', 'Réglages généraux, modules et connexions.'], brainstorm: ['Brainstorming', 'Explorez une idée. Choisissez vos prochaines tâches. Jira reste facultatif.'], overview: ['Vue d’ensemble', 'Vos prochaines actions, en un coup d’œil.'], board: ['Tableau des tâches', 'Préparez, lancez, vérifiez. Une étape à la fois.'], agents: ['Activité des agents', 'Suivez les exécutions et retrouvez leurs résultats.'], processes: ['Processus', 'Lancez et pilotez les fronts et les backs de vos projets.'], worktrees: ['Branches & pull requests', 'Retrouvez les dossiers de travail, les branches et les PR.'], time: ['Temps passé', 'Relisez vos saisies avant leur envoi au service connecté.'], archives: ['Archives', 'Retrouvez l’historique et restaurez une tâche quand vous en avez besoin.'] };
  $('#workspace-title').textContent = viewCopy[currentView][0];
  $('#overview-button').classList.toggle('nav-active', currentView === 'overview');
  $('#overview-button').setAttribute('aria-current', currentView === 'overview' ? 'page' : 'false');
  $('#workspace-subtitle').textContent = viewCopy[currentView][1];
  $('#breadcrumb').textContent = project?.name || 'Tous les projets';
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
  if (project) $('#project-summary').innerHTML = `<span><strong>${cards.length}</strong> tâches</span><span><strong>${runs.filter(r => r.status === 'running').length}</strong> agents actifs</span><span><strong>${cards.filter(c => c.worktree).length}</strong> worktrees</span><span><strong>${cards.filter(c => c.pullRequest).length}</strong> PR</span>`;
  if (currentView === 'processes') document.querySelectorAll('[data-process-log]').forEach(log => { log.textContent = (state.devServers || []).find(session => session.cardId === log.dataset.processLog)?.log || ''; });
  const signature = JSON.stringify([currentView, project?.id, cards, runs, state.queue, timeFilters, summaryDay, currentView === 'overview' ? state.times : times, worktreeChecks, currentView === 'processes' ? [processCards(project), state.projects, (state.devServers || []).map(({ log, ...session }) => session)] : null]);
  if (signature === panelSignature || ['board', 'settings'].includes(currentView)) return;
  panelSignature = signature;
  const cardButton = card => `<button data-open-card="${esc(card.id)}" aria-label="Ouvrir ${esc(card.title)}">Ouvrir la tâche ↗</button>`;
  let html = '';
  if (currentView === 'overview') html = overviewMarkup(cards, runs, times);
  if (currentView === 'processes') html = processesMarkup(project);
  if (currentView === 'archives') html = '<div class="panel-heading"><div><h2>Votre historique</h2><p class="hint">Les tâches archivées, avec leurs temps, branches et journaux. Restaurer les remet dans le tableau à leur étape précédente.</p></div></div>' + (cards.map(card => `<article class="workspace-row archive-row"><div><span class="section-kicker">Tâche archivée</span><h3>${esc(card.ticket?.key || '')} ${esc(card.title)}</h3><p class="hint">${card.worktree ? 'Worktree conservé · visible dans Worktrees' : 'Historique conservé'}</p></div><div class="row-actions">${cardButton(card)}<button data-restore-card="${esc(card.id)}">Restaurer</button></div></article>`).join('') || '<p class="empty">Aucune tâche archivée.</p>');
  if (currentView === 'agents') {
    html = `<div class="view-stats"><div><span>En cours</span><strong>${runs.filter(run => run.status === 'running').length}</strong><small>Agents au travail</small></div><div><span>Terminées</span><strong>${runs.filter(run => run.status === 'completed').length}</strong><small>Exécutions à relire</small></div><div><span>À examiner</span><strong>${runs.filter(run => ['failed', 'cancelled', 'interrupted'].includes(run.status)).length}</strong><small>Erreurs ou interruptions</small></div></div><div class="panel-heading"><div><h2>Vos exécutions</h2><p class="hint">Suivez le travail en cours ou ouvrez une tâche pour consulter son résultat.</p></div></div><details class="view-help"><summary>Un agent ne répond plus ?</summary><p class="hint">Le silence ne prouve pas un blocage. Après un refus de permission, adaptez les instructions puis relancez depuis la tâche.</p></details>`;
    const usage = usageTotals(runs);
    html += `<section class="worktree-box"><h3>Coût API approximatif et temps agent</h3><p><strong>${esc(usage.costUsd.toLocaleString('fr-FR', { style: 'currency', currency: 'USD', minimumFractionDigits: 4 }))}</strong> sur ${usage.measured} exécution(s) avec coût · Temps cumulé : <strong>${duration(agentTime(runs).reduce((sum, group) => sum + group.seconds, 0))}</strong> sur ${runs.length} exécution(s).</p><p class="hint">${usage.missing} exécution(s) sans coût disponible. Périmètre affiché, toutes dates. Estimation API indicative ; la facturation de votre abonnement peut différer.</p></section>`;
    html += runs.map(run => {
      const card = cards.find(c => c.id === run.cardId);
      return `<article class="workspace-row agent-run" data-status="${esc(run.status)}"><div><span class="agent-pill ${esc(run.agent)}">${run.agent === 'claude' ? 'Claude Code' : 'Codex'}</span><h3>${esc(card.title)}</h3><p><span class="state-badge" data-agent-state="${run.id}">${esc(agentState(run))}</span> <span data-clock="${run.id}"></span></p><p class="hint">${esc(run.attention || run.activity || '')}</p><p class="hint">${esc(usageText(run))}</p><code>${esc(run.branch || run.repo)}</code></div><div class="row-actions">${cardButton(card)}${run.status === 'running' ? `<button data-stop-agent="${run.id}" class="danger">Arrêter</button>` : ''}</div></article>`;
    }).join('') || '<p class="empty">Aucun agent lancé pour ce projet.</p>';
  }
  if (currentView === 'worktrees') {
    html = '<div class="panel-heading"><div><h2>Dossiers de travail</h2><p class="hint">Vos branches, leurs dossiers de travail et les pull requests associées.</p></div><div class="row-actions"><button id="refresh-prs">Actualiser les PR</button><button id="inspect-worktrees">Actualiser l’état Git</button></div></div>';
    html += cleanupMarkup(cards);
    html += cards.filter(c => !c.worktree && c.pullRequest).map(card => `<article class="workspace-row"><div><h3>${esc(card.title)}</h3>${prMarkup(card)}</div><div class="row-actions">${cardButton(card)}</div></article>`).join('');
    html += cards.filter(c => c.worktree).map(card => {
      const check = worktreeChecks.find(w => w.cardId === card.id);
      const active = card.publishing || runs.some(r => r.cardId === card.id && r.status === 'running');
      return `<article class="workspace-row worktree-row"><div><span class="section-kicker">${esc(card.ticket?.key || 'Branche locale')}</span><h3>${esc(card.title)}</h3><code class="branch-label">${esc(card.branch)}</code><p class="path-text resource-location">${esc(card.worktree.path)}</p>${Object.entries(card.serviceWorktrees || {}).map(([role, worktree]) => `<p class="path-text resource-location">Worktree ${esc(role)} : ${esc(worktree.path)} · ${esc(worktree.branch)}</p>`).join('')}<p class="hint">${esc(check ? check.error || (!check.coherent ? 'Branche inattendue' : check.clean ? 'Dossier propre' : 'Modifications locales') : 'État Git non vérifié')}</p>${check?.status ? `<pre class="log">${esc(check.status)}</pre>` : ''}${prMarkup(card)}</div><div class="row-actions">${cardButton(card)}<button class="danger" data-clean-worktree="${esc(card.id)}" ${active ? 'disabled' : ''}>Retirer le worktree</button></div></article>`;
    }).join('') || '<p class="empty">Aucun worktree. Le premier sera créé au lancement d’une tâche.</p>';
  }
  if (currentView === 'time') {
    const human = times.reduce((sum, e) => sum + e.seconds, 0);
    const agentTimes = agentTime(runs, Date.now(), timeFilters);
    const agent = agentTimes.reduce((sum, g) => sum + g.seconds, 0);
    html = `<div class="panel-heading"><div><h2>Saisies & chronomètres</h2><p class="hint">Des saisies locales, à relire avant tout envoi au service connecté.</p></div><button id="add-time" class="primary" ${cards.some(c => !c.archived) ? '' : 'disabled'}>Saisir du temps</button></div>`;
    html += `<div class="view-stats time-stats"><div><span>Temps humain</span><strong id="human-total">${duration(human)}</strong><small>Période sélectionnée</small></div><div><span>Temps des agents</span><strong id="agent-total">${duration(agent)}</strong><small>Période sélectionnée · non facturé</small></div><div><span>À valider</span><strong>${times.filter(entry => !entry.runningSince && entry.exportState !== 'exported').length}</strong><small>Saisies à envoyer ou vérifier</small></div></div>`;
    html += `<section class="daily-summary"><h3>Temps des agents par tâche</h3><p class="hint">Ajouté automatiquement à partir des exécutions, y compris les reviews, échecs et arrêts. Période sélectionnée · jours locaux · hors export de temps. Une exécution interrompue est estimée au dernier signal reçu.</p>${agentTimes.map(g => { const card = cards.find(c => c.id === g.cardId); return `<div class="daily-row"><button data-open-card="${esc(g.cardId)}">${esc(card.ticket?.key || card.title)}</button><strong data-agent-period data-agent-time="${esc(g.cardId)}">${duration(g.seconds)}</strong><span>${g.runs} exécution(s)${g.running ? ' · en cours' : ''}${g.estimated ? ' · durée estimée' : ''}</span></div>`; }).join('') || '<p class="hint">Aucune durée d’agent pour cette période.</p>'}</section>`;
    const daily = dailyTime((state.times || []).filter(e => ids.has(e.cardId)), summaryDay);
    html += `<section class="daily-summary"><div class="panel-heading"><div><h3>Bilan de la journée</h3><p class="hint">${duration(daily.reduce((sum,g) => sum + g.seconds, 0))} · ${daily.reduce((sum,g) => sum + g.pending, 0)} saisie(s) à envoyer ou vérifier · ${daily.reduce((sum,g) => sum + g.running, 0)} chronomètre(s) actif(s)</p></div><label>Journée<input id="summary-day" type="date" value="${summaryDay}"></label></div>${daily.map(g => { const card = cards.find(c => c.id === g.cardId); return `<div class="daily-row"><button data-open-card="${esc(g.cardId)}">${esc(card?.ticket?.key || card?.title || 'Tâche')}</button><strong>${duration(g.seconds)}</strong><span>${g.pending} à valider</span></div>`; }).join('') || '<p class="hint">Aucun temps saisi pour cette journée.</p>'}<p class="hint">Regroupement par date de saisie. Les durées des agents sont exclues.</p><button id="show-summary-day">Afficher les saisies de cette journée</button></section>`;
    html += `<section class="time-filters" aria-label="Filtrer les saisies"><label>Du<input id="time-from" type="date" value="${esc(timeFilters.from)}"></label><label>Au<input id="time-to" type="date" value="${esc(timeFilters.to)}"></label><label>Export de temps<select id="time-status">${[['all', 'Toutes les saisies'], ['pending', 'À valider / vérifier'], ['exported', 'Déjà envoyées']].map(([value, label]) => `<option value="${value}" ${timeFilters.status === value ? 'selected' : ''}>${label}</option>`).join('')}</select></label><button id="reset-time-filters">Tout afficher</button><button id="export-time-csv" ${times.some(e => !e.runningSince) ? '' : 'disabled'}>Exporter en CSV</button></section><p class="hint">Les dates filtrent les saisies et les agents ; le statut d’export filtre uniquement les saisies humaines. ${times.length} saisie(s) dans cette vue. Le CSV reprend ces filtres et exclut les chronomètres actifs. Il n’envoie rien au service connecté.</p>`;
    html += times.map(entry => {
      const card = cards.find(c => c.id === entry.cardId);
      const labels = { draft: 'Local · à valider', sending: 'Envoi en cours', uncertain: 'Envoi à vérifier', exported: 'Envoyé au service' };
      return `<article class="workspace-row time-entry"><div><span class="section-kicker">${esc(entry.date)} · ${esc(entry.startTime)}</span><h3>${esc(card.ticket?.key || card.title)} · <span data-time-entry="${entry.id}">${duration(entry.seconds)}</span></h3><p class="hint">${esc(entry.date)} à ${esc(entry.startTime)} · ${entry.runningSince ? 'Chronomètre actif' : esc(labels[entry.exportState])}${entry.interrupted ? ' · interrompu au redémarrage, durée à vérifier' : ''}</p><p class="entry-description">${esc(entry.description)}</p>${entry.error ? `<p class="inline-error">${esc(entry.error)}</p>` : ''}</div><div class="row-actions">${entry.runningSince ? `<button data-stop-time="${entry.id}">Arrêter</button>` : entry.exportState === 'draft' ? `<button data-edit-time="${entry.id}">Corriger</button><button ${connectorSupports(state.connectors, 'time', 'send') ? '' : 'hidden'} data-export-time="${entry.id}" ${card.ticket || state.connectors?.providers?.time !== 'tempo' ? '' : 'disabled'}>Exporter…</button><button data-delete-time="${entry.id}" class="danger">Retirer</button>` : entry.exportState === 'uncertain' ? `<button ${connectorSupports(state.connectors, 'time', 'reconcile', entry.exportConnector || state.connectors?.providers?.time) ? '' : 'hidden'} data-reconcile-time="${entry.id}">Vérifier auprès du service</button><button data-reset-time="${entry.id}">Débloquer après vérification…</button>` : ''}</div></article>`;
    }).join('') || '<p class="empty">Aucune saisie dans cette vue. Ajustez les filtres ou ajoutez du temps.</p>';
  }
  if (currentView === 'agents') html = queueMarkup() + html;
  if (['overview', 'time'].includes(currentView)) html += reportMarkup();
  const openJournals = new Set([...document.querySelectorAll('[data-process-journal][open]')].map(element => element.dataset.processJournal));
  $('#workspace-panel').innerHTML = html;
  bindWorkflowActions();
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
  if ($('#stop-visible-processes')) $('#stop-visible-processes').onclick = () => action(async () => {
    const results = await Promise.allSettled(processCards(project).filter(card => devRunning(card.id)).map(card => api(cardPath(card.id) + '/dev-servers', 'DELETE', {})));
    await refresh();
    const failure = results.find(result => result.status === 'rejected');
    if (failure) throw failure.reason;
  });
  document.querySelectorAll('[data-process-action]').forEach(button => button.onclick = () => action(async () => {
    button.disabled = true;
    try {
      const operation = button.dataset.processAction, serviceId = button.dataset.serviceId;
      await api(cardPath(button.dataset.processCard) + '/dev-servers', operation === 'stop' ? 'DELETE' : 'POST', { ...(serviceId ? { serviceId } : operation === 'start' ? { standalone: true } : {}), ...(operation === 'restart' ? { restart: true } : {}) });
      await refresh(); await refreshProcesses();
    } finally { if (button.isConnected) button.disabled = false; }
  }));
  document.querySelectorAll('[data-go-view]').forEach(b => b.onclick = () => { currentView = b.dataset.goView; if (b.dataset.focusTarget) { focusFilter = b.dataset.focusTarget; $('#search').value = ''; } board(); });
  document.querySelectorAll('[data-restore-card]').forEach(b => b.onclick = () => action(async () => { await api(cardPath(b.dataset.restoreCard) + '/archive', 'POST', { archived: false }); await refresh(); toast('Tâche restaurée.'); }));
  document.querySelectorAll('[data-open-card]').forEach(b => b.onclick = () => action(() => openCard(b.dataset.openCard)));
  document.querySelectorAll('[data-stop-agent]').forEach(b => b.onclick = () => action(async () => { await api(`/api/runs/${b.dataset.stopAgent}/cancel`, 'POST', {}); await refresh(); }));
  if ($('#inspect-worktrees')) $('#inspect-worktrees').onclick = () => action(async () => { const button = $('#inspect-worktrees'); button.disabled = true; try { worktreeChecks = await api('/api/worktrees'); renderWorkspace(); } finally { if (button.isConnected) button.disabled = false; } });
  document.querySelectorAll('[data-clean-worktree]').forEach(b => b.onclick = () => action(async () => { if (!(await confirmAction("Retirer le worktree principal et les worktrees Front / Back associés, et supprimer leurs fichiers locaux, y compris les modifications non commitées et les fichiers non suivis ? Cette perte est irréversible. Les branches et leurs commits seront conservés. Aucun ticket Jira ni autre service ne sera modifié.", "Retirer le worktree ?", "Retirer et supprimer les fichiers"))) return; await api(cardPath(b.dataset.cleanWorktree) + '/worktree', 'DELETE', { discardChanges: true }); worktreeChecks = []; await refresh(); }));
  if ($('#summary-day')) { $('#summary-day').onchange = event => { if (event.target.value) { summaryDay = event.target.value; board(); } }; $('#show-summary-day').onclick = () => { timeFilters = { from: summaryDay, to: summaryDay, status: 'all' }; board(); }; }
  if ($('#time-from')) {
    for (const key of ['from', 'to', 'status']) $('#time-' + key).onchange = event => { timeFilters[key] = event.target.value; board(); };
    $('#reset-time-filters').onclick = () => { timeFilters = { from: '', to: '', status: 'all' }; board(); };
    $('#export-time-csv').onclick = () => {
      const url = URL.createObjectURL(new Blob([timeCsv(times, cards)], { type: 'text/csv;charset=utf-8' }));
      const link = document.createElement('a'); link.href = url; link.download = 'atelier-temps.csv'; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast('CSV exporté : saisies arrêtées uniquement.');
    };
  }
  if ($('#add-time')) $('#add-time').onclick = () => openTime();
  document.querySelectorAll('[data-edit-time]').forEach(b => b.onclick = () => openTime(null, b.dataset.editTime));
  document.querySelectorAll('[data-stop-time]').forEach(b => b.onclick = () => timeAction(b.dataset.stopTime, 'stop'));
  document.querySelectorAll('[data-reconcile-time]').forEach(b => b.onclick = () => timeAction(b.dataset.reconcileTime, 'reconcile'));
  document.querySelectorAll('[data-reset-time]').forEach(b => b.onclick = async () => { if (await confirmAction("Confirmez avoir vérifié directement dans le service de temps que cette saisie n’existe pas. Débloquer un envoi déjà reçu peut créer un doublon.", "Débloquer cet envoi ?", "Débloquer l’envoi")) timeAction(b.dataset.resetTime, 'reset-export', { confirmed: true }); });
  document.querySelectorAll('[data-delete-time]').forEach(b => b.onclick = () => action(async () => { if (!(await confirmAction("Retirer cette saisie locale ?", "Supprimer la saisie ?", "Supprimer la saisie"))) return; await api('/api/time/' + b.dataset.deleteTime, 'DELETE', {}); await refresh(); }));
  document.querySelectorAll('[data-export-time]').forEach(b => b.onclick = () => action(async () => {
    tempoEntry = b.dataset.exportTime;
    const entry = state.times.find(e => e.id === tempoEntry), card = state.cards.find(c => c.id === entry.cardId);
    const preview = await api('/api/time/' + entry.id + '/export-preview');
    $('#tempo-preview').innerHTML = `<h3>${esc(card.ticket?.key || '')} · ${esc(card.title)}</h3><p>${esc(entry.date)} à ${esc(entry.startTime)} · <strong>${duration(entry.seconds)}</strong></p><p class="entry-description">${esc(entry.description)}</p><p class="hint">Destination : ${esc(connectorName('time'))}</p>`;
    if (preview?.message) { const message = document.createElement('p'); message.textContent = preview.message; $('#tempo-preview').append(message); }
    $('#tempo-dialog').showModal();
  }));
}
function timeAction(id, actionName, body = {}) { return action(async () => { await api(`/api/time/${id}/${actionName}`, 'POST', body); await refresh(); }); }
function openTime(cardId, entryId) {
  timeEdit = entryId || null;
  const entry = state.times?.find(e => e.id === entryId), form = $('#time-form');
  form.reset();
  const cards = cardId || entry ? state.cards : scopedCards();
  form.elements.cardId.innerHTML = cards.map(c => `<option value="${esc(c.id)}">${esc(c.ticket?.key || '')} ${esc(c.title)}</option>`).join('');
  form.elements.cardId.value = entry?.cardId || cardId || cards[0]?.id || '';
  form.elements.cardId.disabled = Boolean(entry);
  const now = new Date(), local = new Date(now.getTime() - now.getTimezoneOffset() * 60000).toISOString();
  form.elements.date.value = entry?.date || local.slice(0, 10);
  form.elements.startTime.value = entry?.startTime || local.slice(11, 19);
  form.elements.minutes.value = entry ? Math.max(0.02, Math.round(entry.seconds / 60 * 100) / 100) : 30;
  form.elements.description.value = entry?.description || state.cards.find(c => c.id === form.elements.cardId.value)?.title || '';
  $('#start-timer').hidden = Boolean(entry);
  $('#time-title').textContent = entry ? 'Corriger le temps' : 'Saisir du temps';
  $('#time-dialog').showModal();
}
async function saveTime(start) {
  const form = $('#time-form'); if (!form.reportValidity()) return;
  const body = Object.fromEntries(new FormData(form));
  body.cardId = form.elements.cardId.value;
  body.startTime = body.startTime.length === 5 ? body.startTime + ':00' : body.startTime;
  body.seconds = Math.round(Number(body.minutes) * 60); body.start = start;
  const buttons = [...form.querySelectorAll('button')]; buttons.forEach(b => b.disabled = true);
  try {
    await api(timeEdit ? '/api/time/' + timeEdit : '/api/time', timeEdit ? 'PATCH' : 'POST', body);
    $('#time-dialog').close(); $('#card-dialog').close(); currentView = 'time'; await refresh(); toast(start ? 'Chronomètre démarré. Retrouvez-le dans Temps.' : 'Temps enregistré localement.');
  } finally { buttons.forEach(b => b.disabled = false); }
}
document.querySelectorAll('[data-view]').forEach(b => b.onclick = () => { currentView = b.dataset.view; board(); if (currentView === 'processes') action(refreshProcesses); });
$('#time-form').onsubmit = e => { e.preventDefault(); action(() => saveTime(false)); };
$('#start-timer').onclick = () => action(() => saveTime(true));
$('#confirm-tempo').onclick = () => action(async () => {
  const button = $('#confirm-tempo'); button.disabled = true;
  try {
    const entry = await api(`/api/time/${tempoEntry}/export`, 'POST', { confirmed: true });
    $('#tempo-dialog').close(); await refresh(); toast(entry.exportState === 'exported' ? 'Temps exporté.' : entry.error, entry.exportState !== 'exported');
  } finally { button.disabled = false; }
});
$('#diagnostics-button').onclick = () => $('#diagnostics-dialog').showModal();
$('#test-connections').onclick = () => action(async () => {
  const button = $('#test-connections'); button.disabled = true; $('#diagnostics-results').textContent = 'Vérification en cours…';
  try {
    const repo = state.projects.find(p => p.id === $('#project-filter').value)?.repo || state.settings.repo;
    const results = await api('/api/diagnostics', 'POST', { repo });
    $('#diagnostics-results').innerHTML = results.map(r => `<article class="diagnostic"><strong>${r.ok ? '✓' : '!' } ${esc(r.name)}</strong><p class="${r.ok ? 'hint' : 'inline-error'}">${esc(r.message)}</p></article>`).join('');
  } finally { button.disabled = false; }
});
window.addEventListener('hashchange', () => {
  const params = new URLSearchParams(location.hash.slice(1));
  currentView = ['overview', 'board', 'brainstorm', 'agents', 'processes', 'worktrees', 'time', 'archives'].includes(params.get('view')) ? params.get('view') : 'overview';
  $('#project-filter').value = state.projects.some(p => p.id === params.get('project')) ? params.get('project') : '';
  board();
});


function compactCard() {
  const card = state.cards.find(c => c.id === selected);
  const context = document.createElement('h3'); context.textContent = 'La tâche';
  $('#card-form').prepend(context);
  const criteria = $('#criteria-list').closest('details');
  criteria.open = false;
  const statusRow = $('#card-form').elements.status.closest('.form-row');
  criteria.before(statusRow);
  const instructions = $('#instructions').closest('label');
  const options = document.createElement('details'); options.className = 'disclosure agent-options';
  options.open = Boolean(card?.instructions);
  const optionsTitle = document.createElement('summary'); optionsTitle.textContent = 'Instructions complémentaires';
  instructions.before(options); options.append(optionsTitle, instructions);
  const detail = $('#card-detail'), form = $('#card-form'), runBox = detail.querySelector('.run-box');
  form.after(runBox);
  const heading = document.createElement('h3'); heading.textContent = 'Travailler avec l’agent'; runBox.prepend(heading);
  detail.querySelectorAll('.worktree-box').forEach(section => {
    if (section.id === 'automation-status') return;
    const wrapper = document.createElement('details'); wrapper.className = 'disclosure';
    const title = section.querySelector('h3'), summary = document.createElement('summary'); summary.textContent = title.textContent === 'Worktree dédié' ? 'Dossier de travail · worktree' : title.textContent;
    section.before(wrapper); title.remove(); wrapper.append(summary, section);
    if (section.querySelector('#prepare-pr') && state.cards.find(c => c.id === selected)?.pullRequest) wrapper.open = true;
  });
  const journal = $('#run-details'), report = $('#agent-summary'); runBox.after(report); report.after(journal);
  const headingBar = document.createElement('nav'); headingBar.className = 'detail-nav'; headingBar.setAttribute('aria-label', 'Sections de la tâche');
  for (const [label, target] of [['Contexte', form], ['Agent', runBox], ['Résultat', report], ['PR & review', $('#prepare-pr').closest('details')]]) {
    if (label === 'PR & review' && !connectorSupports(state.connectors, 'repositories', 'preview', repositoryProvider(state, card))) { target.hidden = true; continue; }
    const button = document.createElement('button'); button.type = 'button'; button.textContent = label;
    button.onclick = () => { if (target.hidden) { journal.open = true; journal.scrollIntoView({ block: 'start', behavior: 'instant' }); return; } if (target.tagName === 'DETAILS') target.open = true; target.setAttribute('tabindex', '-1'); target.focus({ preventScroll: true }); target.scrollIntoView({ block: 'start', behavior: 'instant' }); }; headingBar.append(button);
  }
  detail.querySelector('.dialog-heading').after(headingBar);
  const editor = document.createElement('div'); editor.className = 'task-editor';
  form.before(editor); editor.append(form, runBox);
}
function renderTimerBanner() {
  const entry = (state.times || []).find(e => e.runningSince);
  $('#timer-banner').hidden = !entry;
  if (!entry) return;
  const card = state.cards.find(c => c.id === entry.cardId);
  $('#timer-task').textContent = card?.ticket?.key || card?.title || 'Temps humain';
  $('#timer-value').dataset.timeEntry = entry.id;
}
$('#timer-stop').onclick = () => action(async () => {
  const entry = (state.times || []).find(e => e.runningSince); if (!entry) return;
  const button = $('#timer-stop'); button.disabled = true;
  try { await api(`/api/time/${entry.id}/stop`, 'POST', {}); await refresh(); toast('Chronomètre arrêté. La saisie reste à valider dans Temps.'); }
  finally { button.disabled = false; }
});
$('#timer-open').onclick = () => {
  const entry = (state.times || []).find(e => e.runningSince), card = state.cards.find(c => c.id === entry?.cardId);
  const project = state.projects.find(p => sameRepository(p.repo, card?.repo, state.settings.platform));
  $('#project-filter').value = project?.id || ''; currentView = 'time'; board();
};
$('#welcome-projects').onclick = () => $('#projects-button').click();
$('#welcome-create').onclick = () => $('#create-button').click();
document.querySelectorAll('[data-focus]').forEach(b => b.onclick = () => { focusFilter = b.dataset.focus; board(); });
document.querySelectorAll('[data-template]').forEach(b => b.onclick = () => {
  const template = taskTemplates[b.dataset.template], form = $('#new-form');
  if (form.elements.description.value.trim() && !Object.values(taskTemplates).some(t => t.description === form.elements.description.value)) { toast('Le contexte déjà saisi est conservé. Videz-le pour appliquer un modèle.'); return; }
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

function rememberBrainstorm() {
  try { localStorage.setItem('atelier-brainstorm-page', JSON.stringify({ id: brainstormSession, reply: $('#brainstorm-reply').value, fields: Object.fromEntries([...$('#brainstorm-form').elements].filter(field => field.name).map(field => [field.name, field.value])) })); } catch {}
}

function loadBrainstormSession(id) {
  const form = $('#brainstorm-form'), session = [...(state.brainstorms || []), ...state.cards].find(c => c.id === id);
  brainstormSession = session?.id || '';
  $('#brainstorm-reply').value = '';
  form.reset();
  form.elements.agent.value = session?.agent || state.settings.agent || 'codex';
  taskAgentFields(form, session || {});
  if (session?.repo && ![...form.elements.repo.options].some(option => option.value === session.repo)) form.elements.repo.add(new Option(session.repo, session.repo));
  if (session) for (const name of ['title', 'description', 'instructions', 'repo']) form.elements[name].value = session[name] || '';
  const host = $('#brainstorm-page-proposals'); host.replaceChildren(); delete host.dataset.signature;
  rememberBrainstorm(); renderBrainstormPage();
}

function renderBrainstormPage() {
  const form = $('#brainstorm-form'), picker = $('#brainstorm-session');
  const sessions = [...(state.brainstorms || []), ...state.cards].filter(c => !c.archived && (c.brainstormSession || state.runs.some(r => r.cardId === c.id && r.kind === 'brainstorm')));
  const signature = JSON.stringify(sessions.map(c => [c.id, c.title]));
  if (picker.dataset.signature !== signature) {
    picker.innerHTML = '<option value="">Nouveau brainstorming</option>' + sessions.map(c => `<option value="${esc(c.id)}">${esc(c.title)}</option>`).join('');
    picker.dataset.signature = signature;
  }
  const projects = JSON.stringify(state.projects.map(p => [p.repo, p.name]));
  if (form.elements.repo.dataset.signature !== projects) {
    const value = form.elements.repo.value;
    form.elements.repo.innerHTML = '<option value="">Sans dépôt · brainstorming général</option>' + state.projects.map(p => `<option value="${esc(p.repo)}">${esc(p.name)}</option>`).join('');
    form.elements.repo.dataset.signature = projects;
    if (value && !state.projects.some(p => p.repo === value)) form.elements.repo.add(new Option(value, value));
    form.elements.repo.value = value;
  }
  if (!brainstormInitialized) {
    brainstormInitialized = true;
    let saved; try { saved = JSON.parse(localStorage.getItem('atelier-brainstorm-page') || 'null'); } catch {}
    brainstormSession = sessions.some(c => c.id === saved?.id) ? saved.id : '';
    $('#brainstorm-reply').value = brainstormSession && typeof saved?.reply === 'string' ? saved.reply : '';
    form.elements.agent.value = ['codex', 'claude'].includes(saved?.fields?.agent) ? saved.fields.agent : state.settings.agent || 'codex';
    taskAgentFields(form, saved?.fields || {});
    for (const [name, value] of Object.entries(saved?.fields || {})) {
      if (!form.elements[name] || typeof value !== 'string') continue;
      if (name === 'repo' && value && ![...form.elements.repo.options].some(o => o.value === value)) form.elements.repo.add(new Option(value, value));
      form.elements[name].value = value;
    }
    form.oninput = rememberBrainstorm;
    form.onchange = rememberBrainstorm;
    picker.onchange = () => loadBrainstormSession(picker.value);
    form.onsubmit = event => { event.preventDefault(); action(() => submitBrainstorm(false)); };
    $('#brainstorm-create-local').onclick = () => action(() => submitBrainstorm(true));
    $('#brainstorm-reply').oninput = () => { rememberBrainstorm(); resizeBrainstormComposer(); };
    $('#brainstorm-reply').onkeydown = event => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); if (!$('#brainstorm-send').disabled) $('#brainstorm-reply-form').requestSubmit(); }
    };
    $('#brainstorm-reply-form').onsubmit = event => { event.preventDefault(); action(() => submitBrainstorm(false, true)); };
  }
  picker.value = brainstormSession;
  const run = state.runs.find(r => r.cardId === brainstormSession && r.kind === 'brainstorm');
  const active = run?.status === 'running';
  for (const field of form.elements) field.disabled = active || brainstormingBusy;
  renderBrainstormChat(active);
  resizeBrainstormComposer();
  const status = $('#brainstorm-page-status');
  status.textContent = active ? `${run.activity || 'Brainstorming en cours…'}${run.liveText ? '\n' + run.liveText : ''}` : run ? run.proposals ? 'Choisissez les propositions à transformer en tâches locales. Chaque tâche reste modifiable et peut ensuite être envoyée dans Jira.' : run.attention || 'Brainstorming interrompu ou échoué. Vous pouvez relancer l’exploration.' : 'Décrivez une idée pour démarrer, ou créez directement une tâche locale.';
  $('#brainstorm-page-stop').hidden = !active;
  $('#brainstorm-page-stop').onclick = () => action(async () => { await api(`/api/runs/${run.id}/cancel`, 'POST', {}); await refresh(); });
  if (run) renderBrainstorm(brainstormSession, $('#brainstorm-page-proposals'));
}

async function submitBrainstorm(local, replying = false) {
  const form = $('#brainstorm-form');
  if (brainstormingBusy || state.runs.some(run => run.cardId === brainstormSession && run.status === 'running') || !form.reportValidity()) return;
  const reply = replying ? $('#brainstorm-reply').value.trim() : '';
  if (replying && (!brainstormSession || !reply || !$('#brainstorm-reply-form').reportValidity())) return;
  const body = Object.fromEntries(new FormData(form));
  rememberBrainstorm();
  brainstormingBusy = true;
  try {
    renderBrainstormPage();
    if (local) {
      const card = await api('/api/cards', 'POST', { ...body, mode: 'edit' });
      await refresh(); await openCard(card.id); toast('Tâche locale créée. Jira est facultatif.');
      return;
    }
    const session = await api(brainstormSession ? cardPath(brainstormSession) : '/api/brainstorms', brainstormSession ? 'PATCH' : 'POST', body);
    brainstormSession = session.id;
    await api(cardPath(session.id) + '/brainstorm', 'POST', { reply });
    if (replying) $('#brainstorm-reply').value = '';
  } finally {
    brainstormingBusy = false;
    await refresh(); rememberBrainstorm(); renderBrainstormPage();
  }
}

function renderBrainstormChat(active) {
  const discussing = Boolean(brainstormSession), context = $('#brainstorm-context');
  if (context.dataset.session !== brainstormSession) { context.open = !discussing; context.dataset.session = brainstormSession; }
  $('#brainstorm-editor-kicker').textContent = discussing ? 'VOTRE DISCUSSION' : 'DE L’IDÉE À LA TÂCHE';
  $('#brainstorm-editor-title').textContent = discussing ? 'Affinons votre idée' : 'Qu’avez-vous en tête ?';
  $('#brainstorm-editor-intro').hidden = discussing;
  $('#brainstorm-chat').hidden = !discussing;
  $('#brainstorm-start').textContent = discussing ? 'Actualiser les propositions' : 'Explorer avec l’agent';
  $('#brainstorm-reply').disabled = $('#brainstorm-send').disabled = active || brainstormingBusy;
  $('#brainstorm-composer-agent').textContent = $('#brainstorm-form').elements.agent.value === 'claude' ? '✳ Claude' : '⌘ Codex';
  if (!discussing) return;
  const turns = state.runs.filter(run => run.cardId === brainstormSession && run.kind === 'brainstorm').slice().reverse();
  const host = $('#brainstorm-chat-history');
  const signature = JSON.stringify(turns.map(run => [run.id, run.status, run.instructions, run.requirements, run.proposals, run.attention, run.activity]));
  if (host.dataset.signature === signature) return;
  const follow = host.scrollHeight - host.scrollTop - host.clientHeight < 80;
  host.dataset.signature = signature;
  host.innerHTML = turns.map((run, index) => {
    const user = index === 0 ? [run.requirements?.title, run.requirements?.description, run.requirements?.instructions, run.instructions].filter(Boolean).join('\n\n') : run.instructions || 'Actualise les propositions à partir du besoin et des décisions précédentes.';
    const answer = run.proposals ? `<p>${run.proposals.length} proposition(s) à retrouver à droite.</p>${run.proposals.map(proposal => proposal.questions.length ? `<p><strong>${esc(proposal.title)}</strong></p><ul>${proposal.questions.map(question => `<li>${esc(question)}</li>`).join('')}</ul>` : '').join('')}${run.proposals.every(proposal => !proposal.questions.length) ? '<p>Les points sont clarifiés. Vous pouvez créer les tâches ou demander d’autres ajustements.</p>' : '<p>Vous pouvez répondre ci-dessous pour affiner les propositions.</p>'}` : `<p>${esc(run.status === 'running' ? run.activity || 'Réflexion en cours…' : run.attention || 'Exploration interrompue ou échouée. Votre message reste dans la discussion ; vous pouvez réessayer.')}</p>`;
    return `<article class="brainstorm-message user"><strong>Vous</strong><p>${esc(user)}</p></article><article class="brainstorm-message assistant"><strong>${run.agent === 'claude' ? 'Claude' : 'Codex'}</strong>${answer}</article>`;
  }).join('') || '<p class="hint">Envoyez un message pour poursuivre votre idée.</p>';
  if (follow) host.scrollTop = host.scrollHeight;
}

function resizeBrainstormComposer() {
  const input = $('#brainstorm-reply');
  input.style.height = 'auto';
  input.style.height = Math.min(180, Math.max(30, input.scrollHeight)) + 'px';
  input.style.overflowY = input.scrollHeight > 180 ? 'auto' : 'hidden';
  $('#brainstorm-send').disabled = input.disabled || !input.value.trim();
}

function setupBrainstorm(card, active, saveCard) {
  const section = document.createElement('section'); section.className = 'worktree-box';
  section.innerHTML = `<h3>Brainstorming & création de tâches</h3><p class="hint">L’agent découpe votre idée en propositions de tickets, en lecture seule. Consomme ses quotas. Créez une tâche locale pour la relire et la modifier. Un connecteur compatible peut ensuite la publier dans votre outil de tickets.</p><button type="button" id="brainstorm-ticket" ${active ? 'disabled' : ''}>Proposer des tickets avec l’agent</button><div id="brainstorm-proposals" aria-live="polite"></div>${card.ticket || state.connectors?.providers?.tickets !== 'jira' ? '' : `<form id="jira-create-form"><p class="hint">Destination : ${esc(state.settings.jiraUrl || 'Connectez Jira dans les réglages')} · la création utilise le contenu enregistré de cette fiche, questions comprises.</p><div class="form-row"><label>Clé du projet Jira<input name="project" ${card.ticketCreation ? '' : 'required'} pattern="[A-Z][A-Z0-9_]*" maxlength="100" placeholder="PROJ"></label><label>Nom exact du type Jira<input name="issueType" ${card.ticketCreation ? '' : 'required'} maxlength="100" placeholder="Task, Story, Tâche…"></label></div>${card.ticketCreation ? '<p class="inline-error">Envoi précédent incertain. Vérifiez Jira avant de réessayer pour éviter un doublon.</p><label>Ticket déjà créé à rattacher<input name="existingKey" placeholder="PROJ-123"></label><label class="check"><input name="confirmRetry" type="checkbox"> J’ai vérifié dans Jira que le ticket n’existe pas et je souhaite réessayer.</label>' : ''}<button id="publish-jira" ${active ? 'disabled' : ''}>Relire et créer dans Jira</button></form>`}`;
  $('#card-form').after(section);
  if (!card.ticket && state.connectors?.providers?.tickets !== 'jira' && ticketSupports(card, 'create')) {
    const publish = document.createElement('button'); publish.type = 'button'; publish.disabled = active;
    publish.textContent = 'Créer dans ' + connectorName('tickets');
    publish.onclick = () => action(async () => {
      if (!$('#card-form').reportValidity()) return;
      if (!(await confirmAction('Envoyer le contenu de cette carte à ' + connectorName('tickets') + ' ?', 'Créer le ticket distant ?', 'Créer le ticket'))) return;
      publish.disabled = true;
      try { await saveCard(); await api(cardPath(card.id) + '/create-ticket', 'POST', {}); toast('Ticket distant créé.'); }
      finally { await refresh(); renderDetail(); await updateRun(); }
    });
    section.append(publish);
  }
  $('#brainstorm-ticket').onclick = () => action(async () => {
    if (!$('#card-form').reportValidity()) return;
    $('#brainstorm-ticket').disabled = true;
    try {
      await saveCard(true);
      const run = await api(cardPath(card.id) + '/brainstorm', 'POST', {});
      await refresh(); renderDetail(); selectedRun = run.id; $('#run-select').value = run.id; await updateRun();
    } finally { if ($('#brainstorm-ticket')) $('#brainstorm-ticket').disabled = state.runs.some(r => r.cardId === card.id && r.status === 'running') || card.archived; }
  });
  renderBrainstorm(card.id);
  if ($('#jira-create-form')) $('#jira-create-form').onsubmit = e => {
    e.preventDefault(); action(async () => {
      if (!$('#card-form').reportValidity()) return;
      const body = Object.fromEntries(new FormData(e.target)); body.confirmRetry = e.target.elements.confirmRetry?.checked === true;
      if (body.existingKey) { delete body.project; delete body.issueType; }
      await saveCard(true);
      if (!(await confirmAction(`Site : ${state.settings.jiraUrl}\nProjet : ${body.project || body.existingKey}\nType : ${body.issueType || 'Rattachement'}\nTitre : ${$('#card-form').elements.title.value}\n\n${$('#card-form').elements.description.value}\n\nLes critères et les instructions de la fiche seront également envoyés.`, body.existingKey ? 'Rattacher le ticket existant ?' : 'Créer ce ticket Jira ?', body.existingKey ? 'Rattacher' : 'Créer dans Jira'))) return;
      $('#publish-jira').disabled = true;
      try { await api(cardPath(card.id) + '/create-ticket', 'POST', body); toast('Ticket Jira lié à la carte.'); }
      finally { await refresh(); renderDetail(); await updateRun(); }
    });
  };
  if (card.archived) section.querySelectorAll('button, input').forEach(el => el.disabled = true);
}

function renderBrainstorm(cardId, host = $('#brainstorm-proposals')) {
  if (!host) return;
  const latest = state.runs.find(r => r.cardId === cardId && r.kind === 'brainstorm');
  const run = latest?.proposals ? latest : state.runs.find(r => r.cardId === cardId && r.kind === 'brainstorm' && r.proposals) || latest;
  if (!run) return;
  const signature = JSON.stringify([latest.id, latest.status, run.id, run.status, run.attention, run.proposals, state.cards.filter(c => c.brainstormSource?.runId === run.id).map(c => c.id)]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  if (run.status === 'running') { host.textContent = 'Brainstorming en cours… Suivez l’agent dans le journal.'; return; }
  if (!run.proposals) { host.textContent = run.attention || 'Brainstorming interrompu ou échoué. Consultez le journal.'; return; }
  host.innerHTML = run.proposals.map((proposal, index) => {
    const draft = state.cards.find(c => c.brainstormSource?.runId === run.id && c.brainstormSource.index === index);
    return `<details class="disclosure"><summary>${esc(proposal.title)}</summary><p class="path-text">${esc(proposal.description)}</p><ul>${proposal.checklist.map(item => `<li>${esc(item.text)}</li>`).join('')}</ul>${proposal.questions.length ? `<p>Questions à trancher :</p><ul>${proposal.questions.map(q => `<li>${esc(q)}</li>`).join('')}</ul>` : ''}<button type="button" data-brainstorm-index="${index}" ${latest.status === 'running' && !draft ? 'disabled' : ''}>${draft ? 'Ouvrir la tâche créée' : 'Créer une tâche locale'}</button></details>`;
  }).join('');
  host.querySelectorAll('[data-brainstorm-index]').forEach(button => button.onclick = () => action(async () => {
    button.disabled = true;
    try {
      const existing = state.cards.find(c => c.brainstormSource?.runId === run.id && c.brainstormSource.index === Number(button.dataset.brainstormIndex));
      if (existing) { await openCard(existing.id); return; }
      const draft = await api(cardPath(cardId) + '/brainstorm-draft', 'POST', { runId: run.id, index: Number(button.dataset.brainstormIndex) });
      await refresh();
      if (host.id === 'brainstorm-page-proposals') toast('Tâche locale créée. Vous pouvez l’ouvrir pour la modifier ou l’envoyer dans Jira.');
      else await openCard(draft.id);
    } finally { button.disabled = false; }
  }));
}

function renderTicketPreparation(cardId) {
  const host = $('#ticket-proposal'); if (!host) return;
  const run = state.runs.find(r => r.cardId === cardId && r.kind === 'ticket-preparation');
  if (!run) return;
  const card = state.cards.find(c => c.id === cardId);
  const requirements = { title: card.title || '', description: card.description || '', instructions: card.instructions || '', criteria: (card.checklist || []).map(i => i.text) };
  const stale = JSON.stringify(requirements) !== JSON.stringify(run.requirements);
  const signature = JSON.stringify([run.id, run.status, run.attention, stale]);
  if (host.dataset.signature !== signature) {
    host.dataset.signature = signature;
    if (run.status === 'running') host.innerHTML = '<p role="status">Préparation en cours… Suivez son activité dans le journal.</p>';
    else if (run.status !== 'completed' || !run.preparation) host.innerHTML = `<p class="inline-error">${esc(run.attention || 'Préparation interrompue ou échouée. Consultez le journal avant de relancer.')}</p>`;
    else host.innerHTML = `<p class="hint">Proposition à relire. Un critère par ligne (20 maximum, 500 caractères chacun). Les questions restent à trancher avant le développement.</p><label>Critères proposés<textarea id="proposed-criteria" rows="6">${esc(run.preparation.checklist.map(i => i.text).join('\n'))}</textarea></label><label>Questions à trancher (une par ligne, 10 maximum)<textarea id="proposed-questions" rows="4">${esc(run.preparation.questions.join('\n'))}</textarea></label>${stale ? '<p class="inline-error">Le besoin enregistré a changé. Relancez la préparation.</p>' : ''}<button type="button" id="apply-ticket-proposal">Remplacer les critères dans la fiche</button><p class="hint">Les critères seront décochés et les questions ajoutées aux instructions locales. Enregistrez ensuite la carte pour conserver le résultat.</p>`;
  }
  const apply = $('#apply-ticket-proposal'); if (!apply) return;
  const locked = stale || card.archived || card.publishing || state.runs.some(r => r.cardId === cardId && r.status === 'running');
  for (const field of host.querySelectorAll('textarea, button')) field.disabled = Boolean(locked);
  apply.disabled ||= host.dataset.applied === run.id;
  apply.onclick = () => action(async () => {
    const lines = selector => $(selector).value.split('\n').map(s => s.trim()).filter(Boolean);
    const criteria = lines('#proposed-criteria'), questions = lines('#proposed-questions');
    if (!criteria.length || criteria.length > 20 || questions.length > 10 || [...criteria, ...questions].some(s => s.length > 500)) return toast('De 1 à 20 critères et au plus 10 questions, de 500 caractères chacun.', true);
    const instructions = $('#instructions');
    const nextInstructions = instructions.value + (questions.length ? '\n\nQuestions à trancher avant développement :\n' + questions.map(q => '- ' + q).join('\n') : '');
    if (nextInstructions.length > 20000) return toast('Les instructions dépasseraient 20 000 caractères. Raccourcissez-les avant de transférer la proposition.', true);
    $('#criteria-list').replaceChildren(); criteria.forEach(text => addCriterion({ text, done: false }, false));
    $('#criteria-list').closest('details').open = true;
    instructions.value = nextInstructions;
    $('#card-form').dispatchEvent(new Event('input', { bubbles: true }));
    host.dataset.applied = run.id;
    apply.disabled = true;
    toast('Proposition transférée. Relisez puis enregistrez la carte.');
  });
}

function addCriterion(item, disabled) {
  const row = document.createElement('div'); row.className = 'criterion-row';
  row.innerHTML = `<input type="checkbox" aria-label="Critère vérifié" ${item.done ? 'checked' : ''} ${disabled ? 'disabled' : ''}><input type="text" aria-label="Critère de réussite" maxlength="500" required value="${esc(item.text)}" placeholder="Ex. L’export respecte les filtres sélectionnés" ${disabled ? 'disabled' : ''}><button type="button" aria-label="Retirer ce critère" ${disabled ? 'disabled' : ''}>×</button>`;
  row.querySelector('button').onclick = () => { row.remove(); $('#card-form').dispatchEvent(new Event('input', { bubbles: true })); }; $('#criteria-list').append(row);
}
function overviewMarkup(cards, runs) {
  const review = cards.filter(c => c.status === 'review'), attention = cards.filter(c => matchesFocus(c, runs, 'attention', state.cards));
  const active = runs.filter(r => r.status === 'running');
  const ids = new Set(scopedCards(true).map(c => c.id));
  const pendingTime = (state.times || []).filter(e => ids.has(e.cardId) && !e.runningSince && e.exportState !== 'exported');
  const priority = [...attention, ...review.filter(c => !attention.includes(c)), ...cards.filter(c => c.status === 'todo' && !attention.includes(c))].slice(0, 6);
  return `${cards.filter(c => c.automation && c.status !== 'done' && ['ready', 'blocked', 'error', 'interrupted', 'cancelled'].includes(c.automation.state)).map(c => `<article class="worktree-box"><strong>${esc(c.title)}</strong><p>${esc(c.automation.message)}</p>${c.automation.acceptance ? `<p>${c.automation.acceptance.total ? `Critères : ${c.automation.acceptance.covered}/${c.automation.acceptance.total} couverts · ${c.automation.acceptance.notCovered} non couverts · ${c.automation.acceptance.unverifiable} impossibles à vérifier` : 'Aucun critère de réussite défini.'}</p>` : ''}${c.automation.counts ? `<p>${Object.entries(c.automation.counts).map(([p, n]) => `${p === 'suggestion' ? 'Suggestions' : p} : ${n}`).join(' · ')}</p>` : ''}<button data-open-card="${esc(c.id)}">Consulter le résultat</button>${c.pullRequest ? ` <a href="${esc(c.pullRequest.url)}" target="_blank" rel="noopener noreferrer">Ouvrir la PR ↗</a>` : ''}</article>`).join('')}<div class="overview-metrics"><button data-go-view="board" data-focus-target="review"><span>À vérifier</span><strong>${review.length}</strong><small>Relire le travail terminé →</small></button><button data-go-view="board" data-focus-target="attention"><span>À reprendre</span><strong>${attention.length}</strong><small>Examiner les interruptions →</small></button><button data-go-view="agents"><span>Agents actifs</span><strong>${active.length}</strong><small>Suivre les exécutions →</small></button><button data-go-view="time"><span>Temps à valider</span><strong>${pendingTime.length}</strong><small>Relire avant l’envoi Tempo →</small></button></div>
  <div class="overview-columns"><section class="overview-section"><div class="panel-heading"><h2>Votre prochain pas</h2></div>${priority.map(card => `<button class="next-task" data-open-card="${esc(card.id)}"><span class="state-badge">${attention.includes(card) ? 'À reprendre' : card.status === 'review' ? 'À vérifier' : 'À faire'}</span><strong>${esc(card.title)}</strong><small>${esc(card.ticket?.key || 'Tâche locale')}${card.checklist?.length ? ` · ${card.checklist.filter(c => c.done).length}/${card.checklist.length} critères validés` : ''}</small><span class="next-arrow">→</span></button>`).join('') || '<p class="empty">Rien à reprendre ou à relire. Vous pouvez préparer la suite.</p>'}</section>
  <section class="overview-section"><div class="panel-heading"><h2>Dernières exécutions</h2></div>${runs.slice(0, 5).map(run => { const card = cards.find(c => c.id === run.cardId); return `<button class="recent-run" data-open-card="${esc(card.id)}"><span class="agent-pill ${esc(run.agent)}">${run.agent === 'claude' ? 'Claude' : 'Codex'}</span><strong>${esc(card.title)}</strong><small data-agent-state="${run.id}">${esc(agentState(run))}</small><small data-clock="${run.id}"></small></button>`; }).join('') || '<p class="empty">Vos agents apparaîtront ici dès leur première tâche.</p>'}<button class="text-button" data-go-view="worktrees">Voir les branches, worktrees et PR →</button></section></div>`;
}
function openCommands() {
  $('#command-query').value = ''; commandIndex = 0; renderCommands(); $('#command-dialog').showModal(); $('#command-query').focus();
}
function renderCommands() {
  commandResults = paletteItems(state, $('#command-query').value); commandIndex = Math.min(commandIndex, Math.max(0, commandResults.length - 1));
  $('#command-results').innerHTML = commandResults.map((item, index) => `<button type="button" class="command-result" data-command-index="${index}" aria-current="${index === commandIndex ? 'true' : 'false'}"><span class="command-kind">${item.kind === 'project' ? 'PROJET' : item.kind === 'card' ? 'TÂCHE' : 'ACTION'}</span><span><strong>${esc(item.title)}</strong><small>${esc(item.detail)}</small></span><span>↵</span></button>`).join('') || '<p class="empty">Aucun résultat. Essayez un titre, un ID Jira ou un nom de projet.</p>';
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


function renderReviewReport(cardId) {
  const host = $('#pr-review-result'); if (!host) return;
  const run = state.runs.find(r => r.id === selectedRun && r.cardId === cardId && r.kind === 'pr-review') || state.runs.find(r => r.cardId === cardId && r.kind === 'pr-review');
  if (!run) { host.textContent = 'Aucune review lancée.'; return; }
  const reportSignature = JSON.stringify([run, state.cards.find(c => c.id === cardId)?.publishing, state.cards.find(c => c.id === cardId)?.archived, state.runs.some(r => r.cardId === cardId && r.status === 'running')]);
  if (host.dataset.signature === reportSignature) return; host.dataset.signature = reportSignature;
  const labels = { P0: 'Critique', P1: 'Important', P2: 'Modéré', P3: 'Mineur', suggestion: 'Suggestion facultative' };
  const context = run.reviewContext;
  host.innerHTML = `<p class="hint">Review du ${esc(new Date(run.startedAt).toLocaleString('fr-FR'))} · source ${esc(context?.source?.slice(0, 12))} / cible ${esc(context?.target?.slice(0, 12))} · ${esc(runNames[run.status])}</p>`;
  if (run.status === 'running') { host.innerHTML += '<p class="hint">Analyse en cours… Suivez son activité dans le journal.</p>'; return; }
  if (!run.review || run.status !== 'completed') { host.innerHTML += `<p class="inline-error">${esc(run.reviewError || 'Review non aboutie. Consultez le journal avant de relancer.')}</p>`; return; }
  host.innerHTML += `<p class="entry-description">${esc(run.review.summary)}</p><p class="hint">${run.review.findings.filter(f => f.priority !== 'suggestion').length} problème(s), ${run.review.findings.filter(f => f.priority === 'suggestion').length} suggestion(s). Ce rapport concerne uniquement les commits indiqués ; relancez après toute nouvelle publication.</p>`;
  const card = state.cards.find(c => c.id === cardId), locked = card.archived || card.publishing || state.runs.some(r => r.cardId === cardId && r.status === 'running');
  const coverageLabels = { covered: 'Couvert', 'not-covered': 'Non couvert', unverifiable: 'Impossible à vérifier' };
  host.innerHTML += '<h4>Couverture des critères de réussite</h4><p class="hint">Évaluation de l’agent sur le besoin au moment de la review. Les cases restent à valider par vous.</p>';
  host.innerHTML += !run.review.criteria ? '<p class="hint">Cette ancienne review ne contient pas de bilan des critères. Relancez-la pour obtenir ce contrôle.</p>' : !run.review.criteria.length ? '<p class="inline-error">Aucun critère de réussite défini lors de cette review : la couverture fonctionnelle n’a pas été évaluée.</p>' : run.review.criteria.map(item => `<article class="review-finding"><h4>${esc(coverageLabels[item.status])} · ${esc(item.text)}</h4><p class="entry-description">${esc(item.evidence)}</p></article>`).join('');
  if (run.reviewStaleReason) host.innerHTML += `<p class="inline-error">Review à refaire : ${esc(run.reviewStaleReason)}</p>`;
  host.innerHTML += run.review.findings.map((f, index) => `<article class="review-finding"><label class="check"><input type="checkbox" data-finding="${index}" ${locked || run.reviewStaleReason ? 'disabled' : ''}> Inclure dans la correction</label><h4><span class="review-priority priority-${f.priority}">${f.priority} · ${labels[f.priority]}</span> ${esc(f.title)}</h4><code>${esc(f.file)}${f.line ? ':' + f.line : ''}</code><p>${esc(f.problem)}</p><p><strong>Correction proposée :</strong> ${esc(f.suggestion)}</p></article>`).join('');
  host.innerHTML += `<div class="row-actions"><button id="check-review" ${locked ? 'disabled' : ''}>Vérifier l’actualité</button>${run.review.findings.length ? `<button id="fix-review" class="primary" ${locked || run.reviewStaleReason ? 'disabled' : ''}>Corriger la sélection avec l’agent</button>` : ''}</div><p class="hint">La correction utilise le même worktree. Ensuite : relisez, publiez les changements, puis relancez Review. Aucune publication automatique.</p>`;
  $('#check-review').onclick = () => action(async () => { $('#check-review').disabled = true; try { await api(cardPath(cardId) + '/check-review', 'POST', { runId: run.id }); await refresh(); renderReviewReport(cardId); } finally { if ($('#check-review')) $('#check-review').disabled = locked; } });
  if ($('#fix-review')) $('#fix-review').onclick = () => action(async () => {
    const indexes = [...host.querySelectorAll('[data-finding]:checked')].map(e => Number(e.dataset.finding));
    if (!indexes.length) return toast('Sélectionnez au moins un problème à corriger.', true);
    $('#fix-review').disabled = true;
    try { const correction = await api(cardPath(cardId) + '/fix-review', 'POST', { runId: run.id, indexes }); await refresh(); renderDetail(); selectedRun = correction.id; $('#run-select').value = correction.id; await updateRun(); toast('Correction lancée dans le worktree de la tâche.'); }
    finally { await refresh(); if ($('#pr-review-result')) renderReviewReport(cardId); }
  });
  if (!run.review.findings.length) host.insertAdjacentHTML('beforeend', '<p>Aucun problème étayé relevé. Cela ne garantit pas l’absence de défaut.</p>');
}


function projectConfig(repo) {
  const project = state.projects.find(p => sameRepository(p.repo, repo, state.settings.platform));
  return { ...state.settings, ...(project?.baseBranch ? { baseBranch: project.baseBranch } : {}), ...(project?.jiraMode === 'off' ? { jiraAutoTransition: false } : project?.jiraMode === 'custom' ? { jiraAutoTransition: true, jiraStatusMap: project.jiraStatusMap } : {}) };
}
function updateAutomation() {
  const host = $('#automation-status'), card = state.cards.find(c => c.id === selected);
  if (!host || !card) return;
  const flow = card.automation, project = state.projects.find(p => sameRepository(p.repo, card.repo, state.settings.platform));
  host.hidden = !flow && !project?.automationEnabled;
  const signature = JSON.stringify([flow, card.archived, card.publishing, card.pullRequest]);
  if (host.dataset.signature === signature) return; host.dataset.signature = signature;
  const steps = { development: 'Développement', tests: 'Vérifications', publish: 'Publication PR', review: 'Review', correction: 'Correction P0/P1' };
  const states = { running: 'En cours', ready: 'À vérifier par toi', blocked: 'Intervention nécessaire', error: 'Échec', interrupted: 'Interrompu', cancelled: 'Arrêté' };
  host.innerHTML = `<h3>Parcours automatique</h3><p role="status">${flow ? `${esc(steps[flow.step])} · ${esc(states[flow.state])} · ${flow.cycles}/2 cycles de correction` : 'Activé pour ce projet. Le prochain lancement en modification déroulera le parcours complet.'}</p>${flow?.message ? `<p>${esc(flow.message)}</p>` : ''}${flow?.acceptance ? `<p>${flow.acceptance.total ? `Critères : ${flow.acceptance.covered}/${flow.acceptance.total} couverts · ${flow.acceptance.notCovered} non couverts · ${flow.acceptance.unverifiable} impossibles à vérifier` : 'Aucun critère de réussite défini.'}</p>` : ''}${flow?.counts ? `<p>${Object.entries(flow.counts).map(([p, n]) => `${p === 'suggestion' ? 'Suggestions' : p} : ${n}`).join(' · ')}</p>` : ''}${card.pullRequest ? `<p><a href="${esc(card.pullRequest.url)}" target="_blank" rel="noopener noreferrer">Ouvrir la PR #${card.pullRequest.number} ↗</a></p>` : ''}${flow?.tests ? `<details><summary>Vérifications : ${esc({ passed: 'réussies', failed: 'échouées', running: 'en cours' }[flow.tests.state])}</summary><pre class="log">${esc(flow.tests.command)}\n${esc(flow.tests.output)}\n${esc(flow.tests.error || '')}</pre></details>` : ''}${flow?.state === 'running' ? '<button type="button" id="stop-automation" class="danger">Arrêter le parcours</button>' : ['error', 'interrupted', 'cancelled'].includes(flow?.state) ? `<button type="button" id="resume-automation" ${card.archived || card.publishing ? 'disabled' : ''}>Reprendre l’étape</button>` : ''}${flow?.reportId ? '<button type="button" id="show-automation-review">Consulter la review</button>' : ''}`;
  if ($('#stop-automation')) $('#stop-automation').onclick = () => action(async () => { await api(cardPath(card.id) + '/stop-automation', 'POST', {}); await refresh(); });
  if ($('#resume-automation')) $('#resume-automation').onclick = () => action(async () => { await api(cardPath(card.id) + '/resume-automation', 'POST', {}); await refresh(); renderDetail(); await updateRun(); });
  if ($('#show-automation-review')) $('#show-automation-review').onclick = () => action(async () => { selectedRun = flow.reportId; renderDetail(); $('#run-select').value = selectedRun; await updateRun(); $('#pr-review-result')?.scrollIntoView({ block: 'start' }); });
}
function updateDevServers() {
  if (!$('#dev-status')) return;
  const card = state.cards.find(card => card.id === selected), session = (state.devServers || []).find(session => session.cardId === selected);
  if ($('#service-worktree-paths')) $('#service-worktree-paths').textContent = Object.entries(card?.serviceWorktrees || {}).map(([role, worktree]) => `Worktree ${role} : ${worktree.path} · ${worktree.branch}`).join(' · ');
  const project = state.projects.find(project => sameRepository(project.repo, card?.repo, state.settings.platform));
  const linkedProject = state.projects.find(item => item.id === project?.linkedProjectId);
  $('#dev-context').textContent = session && devRunning(selected)
    ? session.services.map(service => `${service.name} : ${service.repo}`).join(' · ')
    : `${project?.name || 'Projet de la carte'} : ${card?.worktree?.path || 'worktree à préparer'}${linkedProject ? ` · ${linkedProject.name} (dépôt local) : ${linkedProject.repo}` : project?.linkedProjectId ? ' · Projet lié introuvable : vérifie les réglages.' : ''}`;
  $('#start-dev').disabled = !card?.worktree || card.archived || devRunning(selected);
  $('#stop-dev').disabled = !devRunning(selected);
  $('#dev-status').textContent = session ? session.services.map(service => `${service.name} : ${service.status === 'running' ? 'en cours' : service.status === 'stopping' ? 'arrêt demandé' : 'arrêté (code ' + service.exitCode + ')'}`).join(' · ') : 'Aucun serveur lancé.';
  $('#dev-log').textContent = session?.log || '';
  $('#remove-worktree').disabled = !card?.worktree || card?.publishing || state.runs.some(run => run.cardId === selected && run.status === 'running') || devRunning(selected);
}
function clearDraft(key) { try { localStorage.removeItem('atelier-draft:' + key); } catch {} }
function attachDraft(form, key, host) {
  host.querySelector('.draft-notice')?.remove();
  const notice = document.createElement('div'); notice.className = 'draft-notice';
  (host.querySelector('.detail-nav') || host.querySelector('.dialog-heading')).after(notice);
  const snapshot = () => ({ fields: Object.fromEntries([...form.elements].filter(e => e.name && e.name !== 'status' && e.type !== 'password').map(e => [e.name, e.value])), ...(key !== 'new' ? { blockedBy: [...($('#blocked-by')?.selectedOptions || [])].map(o => o.value), checklist: [...$('#criteria-list').children].map(row => ({ text: row.querySelector('input[type=text]').value, done: row.querySelector('input[type=checkbox]').checked })) } : {}) });
  let saved; try { saved = JSON.parse(localStorage.getItem('atelier-draft:' + key) || 'null'); } catch { clearDraft(key); }
  if (saved?.fields && typeof saved.fields === 'object') {
    notice.innerHTML = '<span>Brouillon non enregistré disponible.</span><button type="button" data-restore-draft>Reprendre</button><button type="button" data-discard-draft>Oublier</button>';
    notice.querySelector('[data-discard-draft]').onclick = () => { clearDraft(key); notice.textContent = 'Brouillon supprimé.'; };
    notice.querySelector('[data-restore-draft]').onclick = () => {
      if (['codex', 'claude'].includes(saved.fields.agent)) { form.elements.agent.value = saved.fields.agent; taskAgentFields(form); }
      for (const [name,value] of Object.entries(saved.fields)) { const field = form.elements.namedItem(name); if (field && name !== 'status' && !field.readOnly && !field.disabled && typeof value === 'string') field.value = value; }
      if (key !== 'new' && Array.isArray(saved.blockedBy) && $('#blocked-by')) for (const option of $('#blocked-by').options) option.selected = saved.blockedBy.includes(option.value);
      if (key !== 'new' && Array.isArray(saved.checklist)) { $('#criteria-list').replaceChildren(); saved.checklist.slice(0,20).filter(i => typeof i.text === 'string' && typeof i.done === 'boolean').forEach(i => addCriterion(i, false)); }
      notice.textContent = 'Brouillon repris. Enregistrez la carte pour appliquer les modifications.';
    };
  } else notice.textContent = 'Vos saisies sont conservées en brouillon dans ce navigateur.';
  host.oninput = event => {
    if (event.target !== form && event.target.form !== form) return;
    try { localStorage.setItem('atelier-draft:' + key, JSON.stringify(snapshot())); notice.textContent = 'Brouillon conservé dans ce navigateur. Modifications non enregistrées.'; }
    catch { notice.textContent = 'Brouillon indisponible : enregistrez avant de fermer.'; }
  };

}


async function renderBackups() {
  const result = await api('/api/backups');
  $('#backups-directory').textContent = 'Dossier : ' + result.directory;
  $('#backup-list').innerHTML = result.backups.map(b => `<article class="workspace-row"><div><strong>${esc(new Date(b.date).toLocaleString('fr-FR'))}</strong><p class="hint">${Math.ceil(b.bytes / 1024)} Ko</p></div><button data-restore-backup="${esc(b.name)}">Restaurer…</button></article>`).join('') || '<p class="hint">Aucune sauvegarde. Créez votre premier point de restauration.</p>';
  $('#backup-list').querySelectorAll('[data-restore-backup]').forEach(button => button.onclick = () => action(async () => {
    if (!(await confirmAction("Remplacer les données locales par cette sauvegarde ? Une copie de secours sera créée avant. Les jetons devront être ressaisis. Les fichiers Git, Jira et Tempo restent inchangés.", "Restaurer la sauvegarde ?", "Restaurer la sauvegarde"))) return;
    button.disabled = true;
    try { await api('/api/backups/restore', 'POST', { name: button.dataset.restoreBackup, confirmed: true }); projectsView.invalidate(); boardView.invalidate(); panelSignature = ''; await refresh(); await renderBackups(); toast('Sauvegarde restaurée. Les jetons sont à ressaisir.'); }
    finally { button.disabled = false; }
  }));
}
$('#backups-button').onclick = () => action(async () => { await renderBackups(); $('#backups-dialog').showModal(); });
$('#create-backup').onclick = () => action(async () => { const button = $('#create-backup'); button.disabled = true; try { await api('/api/backups', 'POST', {}); await renderBackups(); toast('Sauvegarde locale créée.'); } finally { button.disabled = false; } });

function renderTemplates() {
  const select = $('#personal-template'), value = select.value;
  select.innerHTML = '<option value="">Sans modèle personnel</option>' + (state.settings.taskTemplates || []).map(t => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join('');
  select.value = value;
  $('#remove-template').hidden = !select.value;
}
$('#personal-template').onchange = () => action(async () => {
  const form = $('#new-form'), template = state.settings.taskTemplates?.find(t => t.id === form.elements.templateId.value);
  $('#remove-template').hidden = !template;
  if (!template) { $('#template-criteria').textContent = ''; return; }
  if ((form.elements.description.value.trim() || form.elements.instructions.value.trim()) && !(await confirmAction('Remplacer la description, les instructions et le mode saisis par ce modèle ?', 'Appliquer le modèle ?', 'Appliquer'))) { form.elements.templateId.value = ''; $('#remove-template').hidden = true; return; }
  for (const key of ['description', 'instructions', 'mode']) form.elements[key].value = template[key];
  $('#template-criteria').textContent = 'Critères repris à la création : ' + (template.checklist.map(c => c.text).join(' · ') || 'aucun');
  form.elements.description.dispatchEvent(new Event('input', { bubbles: true }));
});
$('#remove-template').onclick = () => action(async () => {
  if (!(await confirmAction('Retirer ce modèle ? Les tâches créées restent conservées.', 'Retirer le modèle ?', 'Retirer'))) return;
  await api('/api/templates/' + encodeURIComponent($('#personal-template').value), 'DELETE', {}); await refresh(); $('#template-criteria').textContent = '';
});
$('#template-form').onsubmit = e => { e.preventDefault(); action(async () => {
  const button = e.target.querySelector('.primary'); button.disabled = true;
  try { await api('/api/templates', 'POST', Object.fromEntries(new FormData(e.target))); await refresh(); $('#template-dialog').close(); toast('Modèle enregistré. Disponible dans Nouvelle tâche.'); }
  finally { button.disabled = false; }
}); };

function prMarkup(card, link = true) {
  const pr = card.pullRequest;
  if (!pr) return '<p class="hint">Aucune PR enregistrée.</p>';
  const checkNames = { succeeded: 'réussi', pending: 'en cours', failed: 'échec', error: 'erreur', notApplicable: 'non applicable', notSet: 'non renseigné' };
  return `${link ? `<a class="pr-link" href="${esc(pr.url)}" target="_blank" rel="noopener noreferrer">PR #${pr.number} ↗</a>` : ''}<p class="state-badge">${esc(prLabel(pr))}</p><p class="hint">${pr.checkedAt ? 'Dernier relevé : ' + esc(new Date(pr.checkedAt).toLocaleString('fr-FR')) : 'État distant non relevé.'}</p>${pr.refreshError ? `<p class="inline-error">${esc(pr.refreshError)}</p>` : ''}${pr.checksError ? `<p class="inline-error">Contrôles indisponibles : ${esc(pr.checksError)}</p>` : ''}${pr.checkedAt ? `<p class="hint">${(pr.checks || []).length ? pr.checks.map(c => esc(c.name) + ' : ' + esc(checkNames[c.state] || c.state)).join(' · ') : 'Aucun statut de contrôle remonté.'} Les règles de branche et pipelines restent à vérifier dans le service de dépôt.</p>` : ''}<button type="button" ${connectorSupports(state.connectors, 'repositories', 'refresh', repositoryProvider(state, card)) ? '' : 'hidden'} data-refresh-pr="${esc(card.id)}" ${card.publishing ? 'disabled' : ''}>Actualiser cette PR</button>`;
}
function renderWorkflowDetail(card, active) {
  const dependencies = document.createElement('details'); dependencies.className = 'disclosure'; dependencies.open = Boolean(card.blockedBy?.length);
  dependencies.innerHTML = `<summary>Bloquée par <span class="count">${blockers(card, state.cards).length}</span></summary><label>Tâches préalables (Ctrl pour sélectionner plusieurs tâches)<select id="blocked-by" multiple size="5" ${active ? 'disabled' : ''}>${state.cards.filter(c => c.id !== card.id).map(c => `<option value="${esc(c.id)}" ${card.blockedBy?.includes(c.id) ? 'selected' : ''}>${esc(c.ticket?.key || '')} ${esc(c.title)}${c.status === 'done' ? ' · terminée' : c.archived ? ' · archivée' : ''}</option>`).join('')}</select></label><p class="hint">Le lancement attend que toutes les tâches choisies soient terminées. Une tâche archivée inachevée reste bloquante. Maximum 20 ; les cycles sont refusés.</p>`;
  $('#card-form button[type=submit]').before(dependencies);
  const next = nextAction(card, state.runs, state.cards);
  if (next) { const p = document.createElement('p'); p.className = 'next-action'; p.textContent = next; $('#card-form').before(p); }
  const templateButton = document.createElement('button'); templateButton.type = 'button'; templateButton.textContent = 'Enregistrer comme modèle';
  templateButton.onclick = () => { const form = $('#template-form'); form.elements.cardId.value = card.id; form.elements.name.value = card.title.slice(0, 100); $('#template-dialog').showModal(); form.elements.name.focus(); };
  $('.card-utilities').append(templateButton);
  const queueButton = document.createElement('button'); queueButton.type = 'button'; queueButton.textContent = state.queue?.ids.includes(card.id) ? 'Déjà dans la file' : 'Ajouter à la file'; queueButton.disabled = active || card.status === 'done' || state.queue?.ids.includes(card.id);
  queueButton.onclick = () => action(async () => { await api(cardPath(card.id) + '/queue', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast('Tâche ajoutée. La file se pilote dans Agents.'); });
  $('#launch').after(queueButton);
  if (card.pullRequest) {
    const status = document.createElement('div'); status.innerHTML = prMarkup(card, false);
    if (card.pullRequest.status === 'completed' && !card.pullRequest.refreshError && card.status !== 'done' && !active) {
      const done = document.createElement('button'); done.type = 'button'; done.textContent = 'Terminer la tâche après fusion';
      done.onclick = () => action(async () => {
        if (!(await confirmAction('As-tu vérifié les critères ? Terminer la tâche appliquera aussi la correspondance des statuts si elle est activée.', 'Terminer la tâche ?', 'Terminer'))) return;
        await api(cardPath(card.id), 'PATCH', { status: 'done' }); await refresh(); renderDetail(); await updateRun();
      }); status.append(done);
    }
    $('#prepare-pr').before(status); bindPrActions(status);
  }
}
function bindPrActions(host = document) {
  host.querySelectorAll('[data-refresh-pr]').forEach(button => button.onclick = () => action(async () => {
    button.disabled = true;
    try {
      const card = await api(cardPath(button.dataset.refreshPr) + '/refresh-pr', 'POST', {}); await refresh();
      if ($('#card-dialog').open && selected === card.id) { renderDetail(); await updateRun(); }
      if (card.pullRequest.refreshError) toast(card.pullRequest.refreshError, true);
    } finally { if (button.isConnected) button.disabled = false; }
  }));
}
function cleanupMarkup(cards) {
  const candidates = cards.filter(cleanupCandidate);
  return `<details class="disclosure"><summary>Nettoyer après livraison <span class="count">${candidates.length}</span></summary><p class="hint">Tâches terminées, archivées ou PR fusionnées. Actualise l’état Git avant de choisir. Le retrait conserve les branches et refuse les modifications locales ; les worktrees associés sont également contrôlés.</p>${candidates.map(c => {
    const check = worktreeChecks.find(w => w.cardId === c.id), unavailable = c.publishing || state.runs.some(r => r.cardId === c.id && r.status === 'running') || devRunning(c.id);
    return `<article class="workspace-row"><div><h3>${esc(c.title)}</h3><p class="hint">${esc(check ? check.error || (!check.coherent ? 'Branche inattendue' : check.clean ? 'Dossier principal propre' : 'Modifications locales à conserver') : 'État Git non vérifié')}</p></div><button class="danger" data-clean-candidate="${esc(c.id)}" ${unavailable ? 'disabled' : ''}>Retirer sans forcer</button></article>`;
  }).join('') || '<p class="hint">Aucun dossier candidat au nettoyage.</p>'}</details>`;
}
function queueMarkup() {
  const queue = state.queue || { ids: [], paused: true };
  return `<section class="worktree-box"><h3>File d’exécution globale</h3><p>${queue.paused ? 'En pause' : 'Active'} · ${queue.ids.length} tâche(s) · un seul agent à la fois</p><p class="hint">Tous projets. Les réglages enregistrés de chaque carte sont utilisés à son démarrage. La pause laisse finir la tâche active ; un échec arrête la file.</p>${queue.message ? `<p role="status">${esc(queue.message)}</p>` : ''}<button data-queue-pause="${!queue.paused}" ${!queue.ids.length ? 'disabled' : ''}>${queue.paused ? 'Démarrer / reprendre la file' : 'Mettre la file en pause'}</button>${queue.ids.map((id, index) => { const card = state.cards.find(c => c.id === id); return `<div class="workspace-row"><span>${index + 1}. ${esc(card?.title || 'Tâche introuvable')}${queue.current === id ? ' · en cours' : ''}</span><div class="row-actions"><button data-open-card="${esc(id)}">Ouvrir la tâche</button><button data-dequeue="${esc(id)}" ${queue.current === id ? 'disabled' : ''}>Retirer de la file</button></div></div>`; }).join('')}</section>`;
}
function reportMarkup() {
  return `<details class="disclosure"><summary>Bilan de travail à copier</summary><label>Journée<input id="report-day" type="date" value="${summaryDay}"></label><p class="hint">Projet sélectionné, archives incluses. Les blocages reflètent l’état actuel. Aucun appel IA.</p><label>Bilan<textarea id="work-report" rows="12" readonly>${esc(workReport(scopedCards(true), state.runs, state.times || [], summaryDay, state.cards))}</textarea></label><button id="copy-work-report">Copier le bilan</button></details>`;
}
function bindWorkflowActions() {
  bindPrActions();
  if ($('#refresh-prs')) $('#refresh-prs').onclick = () => action(async () => {
    const button = $('#refresh-prs'); button.disabled = true; let failures = 0;
    try {
      for (const card of scopedCards(true).filter(c => c.pullRequest && !c.publishing)) {
        try { const updated = await api(cardPath(card.id) + '/refresh-pr', 'POST', {}); if (updated.pullRequest.refreshError) failures++; } catch { failures++; }
      }
      await refresh(); toast(failures ? `${failures} PR non actualisée(s) : consulter leur message.` : 'États des PR actualisés.', Boolean(failures));
    } finally { if (button.isConnected) button.disabled = false; }
  });
  document.querySelectorAll('[data-clean-candidate]').forEach(button => button.onclick = () => action(async () => {
    if (!(await confirmAction('Retirer le worktree principal et ses worktrees associés sans forcer ? Les branches sont conservées. Des modifications locales feront refuser le retrait.', 'Nettoyer ce dossier ?', 'Retirer sans forcer'))) return;
    button.disabled = true;
    try { await api(cardPath(button.dataset.cleanCandidate) + '/worktree', 'DELETE', {}); worktreeChecks = []; await refresh(); toast('Worktrees retirés, branches conservées.'); }
    finally { if (button.isConnected) button.disabled = false; }
  }));
  document.querySelectorAll('[data-queue-pause]').forEach(button => button.onclick = () => action(async () => { await api('/api/queue', 'POST', { paused: button.dataset.queuePause === 'true' }); await refresh(); }));
  document.querySelectorAll('[data-dequeue]').forEach(button => button.onclick = () => action(async () => { await api(cardPath(button.dataset.dequeue) + '/queue', 'DELETE', {}); await refresh(); }));
  if ($('#report-day')) $('#report-day').onchange = e => { if (e.target.value) { summaryDay = e.target.value; board(); } };
  if ($('#copy-work-report')) $('#copy-work-report').onclick = () => action(async () => {
    const report = $('#work-report');
    try { await navigator.clipboard.writeText(report.value); toast('Bilan copié.'); }
    catch { report.focus(); report.select(); toast('Copie indisponible : le texte est sélectionné, utilise Ctrl+C.'); }
  });
}
