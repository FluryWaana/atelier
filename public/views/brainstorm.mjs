import { agentName, agentProviders } from '../agent-options.mjs';
import { messageText, msg, markupText, locale } from '../i18n.mjs';
import { esc, cardPath } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';

export function createBrainstormView({ document, getState, localStorage, taskAgentFields, api, action, refresh, openCard, toast, renderBrainstorm, FormData = globalThis.FormData, Option = globalThis.Option }) {
  const $ = selector => document.querySelector(selector);
  let brainstormSession = '', brainstormInitialized = false, brainstormingBusy = false;
function rememberBrainstorm() {
  try {
    const draft = JSON.stringify({ id: brainstormSession, reply: $('#brainstorm-reply').value, fields: Object.fromEntries([...$('#brainstorm-form').elements].filter(field => field.name).map(field => [field.name, field.value])) });
    localStorage.setItem('atelier-brainstorm-page', draft);
    localStorage.setItem('atelier-brainstorm-draft:' + brainstormSession, draft);
  } catch {}
}

function loadBrainstormSession(id) {
  rememberBrainstorm();
  const form = $('#brainstorm-form'), session = [...(getState().brainstorms || []), ...getState().cards].find(c => c.id === id);
  brainstormSession = session?.id || '';
  $('#brainstorm-reply').value = '';
  form.reset();
  form.elements.agent.value = session?.agent || getState().settings.agent || 'codex';
  taskAgentFields(form, session || {});
  if (session?.repo && ![...form.elements.repo.options].some(option => option.value === session.repo)) form.elements.repo.add(new Option(session.repo, session.repo));
  if (session) for (const name of ['title', 'description', 'instructions', 'repo']) form.elements[name].value = session[name] || '';
  try {
    const saved = JSON.parse(localStorage.getItem('atelier-brainstorm-draft:' + brainstormSession) || 'null');
    if (saved && !session) {
      form.elements.agent.value = saved.fields?.agent || form.elements.agent.value;
      taskAgentFields(form, saved.fields || {});
      for (const [name, value] of Object.entries(saved.fields || {})) {
        if (!form.elements[name] || typeof value !== 'string') continue;
        if (name === 'repo' && value && ![...form.elements.repo.options].some(option => option.value === value)) form.elements.repo.add(new Option(value, value));
        form.elements[name].value = value;
      }
    }
    $('#brainstorm-reply').value = typeof saved?.reply === 'string' ? saved.reply : '';
  } catch {}
  const host = $('#brainstorm-page-proposals'); host.replaceChildren(); delete host.dataset.signature; delete host.dataset.runId;
  const history = $('#brainstorm-chat-history'); delete history.dataset.signature; history.scrollTop = history.scrollHeight;
  showBrainstormTasks(false);
  rememberBrainstorm(); renderBrainstormPage();
  if (session) $('#brainstorm-reply').focus();
  else form.elements.description.focus();
}

function renderBrainstormPage() {
  const form = $('#brainstorm-form'), picker = $('#brainstorm-session');
  const sessions = [...(getState().brainstorms || []), ...getState().cards].filter(c => !c.archived && (c.brainstormSession || getState().runs.some(r => r.cardId === c.id && r.kind === 'brainstorm')));
  $('#brainstorm-toolbar').hidden = !sessions.length;
  const signature = JSON.stringify(sessions.map(c => [c.id, c.title]));
  if (picker.dataset.signature !== signature) {
    picker.innerHTML = `<option value="">${markupText("Nouvelle discussion")}</option>` + sessions.map(c => `<option value="${esc(c.id)}">${esc(c.title)}</option>`).join('');
    picker.dataset.signature = signature;
  }
  const projects = JSON.stringify(getState().projects.map(p => [p.repo, p.name]));
  if (form.elements.repo.dataset.signature !== projects) {
    const value = form.elements.repo.value;
    form.elements.repo.innerHTML = `<option value="">${markupText("Sans dépôt · brainstorming général")}</option>` + getState().projects.map(p => `<option value="${esc(p.repo)}">${esc(p.name)}</option>`).join('');
    form.elements.repo.dataset.signature = projects;
    if (value && !getState().projects.some(p => p.repo === value)) form.elements.repo.add(new Option(value, value));
    form.elements.repo.value = value;
  }
  if (!brainstormInitialized) {
    brainstormInitialized = true;
    bindBrainstormSplitter();
    let saved; try { saved = JSON.parse(localStorage.getItem('atelier-brainstorm-page') || 'null'); } catch {}
    brainstormSession = sessions.some(c => c.id === saved?.id) ? saved.id : '';
    const session = sessions.find(c => c.id === brainstormSession);
    const fields = session || (!saved?.id ? saved?.fields : {}) || {};
    $('#brainstorm-reply').value = brainstormSession && typeof saved?.reply === 'string' ? saved.reply : '';
    form.elements.agent.value = agentProviders(getState().settings).some(provider => provider.id === fields.agent) ? fields.agent : getState().settings.agent || 'codex';
    taskAgentFields(form, fields);
    for (const [name, value] of Object.entries(fields)) {
      if (!form.elements[name] || typeof value !== 'string') continue;
      if (name === 'repo' && value && ![...form.elements.repo.options].some(o => o.value === value)) form.elements.repo.add(new Option(value, value));
      form.elements[name].value = value;
    }
    form.oninput = rememberBrainstorm;
    form.onchange = () => { rememberBrainstorm(); renderBrainstormChat(getState().runs.some(run => run.cardId === brainstormSession && run.status === 'running')); };
    picker.onchange = () => loadBrainstormSession(picker.value);
    $('#brainstorm-new').onclick = () => loadBrainstormSession('');
    $('#brainstorm-toggle-tasks').onclick = () => {
      const show = $('#brainstorm-layout').dataset.panel !== 'tasks';
      showBrainstormTasks(show);
      (show ? $('#brainstorm-results') : $('#brainstorm-reply')).focus({ preventScroll: true });
    };
    $('#brainstorm-generate').onclick = () => action(() => submitBrainstorm(false, true, msg("Propose un découpage en tâches."), true));
    $('#brainstorm-retry').onclick = () => action(() => submitBrainstorm(false, true, getState().runs.find(r => r.cardId === brainstormSession && r.kind === 'brainstorm')?.instructions || msg("Reprends l’exploration à partir de notre discussion.")));
    form.onsubmit = event => { event.preventDefault(); action(() => submitBrainstorm(false)); };
    $('#brainstorm-create-local').onclick = () => action(() => submitBrainstorm(true));
    $('#brainstorm-reply').oninput = () => { rememberBrainstorm(); resizeBrainstormComposer(); };
    $('#brainstorm-reply').onkeydown = event => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); if (!$('#brainstorm-send').disabled) $('#brainstorm-reply-form').requestSubmit(); }
    };
    $('#brainstorm-reply-form').onsubmit = event => { event.preventDefault(); action(() => submitBrainstorm(false, true)); };
  }
  picker.value = brainstormSession;
  $('#brainstorm-new').hidden = !brainstormSession;
  picker.disabled = $('#brainstorm-new').disabled = brainstormingBusy;
  const run = getState().runs.find(r => r.cardId === brainstormSession && r.kind === 'brainstorm');
  const active = run?.status === 'running';
  const proposed = getState().runs.some(run => run.cardId === brainstormSession && run.kind === 'brainstorm' && run.proposals?.length);
  const step = !brainstormSession ? 'idea' : proposed ? 'tasks' : 'chat';
  for (const name of ['idea', 'chat', 'tasks']) $('#brainstorm-step-' + name).setAttribute('aria-current', name === step ? 'step' : 'false');
  $('#brainstorm-start').textContent = brainstormingBusy ? msg("Démarrage…") : msg("Démarrer la discussion");
  for (const field of form.elements) field.disabled = Boolean(brainstormSession) || brainstormingBusy;
  form.elements.description.required = !form.elements.title.value.trim();
  renderBrainstormChat(active);
  resizeBrainstormComposer();
  const status = $('#brainstorm-page-status');
  status.textContent = active ? messageText(run.activity) || msg("Brainstorming en cours…") : run ? run.brainstorm || run.proposals ? msg("Poursuivez la discussion ou demandez un découpage quand votre idée est prête.") : messageText(run.attention) || msg("Brainstorming interrompu ou échoué. Vous pouvez relancer l’exploration.") : msg("La discussion est enregistrée. Réessayez pour lancer l’agent.");
  status.hidden = !brainstormSession || Boolean(run && !active && !run.attention && run.status === 'completed');
  $('#brainstorm-retry').hidden = !brainstormSession || active || Boolean(run && run.status === 'completed' && !run.attention);
  $('#brainstorm-retry').disabled = brainstormingBusy;
  $('#brainstorm-page-stop').hidden = !active;
  $('#brainstorm-page-stop').onclick = () => action(async () => { await api(`/api/runs/${run.id}/cancel`, 'POST', {}); await refresh(); });
  renderBrainstorm(brainstormSession, $('#brainstorm-page-proposals'));
  $('#brainstorm-results').hidden = !proposed;
  $('#brainstorm-splitter').hidden = !proposed;
  $('#brainstorm-toggle-tasks').hidden = !proposed;
  if (!proposed) showBrainstormTasks(false);
}

