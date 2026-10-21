import { msg, markupText } from '../i18n.mjs';
import { esc, cardPath } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';

export function createProcessesView({ getState, getSelection, api, refresh, refreshProcesses }) {
  let launchingProjects = false;
function devRunning(cardId) {
  const state = getState();
  return (state.devServers || []).find(session => session.cardId === cardId)?.services.some(service => ['running', 'stopping'].includes(service.status)) || false;
}
function processCards(project) {
  const state = getState();
  return state.cards.filter(card => {
    const session = (state.devServers || []).find(session => session.cardId === card.id);
    const owner = state.projects.find(item => sameRepository(item.repo, card.repo, state.settings?.platform));
    return (card.worktree || session) && (!project || owner?.id === project.id || owner?.linkedProjectId === project.id || session?.projectIds.includes(project.id));
  }).sort((a, b) => Number(devRunning(b.id)) - Number(devRunning(a.id)) || a.title.localeCompare(b.title));
}
async function launchProjects(projectIds) {
  const state = getState(), launchWorktrees = getSelection();
  if (launchingProjects) return;
  const failures = [];
  launchingProjects = true;
  try {
    for (const id of projectIds) {
      const cardId = launchWorktrees[id];
      if (!cardId || projectLaunchBlocked(id, cardId)) continue;
      try { await api(cardPath(cardId) + '/dev-servers', 'POST', { standalone: true }); }
      catch (error) { failures.push(`${state.projects.find(project => project.id === id)?.name || id} : ${error.message}`); }
    }
    await refresh(); await refreshProcesses();
    if (failures.length) throw new Error(failures.join('\n'));
  } finally { launchingProjects = false; }
}
function projectManagedPorts(project) {
  const commands = [project?.frontCommand, project?.backCommand].filter(Boolean);
  return commands.length > 0 && commands.every(command => command.includes('{port}'));
}
function projectLaunchBlocked(projectId, cardId) {
  const state = getState();
  return devRunning(cardId) || (state.devServers || []).some(session => session.projectIds.includes(projectId) && devRunning(session.cardId) && (!session.isolatedPorts || !projectManagedPorts(state.projects.find(project => project.id === projectId))));
}
function processesMarkup(project) {
  const state = getState(), launchWorktrees = getSelection();
  const cards = processCards(project), sessions = state.devServers || [];
  const count = sessions.filter(session => cards.some(card => card.id === session.cardId)).reduce((sum, session) => sum + session.services.filter(service => service.status === 'running').length, 0);
  let html = `<div class="panel-heading"><div><h2>${markupText("Lancer les projets")}</h2><p class="hint">${markupText("Choisissez un worktree par projet. Ses commandes Front et Back démarrent ensemble, dans ce worktree. Pour deux dépôts, sélectionnez une ligne pour chacun.")}</p></div><div class="row-actions"><button id="start-selected-projects" class="primary">${markupText("Lancer les sélections")}</button><button id="refresh-processes">${markupText("Actualiser")}</button><button id="stop-visible-processes" class="danger" ${cards.some(card => devRunning(card.id)) ? '' : 'disabled'}>${markupText("Tout arrêter dans cette vue")}</button></div></div>`;
  html += state.projects.filter(item => !project || item.id === project.id).map(item => {
    const choices = state.cards.filter(card => card.worktree && !card.archived && sameRepository(card.repo, item.repo, state.settings?.platform));
    const session = sessions.find(session => session.projectIds.includes(item.id) && devRunning(session.cardId));
    const configured = item.frontCommand || item.backCommand;
    if (!launchWorktrees[item.id] && session && choices.some(card => card.id === session.cardId)) launchWorktrees[item.id] = session.cardId;
    if (!choices.some(card => card.id === launchWorktrees[item.id])) delete launchWorktrees[item.id];
    return `<article class="workspace-row"><div><h3>${esc(item.name)}</h3><p class="hint">${configured ? [item.frontCommand ? msg("Front") : '', item.backCommand ? msg("Back") : ''].filter(Boolean).join(' + ') : msg("Renseignez les commandes dans les réglages.")}${session ? msg(" · En cours") : ''}</p><label>${markupText("Worktree")}<select data-launch-project="${esc(item.id)}" ${session && (!session.isolatedPorts || !projectManagedPorts(item)) || !configured ? 'disabled' : ''}><option value="">${choices.length ? msg("Ne pas lancer") : msg("Préparez un worktree depuis une carte")}</option>${choices.map(card => `<option value="${esc(card.id)}" ${launchWorktrees[item.id] === card.id ? 'selected' : ''}>${esc(card.branch || card.title)} · ${esc(card.title)}</option>`).join('')}</select></label>${launchWorktrees[item.id] ? `<p class="path-text">${esc(choices.find(card => card.id === launchWorktrees[item.id]).worktree.path)}</p>` : ''}</div><div class="row-actions"><button data-launch-one="${esc(item.id)}" ${projectLaunchBlocked(item.id, launchWorktrees[item.id]) || !configured || !launchWorktrees[item.id] ? 'disabled' : ''}>${markupText("Lancer")}</button><button data-launch-settings="${esc(item.id)}">${markupText("Configurer le lancement")}</button></div></article>`;
  }).join('');
  html += `<h3>${markupText("Services et journaux · {0} en cours", count)}</h3>`;
  html += cards.filter(card => sessions.some(session => session.cardId === card.id)).map(card => {
    const session = sessions.find(item => item.cardId === card.id), running = devRunning(card.id);
    const owner = state.projects.find(item => sameRepository(item.repo, card.repo, state.settings?.platform));
    const ready = card.worktree && !card.archived && (owner?.frontCommand || owner?.backCommand);
    const serviceRows = (session?.services || []).map(service => {
      const live = service.status === 'running', stopping = service.status === 'stopping';
      const label = live ? msg("En cours") : stopping ? msg("Arrêt en cours") : msg("Arrêté{0}", service.exitCode != null ? msg(" · code {0}", service.exitCode) : '');
      const action = (operation, text, disabled) => `<button data-process-action="${operation}" data-process-card="${esc(card.id)}" data-service-id="${esc(service.id)}" ${operation === 'stop' ? 'class="danger"' : ''} ${disabled ? 'disabled' : ''}>${text}</button>`;
      return `<section class="process-service"><div class="panel-heading"><div><h4>${esc(service.name)}</h4><span class="state-badge" data-status="${esc(service.status)}">${esc(label)}</span></div><div class="row-actions">${action('start', msg("Démarrer"), live || stopping || !ready)}${action('restart', msg("Redémarrer"), !live || !ready)}${action('stop', msg("Arrêter"), !live)}</div></div><p class="path-text">${esc(service.repo)}</p><p class="hint">${markupText("Port attribué par Atelier :")} <code>${esc(service.port || '—')}</code> ${markupText("· PID lanceur :")} <code>${esc(service.pid || '—')}</code>${service.processIds?.length > 1 ? msg(" · Processus enfants : {0}", service.processIds.filter(pid => pid !== service.pid).map(pid => esc(pid)).join(', ')) : ''}</p>${service.inspectionError && live ? `<p class="inline-error">${esc(service.inspectionError)}</p>` : ''}${service.endpoints?.length ? `<div class="process-ports"><table><caption class="hint">${markupText("Adresses TCP en écoute")}</caption><thead><tr><th>${markupText("Adresse IP")}</th><th>${markupText("Port")}</th><th>PID</th><th>${markupText("Application")}</th></tr></thead><tbody>${service.endpoints.map(endpoint => `<tr><td><code>${esc(endpoint.address)}</code>${['0.0.0.0', '::'].includes(endpoint.address) ? ` <small>${markupText("(toutes interfaces)")}</small>` : ''}</td><td>${esc(endpoint.port)}</td><td>${esc(endpoint.pid)}</td><td><a href="${esc(endpoint.url)}" target="_blank" rel="noopener noreferrer">${markupText("Ouvrir {0} ↗", endpoint.url.startsWith('https:') ? 'HTTPS' : 'HTTP')}</a></td></tr>`).join('')}</tbody></table></div>` : `<p class="hint">${live ? msg("Aucun port TCP en écoute détecté pour l’instant.") : msg("Aucun port en écoute.")}</p>`}</section>`;
    }).join('');
    return `<article class="process-stack"><div class="panel-heading"><div><span class="eyebrow">${esc(owner?.name || msg("Projet retiré"))}</span><h3>${esc(card.title)}</h3><p class="hint">${esc(card.branch || msg("Worktree de la carte"))}${card.archived ? msg(" · Carte archivée") : ''}</p></div><div class="row-actions"><button data-open-card="${esc(card.id)}">${markupText("Voir la carte")}</button><button data-process-action="start" data-process-card="${esc(card.id)}" ${running || !ready ? 'disabled' : ''}>${markupText("Démarrer l’ensemble")}</button><button class="danger" data-process-action="stop" data-process-card="${esc(card.id)}" ${running ? '' : 'disabled'}>${markupText("Arrêter l’ensemble")}</button></div></div><div class="process-services">${serviceRows || `<p class="hint">${markupText("Aucun lancement pendant cette session. Les commandes du projet et de son éventuel projet lié sont définies dans Mes projets → Réglages.")}</p>`}</div>${!card.worktree ? `<p class="hint">${markupText("Worktree absent : préparez-le depuis la carte avant de relancer.")}</p>` : ''}${session ? `<details class="process-journal" data-process-journal="${esc(card.id)}"><summary>${markupText("Journal de l’ensemble")}</summary><pre class="log" data-process-log="${esc(card.id)}">${esc(session.log)}</pre></details>` : ''}</article>`;
  }).join('') || `<p class="empty">${markupText("Aucun service lancé. Préparez le worktree d’une carte puis sélectionnez-le ci-dessus.")}</p>`;
  return html;
}

  return { devRunning, processCards, launchProjects, projectManagedPorts, projectLaunchBlocked, processesMarkup };
}

export function bindProcessActions({ document, api, action, refresh, refreshProcesses, processCards, devRunning }, project) {
  const $ = selector => document.querySelector(selector);
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
}
