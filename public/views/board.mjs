import { messageText, msg, markupText } from '../i18n.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { names, runNames, esc, cardPath } from '../view-helpers.mjs';
import { nextAction, prLabel } from '../workflow.mjs';

export function createBoardView({ document, window, api, action, confirmAction, refresh, openCard, toast, rerender }) {
  const $ = selector => document.querySelector(selector);
  
  let boardSignature = '';
  function render({ cards, state, query, focusFilter }) {
    const sameRepo = (a, b) => sameRepository(a, b, state.settings?.platform);
    const signature = JSON.stringify([cards, state.projects, state.runs.map(r => [r.id, r.status, r.activity]), query, focusFilter]);
    if (signature === boardSignature) return;
    boardSignature = signature;
    $('#board').innerHTML = Object.entries(names).map(([status, name]) => {
      const items = cards.filter(c => c.status === status);
      return `<div class="column ${status}" data-status="${status}"><div class="column-heading"><span class="status-dot"></span>${name}<span class="count">${items.length}</span></div>${items.map(c => {
        const run = state.runs.find(r => r.cardId === c.id && r.status === 'running') || state.runs.find(r => r.cardId === c.id);
        const project = state.projects.find(p => sameRepo(p.repo, c.repo));
        const projectPicker = `<select class="project-pill" data-card-project="${esc(c.id)}" aria-label="${markupText("Projet de {0}", esc(c.title))}" title="${esc(c.worktree ? msg("Retirez le worktree pour changer de projet.") : msg("Changer le projet"))}" ${c.worktree || c.publishing || run?.status === 'running' ? 'disabled' : ''}><option value="" ${!c.repo ? 'selected' : ''}>${markupText("Sans projet")}</option>${c.repo && !project ? `<option value="${esc(c.repo)}" selected>${esc(c.repo.split(/[\\/]/).pop())}</option>` : ''}${state.projects.map(p => `<option value="${esc(p.repo)}" ${sameRepo(p.repo, c.repo) ? 'selected' : ''}>${esc(p.name)}</option>`).join('')}</select>`;
        return `<article class="card" draggable="true" data-id="${esc(c.id)}"><div class="card-actions"><button type="button" class="card-actions-toggle" popovertarget="card-actions-${esc(c.id)}" aria-label="${markupText("Actions de {0}", esc(c.title))}" title="${markupText("Actions de la tâche")}"><svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><circle cx="3" cy="8" r="1.5"/><circle cx="8" cy="8" r="1.5"/><circle cx="13" cy="8" r="1.5"/></svg></button><div id="card-actions-${esc(c.id)}" class="card-action-menu" popover>${run?.status === 'running' ? `<button type="button" data-card-stop-agent="${esc(run.id)}" class="danger"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2"/></svg>${markupText("Arrêter l’agent")}</button>` : `<button type="button" data-archive-card="${esc(c.id)}" ${c.publishing ? 'disabled' : ''} title="${markupText("Déplacer dans les archives")}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 8h16v12H4zM3 4h18v4H3zM9 12h6"/></svg>${markupText("Archiver")}</button><button type="button" data-delete-card="${esc(c.id)}" class="danger" ${c.publishing ? 'disabled' : ''} title="${markupText("Supprimer uniquement dans Atelier")}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/></svg>${markupText("Supprimer localement")}</button>`}</div></div><span class="card-label">${esc(c.ticket?.key || msg("TÂCHE LOCALE"))}</span><button type="button" class="card-title card-open">${esc(c.title)}</button><span class="card-description">${esc(c.description || msg("Ouvrir pour ajouter du contexte et lancer un agent."))}</span>${run?.status === 'running' ? `<span class="card-live"><span class="live-dot"></span><span>${esc(messageText(run.activity) || msg("Travail en cours"))}</span></span><span class="card-clock" data-clock="${run.id}"></span>` : ''}${c.pullRequest ? `<span class="state-badge">${esc(prLabel(c.pullRequest))}</span>` : ''}${nextAction(c, state.runs, state.cards) ? `<p class="hint next-action">${esc(nextAction(c, state.runs, state.cards))}</p>` : ''}${state.queue?.ids.includes(c.id) ? `<span class="state-badge">${markupText("Dans la file")}</span>` : ''}${c.checklist?.length ? `<span class="card-criteria">${markupText("{0}/{1} critères vérifiés", c.checklist.filter(item => item.done).length, c.checklist.length)}</span>` : ''}${c.ticketSync?.state === 'error' ? `<span class="inline-error">${markupText("⚠ Statut Jira à vérifier")}</span>` : ''}<span class="card-bottom"><span class="card-pills"><span class="agent-pill ${c.agent}">${c.agent === 'claude' ? '✳ Claude' : '⌘ Codex'}</span>${projectPicker}</span><span>${esc(run ? runNames[run.status] : c.ticket?.status || msg("À démarrer"))}</span></span>${c.automation ? `<p class="hint">${markupText("Automatique · {0} · {1}", esc(({ development: msg("Développement"), tests: msg("Vérifications"), publish: msg("Publication"), review: msg("Review"), correction: msg("Correction") })[c.automation.step]), esc(messageText(c.automation.message) || msg("En cours")))}</p>` : ''}</article>`;
      }).join('') || `<div class="empty">${query || focusFilter !== 'all' ? msg("Aucune tâche correspondante") : status === 'todo' ? `${markupText("Le prochain pas")}<br>${markupText("commence ici.")}` : status === 'doing' ? `${markupText("Déposez une tâche ici")}<br>${markupText("pour lancer son agent.")}` : msg("Rien pour le moment")}</div>`}${status === 'todo' ? `<button type="button" class="column-add" data-create-status="${status}" aria-label="${markupText("Ajouter une tâche : {0}", name)}" title="${markupText("Ajouter une tâche : {0}", name)}">＋</button>` : ''}</div>`;
    }).join('');
    document.querySelectorAll('.card').forEach(el => { el.onclick = () => action(() => openCard(el.dataset.id)); el.ondragstart = e => e.dataTransfer.setData('text/plain', el.dataset.id); });
    document.querySelectorAll('.card-actions').forEach(menu => {
      menu.onclick = event => event.stopPropagation();
      menu.ondragstart = event => { event.preventDefault(); event.stopPropagation(); };
    });
    document.querySelectorAll('.card-action-menu').forEach(menu => menu.ontoggle = event => {
      if (event.newState !== 'open') return;
      const bounds = menu.previousElementSibling.getBoundingClientRect();
      menu.style.top = Math.max(8, Math.min(bounds.bottom + 4, window.innerHeight - menu.offsetHeight - 8)) + 'px';
      menu.style.right = Math.max(8, window.innerWidth - bounds.right) + 'px';
    });
    document.querySelectorAll('[data-archive-card]').forEach(button => button.onclick = () => action(async () => {
      button.disabled = true;
      try {
        await api(cardPath(button.dataset.archiveCard) + '/archive', 'POST', { archived: true });
        await refresh(); toast(msg("Tâche archivée. Retrouvez-la dans Archives."));
      } finally { button.disabled = false; }
    }));
    document.querySelectorAll('[data-card-stop-agent]').forEach(button => button.onclick = () => action(async () => {
      button.disabled = true;
      try { await api('/api/runs/' + encodeURIComponent(button.dataset.cardStopAgent) + '/cancel', 'POST', {}); await refresh(); toast(msg("Arrêt demandé…")); }
      finally { button.disabled = false; }
    }));
    document.querySelectorAll('[data-delete-card]').forEach(button => button.onclick = () => action(async () => {
      if (!(await confirmAction(msg("Supprimer cette carte uniquement dans Atelier ? Le ticket Jira et les autres services seront conservés."), msg("Supprimer la carte ?"), msg("Supprimer localement")))) return;
      button.disabled = true;
      try { await api(cardPath(button.dataset.deleteCard), 'DELETE', {}); await refresh(); toast(msg("Carte supprimée localement.")); }
      finally { button.disabled = false; }
    }));
    document.querySelectorAll('[data-card-project]').forEach(select => {
      select.onclick = e => e.stopPropagation();
      select.ondragstart = e => { e.preventDefault(); e.stopPropagation(); };
      select.onchange = () => action(async () => {
        select.disabled = true;
        try { await api(cardPath(select.dataset.cardProject), 'PATCH', { repo: select.value }); await refresh(); toast(msg("Projet enregistré.")); }
        finally { boardSignature = ''; rerender(); }
      });
    });
    document.querySelectorAll('[data-create-status]').forEach(button => button.onclick = () => { $('#create-button').click(); $('#new-form').dataset.status = button.dataset.createStatus; });
    document.querySelectorAll('.column').forEach(el => {
      el.ondragover = e => { e.preventDefault(); el.classList.add('drag-over'); }; el.ondragleave = () => el.classList.remove('drag-over');
      el.ondrop = e => { e.preventDefault(); el.classList.remove('drag-over'); action(async () => { const result = await api(cardPath(e.dataTransfer.getData('text/plain')), 'PATCH', { status: el.dataset.status }); await refresh(); if (result.ticketSync?.state === 'error') toast(msg("Avancement local enregistré. Jira à vérifier : ") + result.ticketSync.message, true); }); };
    });
  }
  return { render, invalidate() { boardSignature = ''; } };
}