function bindBrainstormSplitter() {
  const splitter = $('#brainstorm-splitter'), layout = $('#brainstorm-layout');
  let share = 70, drag = null;
  const resize = value => {
    share = Math.max(30, Math.min(75, value));
    layout.style.setProperty('--brainstorm-chat-size', share + 'fr');
    layout.style.setProperty('--brainstorm-task-size', (100 - share) + 'fr');
    splitter.setAttribute('aria-valuenow', String(Math.round(share)));
    splitter.setAttribute('aria-valuetext', msg("Discussion : {0} % · Tâches : {1} %", Math.round(share), 100 - Math.round(share)));
    resizeBrainstormComposer();
  };
  splitter.onpointerdown = event => {
    if (event.button !== 0 || !event.isPrimary) return;
    event.preventDefault(); splitter.focus();
    drag = { id: event.pointerId, offset: event.clientX - splitter.getBoundingClientRect().left };
    splitter.dataset.dragging = 'true'; splitter.setPointerCapture(event.pointerId);
  };
  splitter.onpointermove = event => {
    if (event.pointerId !== drag?.id) return;
    const bounds = layout.getBoundingClientRect(), width = bounds.width - splitter.offsetWidth;
    if (width > 0) resize((event.clientX - bounds.left - drag.offset) / width * 100);
  };
  splitter.onpointerup = splitter.onpointercancel = event => {
    if (splitter.hasPointerCapture(event.pointerId)) splitter.releasePointerCapture(event.pointerId);
    drag = null; delete splitter.dataset.dragging;
  };
  splitter.onlostpointercapture = () => { drag = null; delete splitter.dataset.dragging; };
  splitter.onkeydown = event => {
    const value = { ArrowLeft: share - 5, ArrowRight: share + 5, Home: 30, End: 75 }[event.key];
    if (value === undefined) return;
    event.preventDefault(); resize(value);
  };
}

