import { removeWorktree } from './workspace-workflows.mjs';
import { messageText, msg, markupText, locale } from '../i18n.mjs';
import { names, runNames, esc, cardPath, duration, agentState } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { connectorSupports, repositoryProvider } from '../connector-availability.mjs';
import { agentTime } from '../ui-helpers.mjs';
import { effortLevels, agentName, agentSelect } from '../agent-options.mjs';
import { usageText } from '../agent-usage.mjs';
import { blockers, nextAction, prLabel } from '../workflow.mjs';

export function createCardView({ document, getState, api, action, confirmAction, refresh, toast, connectorName, ticketSupports, taskAgentFields, attachDraft, clearDraft, openTime, devRunning, updateClocks, renderBrainstorm }) {
  const $ = selector => document.querySelector(selector);
  const { renderTicketPreparation } = createTicketPreparation({ document, getState, action, toast, addCriterion });
  const automation = createAutomationView({ document, getState, api, action, refresh,
    onChanged: async () => { renderDetail(); await updateRun(); },
    onRunSelected: async id => { selectedRun = id; renderDetail(); $('#run-select').value = id; await updateRun(); },
  });
  function updateAutomation() { automation.updateAutomation(selected); }
  let selected = null, selectedRun = null, cursor = 0, logRun = null, wasRunning = false, prPreview = null, prCardId = null;
async function openCard(id) {
  selected = id; selectedRun = null; renderDetail(); if (!$('#card-dialog').open) $('#card-dialog').showModal(); await updateRun();
}
function renderDetail() {
  const card = getState().cards.find(c => c.id === selected); if (!card) { $('#card-dialog').close(); return; }
  const effectiveSettings = projectConfig(card.repo);
  const runs = getState().runs.filter(r => r.cardId === card.id);
  const busyCard = card.publishing || runs.find(r => r.status === 'running');
  const active = card.archived || busyCard;
  wasRunning = Boolean(active);
  $('#card-detail').innerHTML = `<div class="dialog-heading"><div><div class="eyebrow">${esc(card.ticket?.key || msg("TÂCHE LOCALE"))}</div><h2>${esc(card.title)}</h2></div><button id="close-card" aria-label="${markupText("Fermer")}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><path d="m5 5 14 14M19 5 5 19"/></svg></button></div>
    ${card.ticket ? `<div class="detail-topline"><a href="${esc(card.ticket.url)}" target="_blank" rel="noopener noreferrer">${markupText("Ouvrir dans {0} ↗", esc(connectorName('tickets', card.ticket.connector || 'jira')))}</a> · ${esc(card.ticket.status)}</div>` : ''}
    ${card.ticket && (!card.ticket.connector || card.ticket.connector === 'jira') ? `<p class="hint">${effectiveSettings.jiraAutoTransition && effectiveSettings.jiraStatusMap?.[card.status] ? msg("Jira automatique pour cette colonne → {0}", esc(effectiveSettings.jiraStatusMap[card.status])) : msg("Cette colonne reste locale : aucune transition Jira automatique configurée.")}</p>${card.ticketSync?.state === 'error' ? `<p class="inline-error">${markupText("Avancement local enregistré, Jira à vérifier : {0}", esc(messageText(card.ticketSync.message)))}</p><button type="button" id="retry-jira-status" ${active ? 'disabled' : ''}>${markupText("Réessayer la mise à jour Jira")}</button>` : ''}` : ''}
    ${card.archived ? `<p class="archive-notice">${markupText("Tâche archivée · restaurez-la pour reprendre le travail.")}</p>` : ''}
    <form id="card-form">${card.ticket ? `<div class="detail-description">${esc(card.description || msg("Aucune description."))}</div>` : `<label>${markupText("Titre")}<input name="title" required maxlength="300" value="${esc(card.title)}"></label><label>${markupText("Description")}<textarea name="description" rows="3">${esc(card.description)}</textarea></label>`}
    <details class="disclosure" ${(card.checklist || []).length ? 'open' : ''}><summary>${markupText("Critères de réussite")} <span class="count">${(card.checklist || []).filter(i => i.done).length}/${(card.checklist || []).length}</span></summary><p class="hint">${markupText("Cochez ce que vous avez vérifié. Ces critères accompagnent les instructions de l’agent et le brouillon de PR.")}</p><p class="hint">${markupText("{0} critère(s) restent à vérifier. La validation est manuelle.", (card.checklist || []).filter(i => !i.done).length)}</p><div id="criteria-list"></div><button type="button" id="add-criterion" ${active ? 'disabled' : ''}>${markupText("＋ Ajouter un critère")}</button></details><div class="form-row"><label>${markupText("Avancement local")}<select name="status" aria-describedby="status-help">${Object.entries(names).map(([key, value]) => `<option value="${key}" ${card.status === key ? 'selected' : ''}>${value}</option>`).join('')}</select><small id="status-help" class="hint">${markupText("Changement enregistré immédiatement avec la carte. « En cours » lance l’agent.")}</small></label><label>${markupText("Agent")}<select name="agent">${agentSelect(getState().settings, card.agent)}</select></label></div><details class="disclosure"><summary>${markupText("Dépôt et branche de travail")}</summary><label>${markupText("Dépôt Git source")}<input list="project-paths" name="repo" value="${esc(card.repo)}" placeholder="${markupText("Chemin absolu du dépôt Git")}" ${card.worktree ? 'readonly' : ''}></label><div class="form-row"><label>${markupText("Branche de la tâche")}<input name="branch" value="${esc(card.branch || card.suggestedBranch)}" ${card.worktree ? 'readonly' : ''}></label><label>${markupText("Partir de")}<input name="baseBranch" value="${esc(card.baseBranch || '')}" placeholder="${esc(effectiveSettings.baseBranch || 'HEAD')}" ${card.worktree ? 'readonly' : ''}></label></div></details><button type="submit" ${active ? 'disabled' : ''}>${markupText("Enregistrer la carte")}</button></form>
    <section class="worktree-box"><h3>${markupText("Worktree dédié")}</h3><code id="worktree-path">${esc(card.worktree?.path || msg("Sera créé au lancement ou au passage en En cours."))}</code><p id="service-worktree-paths" class="hint path-text"></p><p class="hint">${markupText("En cours lance automatiquement {0} dans cette branche. Le worktree est réutilisé aux relances.", esc(agentName(getState().settings, card.agent)))}</p><button id="prepare-worktree" ${active || card.worktree ? 'disabled' : ''}>${markupText("Préparer le worktree")}</button> <button id="remove-worktree" class="danger" ${active || !card.worktree ? 'disabled' : ''}>${markupText("Retirer le worktree")}</button></section><section class="run-box"><label>${markupText("Instructions complémentaires")}<textarea id="instructions" name="instructions" form="card-form" rows="2" placeholder="${markupText("Par exemple : commence par vérifier les tests existants.")}">${esc(card.instructions || '')}</textarea></label><div class="run-controls"><select id="mode" name="mode" form="card-form" aria-label="${markupText("Mode de travail")}"><option value="analyze" ${card.mode === 'analyze' ? 'selected' : ''}>${markupText("Analyser")}</option><option value="edit" ${card.mode !== 'analyze' ? 'selected' : ''}>${markupText("Modifier le code")}</option></select><button id="launch" class="primary" ${active ? 'disabled' : ''}>${markupText("Lancer l’agent →")}</button></div><p class="hint">${markupText("Le lancement enregistre la carte. Les modifications se font dans le worktree dédié. {0} Claude peut refuser les commandes qui exigent une permission interactive.", getState().projects.find(p => sameRepository(p.repo, card.repo, getState().settings.platform))?.automationEnabled ? msg("Le mode automatique publiera une PR brouillon après les vérifications.") : msg("Aucune publication n’est demandée."))}</p></section>
    <section class="worktree-box"><h3>${markupText("Pull request")}</h3>
    ${card.pullRequest ? `<p><a class="pr-link" href="${esc(card.pullRequest.url)}" target="_blank" rel="noopener noreferrer">${markupText("PR #{0} · {1} ↗", card.pullRequest.number, esc(card.pullRequest.title))}</a></p><p class="hint">${card.ticket ? card.pullRequest.ticketLinked ? msg("Liée au ticket Jira.") : esc(card.pullRequest.ticketError || msg("Lien Jira en attente.")) : msg("Carte locale.")}</p>` : `<p class="hint">${markupText("Publiez les changements de ce worktree et reliez la PR à votre ticket.")}</p>`}
    ${card.pullRequest ? `<button id="review-pr" ${active || !card.worktree ? 'disabled' : ''}>${markupText("Review avec {0}", esc(agentName(getState().settings, card.agent)))}</button><p class="hint">${markupText("Analyse du commit publié, worktree propre requis. Résultat local, sans commentaire publié dans le service de dépôt.")}</p><div id="pr-review-result"></div>` : ''}
    <button id="prepare-pr" ${active || !card.worktree ? 'disabled' : ''}>${card.pullRequest ? msg("Publier les nouveaux changements") : msg("Préparer la PR")}</button>
    ${card.ticket && card.pullRequest && ticketSupports(card, 'linkPullRequest') ? `<button id="link-pr" ${active ? 'disabled' : ''}>${card.pullRequest.ticketLinked ? msg("Actualiser le lien Jira") : msg("Réessayer le lien Jira")}</button>` : ''}</section>
    <section class="worktree-box"><h3>${markupText("Temps passé")}</h3><p>${markupText("Agents :")} <strong data-agent-time="${esc(card.id)}">${duration(agentTime(runs)[0]?.seconds || 0)}</strong></p><p class="hint">${markupText("Cumul automatique, toutes dates. Les exécutions interrompues sont estimées au dernier signal reçu. Le temps humain se saisit séparément.")}</p><button id="card-time" ${card.archived ? 'disabled' : ''}>${markupText("Saisir du temps / chronométrer")}</button></section>
    <section id="agent-summary" class="agent-summary" hidden><h3>${markupText("Compte rendu de l’agent")}</h3><p class="hint" id="summary-context"></p><pre id="summary-text"></pre><p class="hint">${markupText("À relire et à vérifier avant de valider la tâche.")}</p></section>
    <details class="disclosure" id="run-details" ${active ? 'open' : ''}><summary>${markupText("Exécutions et journal")} <span class="count">${runs.length}</span></summary><select id="run-select" aria-label="${markupText("Historique des exécutions")}">${runs.length ? runs.map(r => `<option value="${r.id}">${esc(agentName(getState().settings, r.agent, r))} · ${new Date(r.startedAt).toLocaleString(locale)} · ${runNames[r.status]}</option>`).join('') : `<option value="">${markupText("Aucune exécution")}</option>`}</select><div class="detail-actions"><span id="run-status" class="run-status"></span><button id="stop-run" class="danger" hidden>${markupText("Arrêter l’agent")}</button></div>
    <p id="run-usage" class="hint" aria-live="polite"></p><p class="hint">${markupText("Coûts API indicatifs en USD ; la facturation de votre abonnement peut différer.")}</p>
    <section id="live-progress" class="live-progress" hidden><strong id="current-activity"></strong><div class="progress-times"><span id="run-clock"></span><span id="last-event"></span></div><pre id="live-text" hidden></pre></section>
    <label class="check"><input id="follow-log" type="checkbox" checked> ${markupText("Suivre les nouveaux messages")}</label><pre class="log" id="log" aria-label="${markupText("Journal de l’agent")}">${markupText("Les résultats apparaîtront ici.")}</pre></details>
    ${card.ticket && ticketSupports(card, 'transitions') && ticketSupports(card, 'transition') ? `<div class="jira-actions"><button id="load-transitions">${markupText("Changer le statut distant…")}</button><select id="transitions" aria-label="${markupText("Nouveau statut distant")}" hidden></select><button id="apply-transition" hidden>${markupText("Appliquer à Jira")}</button></div>` : ''}
    <div class="card-utilities"><button id="duplicate-card">${markupText("Dupliquer comme tâche locale")}</button><button id="archive-card" ${busyCard ? 'disabled' : ''}>${card.archived ? msg("Restaurer la tâche") : msg("Archiver la tâche")}</button></div><details class="disclosure"><summary>${markupText("Retirer définitivement du tableau…")}</summary><div class="detail-actions"><button id="delete-card" class="danger" ${active ? 'disabled' : ''}>${markupText("Retirer du tableau")}</button><span class="hint">${card.ticket ? msg("Le ticket Jira sera conservé.") : msg("La carte est sauvegardée sur votre PC.")}</span></div></details>`;
  const automationStatus = document.createElement('section'); automationStatus.id = 'automation-status'; automationStatus.className = 'worktree-box';
  $('#card-form').before(automationStatus); updateAutomation();
  compactCard();
  renderWorkflowDetail(card, Boolean(active));
  const fields = document.createElement('div'); fields.dataset.agentFields = '';
  $('#card-form').elements.agent.closest('.form-row').after(fields);
  taskAgentFields($('#card-form'), card);
  (card.checklist || []).forEach(item => addCriterion(item, Boolean(active)));
  const preparation = document.createElement('section'); preparation.className = 'worktree-box';
  preparation.innerHTML = `<h3>${markupText("Préparer le ticket")}</h3><p class="hint">${markupText("L’agent propose des critères et des questions à partir de la carte enregistrée, en lecture seule. Le lancement enregistre vos saisies et conserve la colonne actuelle. Consomme les quotas de votre agent.")}</p><button type="button" id="prepare-ticket" ${active ? 'disabled' : ''}>${markupText("Proposer des critères avec l’agent")}</button><div id="ticket-proposal" aria-live="polite"></div>`;
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
      toast(msg("Préparation lancée. La proposition apparaîtra dans la fiche."));
    } finally { if ($('#prepare-ticket')) $('#prepare-ticket').disabled = getState().cards.find(c => c.id === card.id)?.archived || getState().runs.some(r => r.cardId === card.id && r.status === 'running'); }
  });
  $('#add-criterion').onclick = () => { if ($('#criteria-list').children.length >= 20) return toast(msg("Maximum 20 critères."), true); addCriterion({ text: '', done: false }, false); $('#criteria-list').lastElementChild.querySelector('input[type=text]').focus(); };
  if (card.archived) { [...$('#card-form').elements].forEach(field => field.disabled = true); if ($('#load-transitions')) $('#load-transitions').disabled = true; }
  $('#duplicate-card').onclick = () => action(async () => { const copy = await api(cardPath(card.id) + '/duplicate', 'POST', {}); await refresh(); await openCard(copy.id); toast(msg("Copie locale créée, sans branche ni liaison Jira.")); });
  if ($('#archive-card')) $('#archive-card').onclick = () => action(async () => { await api(cardPath(card.id) + '/archive', 'POST', { archived: !card.archived }); await refresh(); $('#card-dialog').close(); toast(card.archived ? msg("Tâche restaurée.") : msg("Tâche archivée. Retrouvez-la dans Archives.")); });
  if ($('#retry-jira-status')) $('#retry-jira-status').onclick = () => action(async () => { const result = await api(cardPath(card.id) + '/sync-status', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(result.ticketSync?.state === 'error' ? msg("Jira à vérifier : ") + result.ticketSync.message : msg("Statut Jira mis à jour."), result.ticketSync?.state === 'error'); });
  $('#card-time').onclick = () => openTime(card.id);
  if (!active) attachDraft($('#card-form'), card.id, $('#card-detail'));
  if (active) { $('#card-form').elements.model.disabled = true; $('#card-form').elements.effort.disabled = true; $('#card-form').elements.agent.disabled = true; }
  $('#close-card').onclick = () => $('#card-dialog').close();
  async function saveCard(launchSeparately = false) { const body = Object.fromEntries(new FormData($('#card-form'))); body.blockedBy = [...$('#blocked-by').selectedOptions].map(option => option.value); body.checklist = [...$('#criteria-list').children].map(row => ({ text: row.querySelector('input[type=text]').value, done: row.querySelector('input[type=checkbox]').checked })); if (launchSeparately) body.status = getState().cards.find(c => c.id === card.id).status; const saved = await api(cardPath(card.id), 'PATCH', body); clearDraft(card.id); return saved; }
  let savingCard = false;
  const cardForm = $('#card-form'), statusSelect = cardForm.elements.status;
  async function persistCard(statusChanged = false) {
    if (savingCard) return;
    if (!cardForm.reportValidity()) { statusSelect.value = getState().cards.find(c => c.id === card.id)?.status || card.status; return; }
    savingCard = true;
    try {
      const pending = saveCard();
      statusSelect.disabled = true; cardForm.querySelector('button[type=submit]').disabled = true;
      const saved = await pending; await refresh();
      if (selected === card.id && $('#card-dialog').open) { renderDetail(); await updateRun(); }
      toast(saved.ticketSync?.state === 'error' ? msg("Avancement local enregistré. Jira à vérifier : {0}", saved.ticketSync.message) : statusChanged ? msg("Avancement enregistré : {0}.{1}", names[saved.status], saved.ticketSync?.state === 'synced' && saved.ticketSync.localStatus === saved.status ? msg(" Jira : ") + saved.ticket.status + '.' : '') : msg("Carte enregistrée."), saved.ticketSync?.state === 'error');
    } catch (error) {
      statusSelect.value = getState().cards.find(c => c.id === card.id)?.status || card.status;
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
    try { await saveCard(true); const run = await api(cardPath(card.id) + '/run', 'POST', { agent: $('#card-form').elements.agent.value, mode: $('#mode').value, instructions: $('#instructions').value }); await refresh(); renderDetail(); selectedRun = run.automation?.runId || run.id; await updateRun(); } finally { if ($('#launch')) $('#launch').disabled = getState().cards.find(c => c.id === card.id)?.publishing || getState().runs.some(r => r.cardId === card.id && r.status === 'running'); }
  });
  $('#prepare-worktree').onclick = () => action(async () => { await saveCard(true); await api(cardPath(card.id) + '/worktree', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(msg("Worktree prêt.")); });
  const devBox = document.createElement('details');
  devBox.className = 'disclosure'; devBox.open = devRunning(card.id);
  devBox.innerHTML = `<summary>${markupText("Lancer le projet")}</summary><p class="hint">${markupText("Commandes définies dans Mes projets → Réglages. Le projet de la carte tourne dans son worktree ; son éventuel projet lié tourne depuis son dépôt local, avec sa branche et ses fichiers actuels. Démarrage et arrêt communs. Les commandes avec {port} permettent à Atelier de fournir des ports distincts.")}</p><p id="dev-context" class="hint path-text"></p><button id="start-dev">${markupText("Démarrer Front / Back")}</button> <button id="stop-dev">${markupText("Arrêter Front / Back")}</button><p id="dev-status" class="hint" role="status"></p><pre id="dev-log" class="log" aria-label="${markupText("Journal Front / Back")}"></pre>`;
  $('#remove-worktree').closest('details').after(devBox);
  $('#start-dev').onclick = () => action(async () => { await api(cardPath(card.id) + '/dev-servers', 'POST', {}); await refresh(); updateDevServers(); });
  $('#stop-dev').onclick = () => action(async () => { await api(cardPath(card.id) + '/dev-servers', 'DELETE', {}); await refresh(); updateDevServers(); });
  updateDevServers();
  $('#remove-worktree').onclick = () => action(async () => { if (!await removeWorktree(card.id, { api, confirmAction })) return; await refresh(); renderDetail(); await updateRun(); toast(msg("Worktrees retirés, branches conservées.")); });
  renderReviewReport(card.id);
  if ($('#review-pr')) $('#review-pr').onclick = () => action(async () => { $('#review-pr').disabled = true; try { const run = await api(cardPath(card.id) + '/review', 'POST', {}); await refresh(); renderDetail(); selectedRun = run.id; $('#run-select').value = run.id; await updateRun(); toast(msg("Review démarrée. Les constats apparaîtront ici.")); } finally { if ($('#review-pr')) $('#review-pr').disabled = getState().cards.find(c => c.id === card.id)?.publishing || getState().runs.some(r => r.cardId === card.id && r.status === 'running'); } });
  $('#prepare-pr').onclick = () => action(async () => {
    $('#prepare-pr').disabled = true;
    try {
      const preview = await api(cardPath(card.id) + '/pr-preview');
      prPreview = preview; prCardId = card.id;
      const form = $('#pr-form'); form.reset();
      for (const key of ['title', 'base', 'body']) form.elements[key].value = preview[key];
      $('#pr-destination').textContent = preview.repository + ' · ' + preview.branch;
      $('#pr-files').textContent = preview.status || msg("Aucun changement non commité.");
      $('#pr-commits').textContent = preview.commits || msg("Aucun nouveau commit depuis le point de départ.");
      $('#pr-diff').textContent = preview.diff || msg("Aucun diff local.");
      $('#pr-commit-label').hidden = !preview.dirty;
      form.elements.commitChanges.required = preview.dirty;
      $('#pr-dialog').showModal();
    } finally { if ($('#prepare-pr')) $('#prepare-pr').disabled = false; }
  });
  if ($('#link-pr')) $('#link-pr').onclick = () => action(async () => {
    $('#link-pr').disabled = true;
    try { const updated = await api(cardPath(card.id) + '/link-pr', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(updated.pullRequest.ticketLinked ? msg("PR liée au ticket.") : updated.pullRequest.ticketError, !updated.pullRequest.ticketLinked); }
    finally { if ($('#link-pr')) $('#link-pr').disabled = false; }
  });
  $('#delete-card').onclick = () => action(async () => { if (!(await confirmAction(msg("Retirer cette carte du tableau ?"), msg("Supprimer la carte ?"), msg("Supprimer localement")))) return; await api(cardPath(card.id), 'DELETE', {}); $('#card-dialog').close(); await refresh(); });
  $('#run-select').onchange = () => action(async () => { selectedRun = $('#run-select').value; await updateRun(); });
  $('#stop-run').onclick = () => action(async () => { $('#stop-run').disabled = true; try { await api(`/api/runs/${selectedRun}/cancel`, 'POST', {}); toast(msg("Arrêt demandé…")); } finally { $('#stop-run').disabled = false; } });
  if (card.ticket && $('#load-transitions')) {
    $('#load-transitions').onclick = () => action(async () => { const result = await api(cardPath(card.id) + '/transitions'); $('#transitions').innerHTML = result.transitions.map(t => `<option value="${esc(t.id)}">${esc(t.name)}</option>`).join(''); $('#transitions').hidden = false; $('#apply-transition').hidden = !result.transitions.length; if (!result.transitions.length) toast(msg("Aucune transition disponible.")); });
    $('#apply-transition').onclick = () => action(async () => { await api(cardPath(card.id) + '/transition', 'POST', { transitionId: $('#transitions').value }); await refresh(); renderDetail(); toast(msg("Statut Jira mis à jour.")); });
  }
  cursor = 0; logRun = null;
}
async function updateRun() {
  if (!$('#card-dialog').open) return;
  updateAutomation();
  const select = $('#run-select');
  for (const run of getState().runs.filter(r => r.cardId === selected).reverse()) {
    if (![...select.options].some(o => o.value === run.id)) select.prepend(new Option('', run.id));
  }
  if (select.options.length > 1) select.querySelector('option[value=""]')?.remove();
  selectedRun ||= $('#run-select').value;
  if (!selectedRun) return;
  const run = getState().runs.find(r => r.id === selectedRun); if (!run) return;
  const active = getState().cards.find(c => c.id === selected)?.archived || getState().cards.find(c => c.id === selected)?.publishing || getState().runs.some(r => r.cardId === selected && r.status === 'running');
  if (wasRunning && !active) $('#card-form').elements.status.value = getState().cards.find(c => c.id === selected)?.status || 'todo';
  wasRunning = active;
  for (const option of $('#run-select').options) {
    const current = getState().runs.find(r => r.id === option.value);
    if (current) option.textContent = `${agentName(getState().settings, current.agent, current)} · ${new Date(current.startedAt).toLocaleString(locale)} · ${runNames[current.status]}${current.kind === 'pr-review' ? msg(" · Review PR") : current.kind === 'ticket-preparation' ? msg(" · Préparation du ticket") : current.kind === 'brainstorm' ? msg(" · Brainstorming") : ''}`;
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
  $('#summary-context').textContent = `${agentName(getState().settings, run.agent, run)} · ${run.actualModel || run.model || msg("Modèle CLI")} · ${effortLevels[run.agentType || run.agent]?.[run.effort] || msg("Réflexion CLI")} · ${runNames[run.status]} · ${new Date(run.startedAt).toLocaleString(locale)}`;
  $('#run-status').textContent = `${runNames[run.status]} · ${run.mode === 'edit' ? msg("Modification") : msg("Analyse")}`;
  $('#run-usage').textContent = usageText(run);
  $('#stop-run').hidden = run.status !== 'running';
  const currentCard = getState().cards.find(c => c.id === selected);
  $('#worktree-path').textContent = currentCard?.worktree?.path || msg("Sera créé au lancement.");
  $('#prepare-worktree').disabled = active || Boolean(currentCard?.worktree);
  $('#remove-worktree').disabled = active || !currentCard?.worktree || devRunning(selected);
  $('#prepare-pr').disabled = active || !currentCard?.worktree;
  if ($('#review-pr')) $('#review-pr').disabled = active || !currentCard?.worktree;
  renderReviewReport(selected);
  for (const field of ['repo', 'branch', 'baseBranch']) $('#card-form').elements[field].readOnly = active || Boolean(currentCard?.worktree);
  $('#live-progress').hidden = false;
  $('#current-activity').textContent = `${agentState(run)} · ${messageText(run.attention || run.activity || '')}`;
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
function compactCard() {
  const card = getState().cards.find(c => c.id === selected);
  const context = document.createElement('h3'); context.textContent = msg("La tâche");
  $('#card-form').prepend(context);
  const criteria = $('#criteria-list').closest('details');
  criteria.open = false;
  const statusRow = $('#card-form').elements.status.closest('.form-row');
  criteria.before(statusRow);
  const instructions = $('#instructions').closest('label');
  const options = document.createElement('details'); options.className = 'disclosure agent-options';
  options.open = Boolean(card?.instructions);
  const optionsTitle = document.createElement('summary'); optionsTitle.textContent = msg("Instructions complémentaires");
  instructions.before(options); options.append(optionsTitle, instructions);
  const detail = $('#card-detail'), form = $('#card-form'), runBox = detail.querySelector('.run-box');
  form.after(runBox);
  const heading = document.createElement('h3'); heading.textContent = msg("Travailler avec l’agent"); runBox.prepend(heading);
  detail.querySelectorAll('.worktree-box').forEach(section => {
    if (section.id === 'automation-status') return;
    const wrapper = document.createElement('details'); wrapper.className = 'disclosure';
    const title = section.querySelector('h3'), summary = document.createElement('summary'); summary.textContent = title.textContent === msg("Worktree dédié") ? msg("Dossier de travail · worktree") : title.textContent;
    section.before(wrapper); title.remove(); wrapper.append(summary, section);
    if (section.querySelector('#prepare-pr') && getState().cards.find(c => c.id === selected)?.pullRequest) wrapper.open = true;
  });
  const journal = $('#run-details'), report = $('#agent-summary'); runBox.after(report); report.after(journal);
  const headingBar = document.createElement('nav'); headingBar.className = 'detail-nav'; headingBar.setAttribute('aria-label', msg("Sections de la tâche"));
  for (const [label, target] of [[msg("Contexte"), form], [msg("Agent"), runBox], [msg("Résultat"), report], [msg("PR & review"), $('#prepare-pr').closest('details')]]) {
    if (label === msg("PR & review") && !connectorSupports(getState().connectors, 'repositories', 'preview', repositoryProvider(getState(), card))) { target.hidden = true; continue; }
    const button = document.createElement('button'); button.type = 'button'; button.textContent = label;
    button.onclick = () => { if (target.hidden) { journal.open = true; journal.scrollIntoView({ block: 'start', behavior: 'instant' }); return; } if (target.tagName === 'DETAILS') target.open = true; target.setAttribute('tabindex', '-1'); target.focus({ preventScroll: true }); target.scrollIntoView({ block: 'start', behavior: 'instant' }); }; headingBar.append(button);
  }
  detail.querySelector('.dialog-heading').after(headingBar);
  const editor = document.createElement('div'); editor.className = 'task-editor';
  form.before(editor); editor.append(form, runBox);
}
function setupBrainstorm(card, active, saveCard) {
  const section = document.createElement('section'); section.className = 'worktree-box';
  section.innerHTML = `<h3>${markupText("Brainstorming & création de tâches")}</h3><p class="hint">${markupText("L’agent découpe votre idée en propositions de tickets, en lecture seule. Consomme ses quotas. Créez une tâche locale pour la relire et la modifier. Un connecteur compatible peut ensuite la publier dans votre outil de tickets.")}</p><button type="button" id="brainstorm-ticket" ${active ? 'disabled' : ''}>${markupText("Proposer des tickets avec l’agent")}</button><div id="brainstorm-proposals" aria-live="polite"></div>${card.ticket || getState().connectors?.providers?.tickets !== 'jira' ? '' : `<form id="jira-create-form"><p class="hint">${markupText("Destination : {0} · la création utilise le contenu enregistré de cette fiche, questions comprises.", esc(getState().settings.jiraUrl || msg("Connectez Jira dans les réglages")))}</p><div class="form-row"><label>${markupText("Clé du projet Jira")}<input name="project" ${card.ticketCreation ? '' : 'required'} pattern="[A-Z][A-Z0-9_]*" maxlength="100" placeholder="PROJ"></label><label>${markupText("Nom exact du type Jira")}<input name="issueType" ${card.ticketCreation ? '' : 'required'} maxlength="100" placeholder="${markupText("Task, Story, Tâche…")}"></label></div>${card.ticketCreation ? `<p class="inline-error">${markupText("Envoi précédent incertain. Vérifiez Jira avant de réessayer pour éviter un doublon.")}</p><label>${markupText("Ticket déjà créé à rattacher")}<input name="existingKey" placeholder="PROJ-123"></label><label class="check"><input name="confirmRetry" type="checkbox"> ${markupText("J’ai vérifié dans Jira que le ticket n’existe pas et je souhaite réessayer.")}</label>` : ''}<button id="publish-jira" ${active ? 'disabled' : ''}>${markupText("Relire et créer dans Jira")}</button></form>`}`;
  $('#card-form').after(section);
  if (!card.ticket && getState().connectors?.providers?.tickets !== 'jira' && ticketSupports(card, 'create')) {
    const publish = document.createElement('button'); publish.type = 'button'; publish.disabled = active;
    publish.textContent = msg("Créer dans ") + connectorName('tickets');
    publish.onclick = () => action(async () => {
      if (!$('#card-form').reportValidity()) return;
      if (!(await confirmAction(msg("Envoyer le contenu de cette carte à ") + connectorName('tickets') + ' ?', msg("Créer le ticket distant ?"), msg("Créer le ticket")))) return;
      publish.disabled = true;
      try { await saveCard(); await api(cardPath(card.id) + '/create-ticket', 'POST', {}); toast(msg("Ticket distant créé.")); }
      finally { await refresh(); renderDetail(); await updateRun(); }
    });
    section.append(publish);
  }
  $('#brainstorm-ticket').onclick = () => action(async () => {
    if (!$('#card-form').reportValidity()) return;
    $('#brainstorm-ticket').disabled = true;
    try {
      await saveCard(true);
      const run = await api(cardPath(card.id) + '/brainstorm', 'POST', { reply: msg("Propose maintenant le découpage en tâches à partir de notre discussion et des décisions prises.") });
      await refresh(); renderDetail(); selectedRun = run.id; $('#run-select').value = run.id; await updateRun();
    } finally { if ($('#brainstorm-ticket')) $('#brainstorm-ticket').disabled = getState().runs.some(r => r.cardId === card.id && r.status === 'running') || card.archived; }
  });
  renderBrainstorm(card.id);
  if ($('#jira-create-form')) $('#jira-create-form').onsubmit = e => {
    e.preventDefault(); action(async () => {
      if (!$('#card-form').reportValidity()) return;
      const body = Object.fromEntries(new FormData(e.target)); body.confirmRetry = e.target.elements.confirmRetry?.checked === true;
      if (body.existingKey) { delete body.project; delete body.issueType; }
      await saveCard(true);
      if (!(await confirmAction(msg("Site : {0}\nProjet : {1}\nType : {2}\nTitre : {3}\n\n{4}\n\nLes critères et les instructions de la fiche seront également envoyés.", getState().settings.jiraUrl, body.project || body.existingKey, body.issueType || msg("Rattachement"), $('#card-form').elements.title.value, $('#card-form').elements.description.value), body.existingKey ? msg("Rattacher le ticket existant ?") : msg("Créer ce ticket Jira ?"), body.existingKey ? msg("Rattacher") : msg("Créer dans Jira")))) return;
      $('#publish-jira').disabled = true;
      try { await api(cardPath(card.id) + '/create-ticket', 'POST', body); toast(msg("Ticket Jira lié à la carte.")); }
      finally { await refresh(); renderDetail(); await updateRun(); }
    });
  };
  if (card.archived) section.querySelectorAll('button, input').forEach(el => el.disabled = true);
}

function addCriterion(item, disabled) {
  const row = document.createElement('div'); row.className = 'criterion-row';
  row.innerHTML = `<input type="checkbox" aria-label="${markupText("Critère vérifié")}" ${item.done ? 'checked' : ''} ${disabled ? 'disabled' : ''}><input type="text" aria-label="${markupText("Critère de réussite")}" maxlength="500" required value="${esc(item.text)}" placeholder="${markupText("Ex. L’export respecte les filtres sélectionnés")}" ${disabled ? 'disabled' : ''}><button type="button" aria-label="${markupText("Retirer ce critère")}" ${disabled ? 'disabled' : ''}>×</button>`;
  row.querySelector('button').onclick = () => { row.remove(); $('#card-form').dispatchEvent(new Event('input', { bubbles: true })); }; $('#criteria-list').append(row);
}
function renderReviewReport(cardId) {
  const host = $('#pr-review-result'); if (!host) return;
  const run = getState().runs.find(r => r.id === selectedRun && r.cardId === cardId && r.kind === 'pr-review') || getState().runs.find(r => r.cardId === cardId && r.kind === 'pr-review');
  if (!run) { host.textContent = msg("Aucune review lancée."); return; }
  const reportSignature = JSON.stringify([run, getState().cards.find(c => c.id === cardId)?.publishing, getState().cards.find(c => c.id === cardId)?.archived, getState().runs.some(r => r.cardId === cardId && r.status === 'running')]);
  if (host.dataset.signature === reportSignature) return; host.dataset.signature = reportSignature;
  const labels = { P0: msg("Critique"), P1: msg("Important"), P2: msg("Modéré"), P3: msg("Mineur"), suggestion: msg("Suggestion facultative") };
  const context = run.reviewContext;
  host.innerHTML = `<p class="hint">${markupText("Review du {0} · source {1} / cible {2} · {3}", esc(new Date(run.startedAt).toLocaleString(locale)), esc(context?.source?.slice(0, 12)), esc(context?.target?.slice(0, 12)), esc(runNames[run.status]))}</p>`;
  if (run.status === 'running') { host.innerHTML += `<p class="hint">${markupText("Analyse en cours… Suivez son activité dans le journal.")}</p>`; return; }
  if (!run.review || run.status !== 'completed') { host.innerHTML += `<p class="inline-error">${esc(run.reviewError || msg("Review non aboutie. Consultez le journal avant de relancer."))}</p>`; return; }
  host.innerHTML += `<p class="entry-description">${esc(run.review.summary)}</p><p class="hint">${markupText("{0} problème(s), {1} suggestion(s). Ce rapport concerne uniquement les commits indiqués ; relancez après toute nouvelle publication.", run.review.findings.filter(f => f.priority !== 'suggestion').length, run.review.findings.filter(f => f.priority === 'suggestion').length)}</p>`;
  const card = getState().cards.find(c => c.id === cardId), locked = card.archived || card.publishing || getState().runs.some(r => r.cardId === cardId && r.status === 'running');
  const coverageLabels = { covered: msg("Couvert"), 'not-covered': msg("Non couvert"), unverifiable: msg("Impossible à vérifier") };
  host.innerHTML += `<h4>${markupText("Couverture des critères de réussite")}</h4><p class="hint">${markupText("Évaluation de l’agent sur le besoin au moment de la review. Les cases restent à valider par vous.")}</p>`;
  host.innerHTML += !run.review.criteria ? `<p class="hint">${markupText("Cette ancienne review ne contient pas de bilan des critères. Relancez-la pour obtenir ce contrôle.")}</p>` : !run.review.criteria.length ? `<p class="inline-error">${markupText("Aucun critère de réussite défini lors de cette review : la couverture fonctionnelle n’a pas été évaluée.")}</p>` : run.review.criteria.map(item => `<article class="review-finding"><h4>${esc(coverageLabels[item.status])} · ${esc(item.text)}</h4><p class="entry-description">${esc(item.evidence)}</p></article>`).join('');
  if (run.reviewStaleReason) host.innerHTML += `<p class="inline-error">${markupText("Review à refaire : {0}", esc(run.reviewStaleReason))}</p>`;
  host.innerHTML += run.review.findings.map((f, index) => `<article class="review-finding"><label class="check"><input type="checkbox" data-finding="${index}" ${locked || run.reviewStaleReason ? 'disabled' : ''}> ${markupText("Inclure dans la correction")}</label><h4><span class="review-priority priority-${f.priority}">${f.priority} · ${labels[f.priority]}</span> ${esc(f.title)}</h4><code>${esc(f.file)}${f.line ? ':' + f.line : ''}</code><p>${esc(f.problem)}</p><p><strong>${markupText("Correction proposée :")}</strong> ${esc(f.suggestion)}</p></article>`).join('');
  host.innerHTML += `<div class="row-actions"><button id="check-review" ${locked ? 'disabled' : ''}>${markupText("Vérifier l’actualité")}</button>${run.review.findings.length ? `<button id="fix-review" class="primary" ${locked || run.reviewStaleReason ? 'disabled' : ''}>${markupText("Corriger la sélection avec l’agent")}</button>` : ''}</div><p class="hint">${markupText("La correction utilise le même worktree. Ensuite : relisez, publiez les changements, puis relancez Review. Aucune publication automatique.")}</p>`;
  $('#check-review').onclick = () => action(async () => { $('#check-review').disabled = true; try { await api(cardPath(cardId) + '/check-review', 'POST', { runId: run.id }); await refresh(); renderReviewReport(cardId); } finally { if ($('#check-review')) $('#check-review').disabled = locked; } });
  if ($('#fix-review')) $('#fix-review').onclick = () => action(async () => {
    const indexes = [...host.querySelectorAll('[data-finding]:checked')].map(e => Number(e.dataset.finding));
    if (!indexes.length) return toast(msg("Sélectionnez au moins un problème à corriger."), true);
    $('#fix-review').disabled = true;
    try { const correction = await api(cardPath(cardId) + '/fix-review', 'POST', { runId: run.id, indexes }); await refresh(); renderDetail(); selectedRun = correction.id; $('#run-select').value = correction.id; await updateRun(); toast(msg("Correction lancée dans le worktree de la tâche.")); }
    finally { await refresh(); if ($('#pr-review-result')) renderReviewReport(cardId); }
  });
  if (!run.review.findings.length) host.insertAdjacentHTML('beforeend', `<p>${markupText("Aucun problème étayé relevé. Cela ne garantit pas l’absence de défaut.")}</p>`);
}


function projectConfig(repo) {
  const project = getState().projects.find(p => sameRepository(p.repo, repo, getState().settings.platform));
  return { ...getState().settings, ...(project?.baseBranch ? { baseBranch: project.baseBranch } : {}), ...(project?.jiraMode === 'off' ? { jiraAutoTransition: false } : project?.jiraMode === 'custom' ? { jiraAutoTransition: true, jiraStatusMap: project.jiraStatusMap } : {}) };
}
function updateDevServers() {
  if (!$('#dev-status')) return;
  const card = getState().cards.find(card => card.id === selected), session = (getState().devServers || []).find(session => session.cardId === selected);
  if ($('#service-worktree-paths')) $('#service-worktree-paths').textContent = Object.entries(card?.serviceWorktrees || {}).map(([role, worktree]) => msg("Worktree {0} : {1} · {2}", role, worktree.path, worktree.branch)).join(' · ');
  const project = getState().projects.find(project => sameRepository(project.repo, card?.repo, getState().settings.platform));
  const linkedProject = getState().projects.find(item => item.id === project?.linkedProjectId);
  $('#dev-context').textContent = session && devRunning(selected)
    ? session.services.map(service => `${service.name} : ${service.repo}`).join(' · ')
    : `${project?.name || msg("Projet de la carte")} : ${card?.worktree?.path || msg("worktree à préparer")}${linkedProject ? msg(" · {0} (dépôt local) : {1}", linkedProject.name, linkedProject.repo) : project?.linkedProjectId ? msg(" · Projet lié introuvable : vérifie les réglages.") : ''}`;
  $('#start-dev').disabled = !card?.worktree || card.archived || devRunning(selected);
  $('#stop-dev').disabled = !devRunning(selected);
  $('#dev-status').textContent = session ? session.services.map(service => `${service.name} : ${service.status === 'running' ? msg("en cours") : service.status === 'stopping' ? msg("arrêt demandé") : msg("arrêté (code ") + service.exitCode + ')'}`).join(' · ') : msg("Aucun serveur lancé.");
  $('#dev-log').textContent = session?.log || '';
  $('#remove-worktree').disabled = !card?.worktree || card?.publishing || getState().runs.some(run => run.cardId === selected && run.status === 'running') || devRunning(selected);
}
function prMarkup(card, link = true) {
  const pr = card.pullRequest;
  if (!pr) return `<p class="hint">${markupText("Aucune PR enregistrée.")}</p>`;
  const checkNames = { succeeded: msg("réussi"), pending: msg("en cours"), failed: msg("échec"), error: 'erreur', notApplicable: msg("non applicable"), notSet: msg("non renseigné") };
  return `${link ? `<a class="pr-link" href="${esc(pr.url)}" target="_blank" rel="noopener noreferrer">${markupText("PR #{0} ↗", pr.number)}</a>` : ''}<p class="state-badge">${esc(prLabel(pr))}</p><p class="hint">${pr.checkedAt ? msg("Dernier relevé : ") + esc(new Date(pr.checkedAt).toLocaleString(locale)) : msg("État distant non relevé.")}</p>${pr.refreshError ? `<p class="inline-error">${esc(messageText(pr.refreshError))}</p>` : ''}${pr.checksError ? `<p class="inline-error">${markupText("Contrôles indisponibles : {0}", esc(messageText(pr.checksError)))}</p>` : ''}${pr.checkedAt ? `<p class="hint">${markupText("{0} Les règles de branche et pipelines restent à vérifier dans le service de dépôt.", (pr.checks || []).length ? pr.checks.map(c => esc(c.name) + ' : ' + esc(checkNames[c.state] || c.state)).join(' · ') : msg("Aucun statut de contrôle remonté."))}</p>` : ''}<button type="button" ${connectorSupports(getState().connectors, 'repositories', 'refresh', repositoryProvider(getState(), card)) ? '' : 'hidden'} data-refresh-pr="${esc(card.id)}" ${card.publishing ? 'disabled' : ''}>${markupText("Actualiser cette PR")}</button>`;
}
function renderWorkflowDetail(card, active) {
  const dependencies = document.createElement('details'); dependencies.className = 'disclosure'; dependencies.open = Boolean(card.blockedBy?.length);
  dependencies.innerHTML = `<summary>${markupText("Bloquée par")} <span class="count">${blockers(card, getState().cards).length}</span></summary><label>${markupText("Tâches préalables (Ctrl pour sélectionner plusieurs tâches)")}<select id="blocked-by" multiple size="5" ${active ? 'disabled' : ''}>${getState().cards.filter(c => c.id !== card.id).map(c => `<option value="${esc(c.id)}" ${card.blockedBy?.includes(c.id) ? 'selected' : ''}>${esc(c.ticket?.key || '')} ${esc(c.title)}${c.status === 'done' ? msg(" · terminée") : c.archived ? msg(" · archivée") : ''}</option>`).join('')}</select></label><p class="hint">${markupText("Le lancement attend que toutes les tâches choisies soient terminées. Une tâche archivée inachevée reste bloquante. Maximum 20 ; les cycles sont refusés.")}</p>`;
  $('#card-form button[type=submit]').before(dependencies);
  const next = nextAction(card, getState().runs, getState().cards);
  if (next) { const p = document.createElement('p'); p.className = 'next-action'; p.textContent = next; $('#card-form').before(p); }
  const templateButton = document.createElement('button'); templateButton.type = 'button'; templateButton.textContent = msg("Enregistrer comme modèle");
  templateButton.onclick = () => { const form = $('#template-form'); form.elements.cardId.value = card.id; form.elements.name.value = card.title.slice(0, 100); $('#template-dialog').showModal(); form.elements.name.focus(); };
  $('.card-utilities').append(templateButton);
  const queueButton = document.createElement('button'); queueButton.type = 'button'; queueButton.textContent = getState().queue?.ids.includes(card.id) ? msg("Déjà dans la file") : msg("Ajouter à la file"); queueButton.disabled = active || card.status === 'done' || getState().queue?.ids.includes(card.id);
  queueButton.onclick = () => action(async () => { await api(cardPath(card.id) + '/queue', 'POST', {}); await refresh(); renderDetail(); await updateRun(); toast(msg("Tâche ajoutée. La file se pilote dans Agents.")); });
  $('#launch').after(queueButton);
  if (card.pullRequest) {
    const status = document.createElement('div'); status.innerHTML = prMarkup(card, false);
    if (card.pullRequest.status === 'completed' && !card.pullRequest.refreshError && card.status !== 'done' && !active) {
      const done = document.createElement('button'); done.type = 'button'; done.textContent = msg("Terminer la tâche après fusion");
      done.onclick = () => action(async () => {
        if (!(await confirmAction(msg("As-tu vérifié les critères ? Terminer la tâche appliquera aussi la correspondance des statuts si elle est activée."), msg("Terminer la tâche ?"), msg("Terminer")))) return;
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
function bind() {
$('#pr-form').onsubmit = e => { e.preventDefault(); action(async () => {
  if (!prPreview || !prCardId) return;
  const button = $('#publish-pr'); button.disabled = true; button.textContent = msg("Publication en cours…");
  try {
    const body = Object.fromEntries(new FormData(e.target)); body.commitChanges = e.target.elements.commitChanges.checked; body.fingerprint = prPreview.fingerprint;
    const updated = await api(cardPath(prCardId) + '/pull-request', 'POST', body);
    $('#pr-dialog').close(); await refresh();
    if ($('#card-dialog').open && selected === prCardId) { renderDetail(); await updateRun(); }
    toast(updated.pullRequest.ticketError ? msg("PR publiée. Liaison au ticket à réessayer depuis la carte.") : msg("PR publiée") + (updated.pullRequest.ticketLinked ? msg(" et liée à Jira.") : '.'));
  } finally { button.disabled = false; button.textContent = msg("Pousser et créer la PR brouillon"); }
}); };


}

  return { bind, openCard, updateRun, updateAutomation, updateDevServers, prMarkup, bindPrActions, addCriterion, renderTicketPreparation, renderReviewReport, get selectedRun() { return selectedRun; } };
}

export function createTicketPreparation({ document, getState, action, toast, addCriterion }) {
  const $ = selector => document.querySelector(selector);
function renderTicketPreparation(cardId) {
  const host = $('#ticket-proposal'); if (!host) return;
  const run = getState().runs.find(r => r.cardId === cardId && r.kind === 'ticket-preparation');
  if (!run) return;
  const card = getState().cards.find(c => c.id === cardId);
  const requirements = { title: card.title || '', description: card.description || '', instructions: card.instructions || '', criteria: (card.checklist || []).map(i => i.text) };
  const stale = JSON.stringify(requirements) !== JSON.stringify(run.requirements);
  const signature = JSON.stringify([run.id, run.status, run.attention, stale]);
  if (host.dataset.signature !== signature) {
    host.dataset.signature = signature;
    if (run.status === 'running') host.innerHTML = `<p role="status">${markupText("Préparation en cours… Suivez son activité dans le journal.")}</p>`;
    else if (run.status !== 'completed' || !run.preparation) host.innerHTML = `<p class="inline-error">${esc(messageText(run.attention) || msg("Préparation interrompue ou échouée. Consultez le journal avant de relancer."))}</p>`;
    else host.innerHTML = `<p class="hint">${markupText("Proposition à relire. Un critère par ligne (20 maximum, 500 caractères chacun). Les questions restent à trancher avant le développement.")}</p><label>${markupText("Critères proposés")}<textarea id="proposed-criteria" rows="6">${esc(run.preparation.checklist.map(i => i.text).join('\n'))}</textarea></label><label>${markupText("Questions à trancher (une par ligne, 10 maximum)")}<textarea id="proposed-questions" rows="4">${esc(run.preparation.questions.join('\n'))}</textarea></label>${stale ? `<p class="inline-error">${markupText("Le besoin enregistré a changé. Relancez la préparation.")}</p>` : ''}<button type="button" id="apply-ticket-proposal">${markupText("Remplacer les critères dans la fiche")}</button><p class="hint">${markupText("Les critères seront décochés et les questions ajoutées aux instructions locales. Enregistrez ensuite la carte pour conserver le résultat.")}</p>`;
  }
  const apply = $('#apply-ticket-proposal'); if (!apply) return;
  const locked = stale || card.archived || card.publishing || getState().runs.some(r => r.cardId === cardId && r.status === 'running');
  for (const field of host.querySelectorAll('textarea, button')) field.disabled = Boolean(locked);
  apply.disabled ||= host.dataset.applied === run.id;
  apply.onclick = () => action(async () => {
    const lines = selector => $(selector).value.split('\n').map(s => s.trim()).filter(Boolean);
    const criteria = lines('#proposed-criteria'), questions = lines('#proposed-questions');
    if (!criteria.length || criteria.length > 20 || questions.length > 10 || [...criteria, ...questions].some(s => s.length > 500)) return toast(msg("De 1 à 20 critères et au plus 10 questions, de 500 caractères chacun."), true);
    const instructions = $('#instructions');
    const nextInstructions = instructions.value + (questions.length ? msg("\n\nQuestions à trancher avant développement :\n") + questions.map(q => '- ' + q).join('\n') : '');
    if (nextInstructions.length > 20000) return toast(msg("Les instructions dépasseraient 20 000 caractères. Raccourcissez-les avant de transférer la proposition."), true);
    $('#criteria-list').replaceChildren(); criteria.forEach(text => addCriterion({ text, done: false }, false));
    $('#criteria-list').closest('details').open = true;
    instructions.value = nextInstructions;
    $('#card-form').dispatchEvent(new Event('input', { bubbles: true }));
    host.dataset.applied = run.id;
    apply.disabled = true;
    toast(msg("Proposition transférée. Relisez puis enregistrez la carte."));
  });
}

  return { renderTicketPreparation };
}

export function createAutomationView({ document, getState, api, action, refresh, onChanged, onRunSelected }) {
  const $ = selector => document.querySelector(selector);
function updateAutomation(cardId) {
  const host = $('#automation-status'), card = getState().cards.find(c => c.id === cardId);
  if (!host || !card) return;
  const flow = card.automation, project = getState().projects.find(p => sameRepository(p.repo, card.repo, getState().settings.platform));
  host.hidden = !flow && !project?.automationEnabled;
  const signature = JSON.stringify([flow, card.archived, card.publishing, card.pullRequest]);
  if (host.dataset.signature === signature) return; host.dataset.signature = signature;
  const steps = { development: msg("Développement"), tests: msg("Vérifications"), publish: msg("Publication PR"), review: msg("Review"), correction: msg("Correction P0/P1") };
  const states = { running: msg("En cours"), ready: msg("À vérifier par toi"), blocked: msg("Intervention nécessaire"), error: msg("Échec"), interrupted: msg("Interrompu"), cancelled: msg("Arrêté") };
  host.innerHTML = `<h3>${markupText("Parcours automatique")}</h3><p role="status">${flow ? msg("{0} · {1} · {2}/2 cycles de correction", esc(steps[flow.step]), esc(states[flow.state]), flow.cycles) : msg("Activé pour ce projet. Le prochain lancement en modification déroulera le parcours complet.")}</p>${flow?.message ? `<p>${esc(messageText(flow.message))}</p>` : ''}${flow?.acceptance ? `<p>${flow.acceptance.total ? msg("Critères : {0}/{1} couverts · {2} non couverts · {3} impossibles à vérifier", flow.acceptance.covered, flow.acceptance.total, flow.acceptance.notCovered, flow.acceptance.unverifiable) : msg("Aucun critère de réussite défini.")}</p>` : ''}${flow?.counts ? `<p>${Object.entries(flow.counts).map(([p, n]) => `${p === 'suggestion' ? msg("Suggestions") : p} : ${n}`).join(' · ')}</p>` : ''}${card.pullRequest ? `<p><a href="${esc(card.pullRequest.url)}" target="_blank" rel="noopener noreferrer">${markupText("Ouvrir la PR #{0} ↗", card.pullRequest.number)}</a></p>` : ''}${flow?.tests ? `<details><summary>${markupText("Vérifications : {0}", esc({ passed: msg("réussies"), failed: msg("échouées"), running: msg("en cours") }[flow.tests.state]))}</summary><pre class="log">${esc(flow.tests.command)}
${esc(flow.tests.output)}
${esc(flow.tests.error || '')}</pre></details>` : ''}${flow?.state === 'running' ? `<button type="button" id="stop-automation" class="danger">${markupText("Arrêter le parcours")}</button>` : ['error', 'interrupted', 'cancelled'].includes(flow?.state) ? `<button type="button" id="resume-automation" ${card.archived || card.publishing ? 'disabled' : ''}>${markupText("Reprendre l’étape")}</button>` : ''}${flow?.reportId ? `<button type="button" id="show-automation-review">${markupText("Consulter la review")}</button>` : ''}`;
  if ($('#stop-automation')) $('#stop-automation').onclick = () => action(async () => { await api(cardPath(card.id) + '/stop-automation', 'POST', {}); await refresh(); });
  if ($('#resume-automation')) $('#resume-automation').onclick = () => action(async () => { await api(cardPath(card.id) + '/resume-automation', 'POST', {}); await refresh(); await onChanged(); });
  if ($('#show-automation-review')) $('#show-automation-review').onclick = () => action(async () => { await onRunSelected(flow.reportId); $('#pr-review-result')?.scrollIntoView({ block: 'start' }); });
}
  return { updateAutomation };
}