function showBrainstormTasks(show) {
  $('#brainstorm-layout').dataset.panel = show ? 'tasks' : 'chat';
  $('#brainstorm-toggle-tasks').textContent = show ? msg("Revenir à la discussion") : msg("Voir les tâches");
}

async function submitBrainstorm(local, replying = false, message = '', includeDraft = false) {
  const form = $('#brainstorm-form');
  form.elements.description.required = !form.elements.title.value.trim();
  if (brainstormingBusy || getState().runs.some(run => run.cardId === brainstormSession && run.status === 'running') || local && brainstormSession) return;
  if (!brainstormSession && !form.reportValidity()) return;
  const draft = $('#brainstorm-reply').value.trim();
  const reply = replying ? includeDraft ? [draft, message].filter(Boolean).join('\n\n') : message || draft : '';
  if (replying && (!brainstormSession || !reply || (!message || includeDraft && draft) && !$('#brainstorm-reply-form').reportValidity())) return;
  let body;
  if (!brainstormSession) {
    body = Object.fromEntries(new FormData(form));
    if (!body.title.trim() && !body.description.trim()) return;
    if (!body.title.trim()) { body.title = body.description.trim().split('\n')[0].slice(0, 90); form.elements.title.value = body.title; }
  }
  rememberBrainstorm();
  brainstormingBusy = true;
  try {
    renderBrainstormPage();
    if (local) {
      const card = await api('/api/cards', 'POST', { ...body, mode: 'edit' });
      await refresh(); await openCard(card.id); toast(msg("Tâche locale créée. Jira est facultatif."));
      return;
    }
    if (!brainstormSession) {
      const session = await api('/api/brainstorms', 'POST', body);
      brainstormSession = session.id;
      try { localStorage.removeItem('atelier-brainstorm-draft:'); } catch {}
    }
    rememberBrainstorm();
    const run = await api(cardPath(brainstormSession) + '/brainstorm', 'POST', { reply });
    getState().runs = [run, ...getState().runs.filter(previous => previous.id !== run.id)];
    if (replying && (!message || includeDraft) && run.status !== 'failed') $('#brainstorm-reply').value = '';
  } finally {
    brainstormingBusy = false;
    rememberBrainstorm();
    try { await refresh(); }
    finally { renderBrainstormPage(); if (!local) (brainstormSession ? $('#brainstorm-reply') : form.elements.description).focus(); }
  }
}

function renderBrainstormChat(active) {
  const discussing = Boolean(brainstormSession), context = $('#brainstorm-context');
  const form = $('#brainstorm-form');
  form.hidden = discussing;
  $('#brainstorm-session-context').hidden = !discussing;
  $('#brainstorm-session-summary').hidden = !discussing;
  if (discussing) {
    const fields = form.elements;
    const project = getState().projects.find(project => project.repo === fields.repo.value)?.name || fields.repo.value || msg("Sans dépôt · brainstorming général");
    $('#brainstorm-session-summary').textContent = project + ' · ' + agentName(getState().settings, fields.agent.value);
    $('#brainstorm-session-context').innerHTML = `<p class="hint">${markupText("Ces réglages sont fixés pour cette discussion. Pour changer de projet ou de modèle, démarrez une nouvelle discussion.")}</p><dl class="brainstorm-context-values">${[
      [msg("Projet"), project], [msg("Agent"), agentName(getState().settings, fields.agent.value)],
      [msg("Modèle"), fields.model.value || fields.model.placeholder || msg("Configuration CLI")], [msg("Niveau de réflexion"), fields.effort.selectedOptions?.[0]?.textContent || fields.effort.value || msg("Par défaut")],
      [msg("Votre idée"), fields.description.value], [msg("Contraintes à respecter (facultatif)"), fields.instructions.value],
    ].filter(([, value]) => value).map(([label, value]) => `<div><dt>${esc(label)}</dt><dd>${esc(value)}</dd></div>`).join('')}</dl>`;
  }
  if (context.dataset.session !== brainstormSession) { context.open = !discussing; context.dataset.session = brainstormSession; }
  $('#brainstorm-editor-kicker').hidden = discussing;
  $('#brainstorm-editor-title').textContent = discussing ? $('#brainstorm-form').elements.title.value || msg("Votre discussion") : msg("Qu’aimeriez-vous améliorer ?");
  $('#brainstorm-editor-intro').hidden = discussing;
  $('#brainstorm-chat').hidden = !discussing;
  $('#brainstorm-reply').disabled = brainstormingBusy;
  $('#brainstorm-reply-form').dataset.active = String(active);
  $('#brainstorm-composer-agent').textContent = agentName(getState().settings, $('#brainstorm-form').elements.agent.value);
  if (!discussing) return;
  const turns = getState().runs.filter(run => run.cardId === brainstormSession && run.kind === 'brainstorm').slice().reverse();
  const host = $('#brainstorm-chat-history');
  const signature = JSON.stringify(turns.map(run => [run.id, run.status, run.instructions, run.requirements, run.brainstorm, run.proposals, run.attention, run.activity, run.status !== 'running' && run.summary]));
  if (host.dataset.signature === signature) return;
  const follow = host.scrollHeight - host.scrollTop - host.clientHeight < 80;
  host.dataset.signature = signature;
  host.innerHTML = turns.map((run, index) => {
    const user = index === 0 || !run.instructions ? [run.requirements?.description || run.requirements?.title, run.requirements?.instructions, run.instructions].filter(Boolean).join('\n\n') : run.instructions;
    const questions = brainstormQuestions(run);
    const answer = run.brainstorm || run.proposals ? `${run.brainstorm?.message ? brainstormParagraphs(run.brainstorm.message) : ''}${run.proposals?.length ? `<p class="hint">${index === turns.length - 1 ? `<button type="button" class="text-button" data-brainstorm-show-tasks>${markupText("Voir les tâches proposées ({0})", run.proposals.length)}</button>` : markupText("{0} proposition(s) dans le panneau des tâches.", run.proposals.length)}</p>` : ''}${questions.length ? `<ul class="brainstorm-questions">${questions.map((question, q) => `<li>${index === turns.length - 1 ? `<button type="button" data-brainstorm-question="${q}">${esc(question)}<span>${markupText("Répondre")}</span></button>` : esc(question)}</li>`).join('')}</ul>` : ''}` : `<p>${esc(run.status === 'running' ? messageText(run.activity) || msg("Réflexion en cours…") : messageText(run.attention) || msg("Exploration interrompue ou échouée. Votre message reste dans la discussion ; vous pouvez réessayer."))}</p>`;
    const raw = !run.brainstorm && !run.proposals && run.status !== 'running' && run.summary ? `<details class="brainstorm-raw"><summary>${markupText("Voir la réponse brute de l’agent")}</summary><pre>${esc(run.summary)}</pre></details>` : '';
    return `<article class="brainstorm-message user"><strong>${markupText("Vous")}</strong>${brainstormParagraphs(user)}</article><article class="brainstorm-message assistant"><strong>${esc(agentName(getState().settings, run.agent, run))}</strong>${answer}${raw}</article>`;
  }).join('') || `<p class="hint">${markupText("Envoyez un message pour poursuivre votre idée.")}</p>`;
  const questions = brainstormQuestions(turns.at(-1));
  host.querySelectorAll('[data-brainstorm-show-tasks]').forEach(button => button.onclick = () => { showBrainstormTasks(true); $('#brainstorm-results').focus({ preventScroll: true }); });
  host.querySelectorAll('[data-brainstorm-question]').forEach(button => button.onclick = () => {
    const input = $('#brainstorm-reply');
    input.value += (input.value ? '\n\n' : '') + questions[Number(button.dataset.brainstormQuestion)] + '\n';
    rememberBrainstorm(); resizeBrainstormComposer(); input.focus();
  });
  if (follow) host.scrollTop = host.scrollHeight;
}

function resizeBrainstormComposer() {
  const input = $('#brainstorm-reply');
  input.style.height = 'auto';
  input.style.height = Math.min(180, Math.max(30, input.scrollHeight)) + 'px';
  input.style.overflowY = input.scrollHeight > 180 ? 'auto' : 'hidden';
  const active = $('#brainstorm-reply-form').dataset.active === 'true';
  $('#brainstorm-send').disabled = input.disabled || active || !input.value.trim();
  $('#brainstorm-generate').disabled = !brainstormSession || input.disabled || active;
  const proposed = getState().runs.some(run => run.cardId === brainstormSession && run.kind === 'brainstorm' && run.proposals?.length);
  $('#brainstorm-generate').textContent = input.value.trim() ? msg("Envoyer et préparer les tâches") : proposed ? msg("Actualiser les tâches") : msg("Préparer les tâches");
  $('#brainstorm-composer-hint').textContent = active ? msg("L’agent travaille. Vous pouvez préparer votre prochain message ici.") : msg("Entrée pour envoyer · Maj+Entrée pour revenir à la ligne.");
  $('#brainstorm-generate-hint').textContent = input.value.trim() ? msg("Votre message sera pris en compte.") : proposed ? msg("Les tâches déjà ajoutées restent inchangées.") : msg("Quand votre idée est assez claire.");
}


  return { renderBrainstormPage, submitBrainstorm, loadBrainstormSession, get session() { return brainstormSession; } };
}


export function createBrainstormProposals({ document, getState, api, action, refresh, openCard, toast }) {
  const $ = selector => document.querySelector(selector);
function renderBrainstorm(cardId, host = $('#brainstorm-proposals')) {
  if (!host) return;
  const latest = getState().runs.find(r => r.cardId === cardId && r.kind === 'brainstorm');
  const run = latest?.proposals?.length ? latest : getState().runs.find(r => r.cardId === cardId && r.kind === 'brainstorm' && r.proposals?.length) || latest;
  if (!run) {
    if (host.id !== 'brainstorm-page-proposals') return;
    host.innerHTML = `<div class="brainstorm-empty"><strong>${markupText("D’abord les idées, ensuite les tâches.")}</strong><p>${markupText("Répondez aux questions de l’agent. Quand le périmètre vous convient, demandez le découpage en tâches.")}</p></div>`;
    return;
  }
  const card = [...(getState().brainstorms || []), ...getState().cards].find(card => card.id === cardId);
  const changedProject = run.brainstormRepo !== undefined && run.brainstormRepo !== (card?.repo || '') && !sameRepository(run.brainstormRepo, card?.repo, getState().settings.platform);
  const signature = JSON.stringify([latest.id, latest.status, run.id, run.status, run.attention, run.proposals, changedProject, getState().cards.filter(c => c.brainstormSource?.runId === run.id).map(c => c.id)]);
  if (host.dataset.signature === signature) return;
  const expanded = host.dataset.runId === run.id ? new Set([...host.querySelectorAll('[data-brainstorm-proposal][open]')].map(item => Number(item.dataset.brainstormProposal))) : new Set([0]);
  host.dataset.signature = signature;
  if (run.status === 'running') { host.textContent = msg("Brainstorming en cours…"); return; }
  if (!run.proposals) { host.textContent = messageText(run.attention) || msg("Brainstorming interrompu ou échoué. Consultez le journal."); return; }
  if (!run.proposals.length) { host.innerHTML = `<div class="brainstorm-empty"><strong>${markupText("D’abord les idées, ensuite les tâches.")}</strong><p>${markupText("Répondez aux questions de l’agent. Quand le périmètre vous convient, demandez le découpage en tâches.")}</p></div>`; return; }
  host.dataset.runId = run.id;
  host.innerHTML = (changedProject ? `<p class="inline-error">${markupText("Le projet a changé. Demandez un nouveau découpage avant de créer des tâches.")}</p>` : run.id !== latest.id ? `<p class="hint">${markupText("Dernier découpage proposé. Demandez une mise à jour pour intégrer les échanges suivants.")}</p>` : '') + run.proposals.map((proposal, index) => {
    const draft = getState().cards.find(c => c.brainstormSource?.runId === run.id && c.brainstormSource.index === index);
    return `<details class="disclosure" data-brainstorm-proposal="${index}" ${expanded.has(index) ? 'open' : ''}><summary>${esc(proposal.title)}${draft ? `<span class="brainstorm-created">${markupText("Tâche créée")}</span>` : ''}</summary><p class="brainstorm-description">${esc(proposal.description)}</p><ul>${proposal.checklist.map(item => `<li>${esc(item.text)}</li>`).join('')}</ul>${proposal.questions.length ? `<p>${markupText("Questions à trancher :")}</p><ul>${proposal.questions.map(q => `<li>${esc(q)}</li>`).join('')}</ul>` : ''}<button type="button" data-brainstorm-index="${index}" ${(latest.status === 'running' || changedProject) && !draft ? 'disabled' : ''}>${draft ? msg("Ouvrir la tâche") : msg("Ajouter au tableau")}</button></details>`;
  }).join('');
  host.querySelectorAll('[data-brainstorm-index]').forEach(button => button.onclick = () => action(async () => {
    button.disabled = true;
    try {
      const existing = getState().cards.find(c => c.brainstormSource?.runId === run.id && c.brainstormSource.index === Number(button.dataset.brainstormIndex));
      if (existing) { await openCard(existing.id); return; }
      const draft = await api(cardPath(cardId) + '/brainstorm-draft', 'POST', { runId: run.id, index: Number(button.dataset.brainstormIndex) });
      await refresh();
      if (host.id === 'brainstorm-page-proposals') toast(msg("Tâche ajoutée au tableau."));
      else await openCard(draft.id);
    } finally { button.disabled = false; }
  }));
}


  return { renderBrainstorm };
}

export function brainstormQuestions(run) {
  return [...new Set([...(run?.brainstorm?.questions || []), ...(run?.proposals?.flatMap(proposal => proposal.questions) || [])])];
}

function brainstormParagraphs(text) {
  return text.split(/\r?\n\s*\r?\n/).filter(part => part.trim()).map(part => `<p>${esc(part)}</p>`).join('');
}
