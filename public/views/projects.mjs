import { msg, markupText } from '../i18n.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { esc } from '../view-helpers.mjs';

export function createProjectsView({ document, location, api, action, confirmAction, refresh, toast, onSelect, openProjectSettings }) {
  const $ = selector => document.querySelector(selector);
  let projectsSignature = '';
  function render(state) {
    const signature = JSON.stringify([state.projects, state.cards.map(c => [c.id, c.repo, c.archived])]);
    if (signature === projectsSignature) return;
    projectsSignature = signature;
    const filter = $('#project-filter'), previous = filter.value || new URLSearchParams(location.hash.slice(1)).get('project');
    filter.innerHTML = `<option value="">${markupText("Tous les projets")}</option>` + state.projects.map(p => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join('');
    filter.value = state.projects.some(p => p.id === previous) ? previous : '';
    const choice = $('#new-project'), selectedRepo = choice.value;
    choice.innerHTML = `<option value="">${markupText("Choisir un projet…")}</option>` + state.projects.map(p => `<option value="${esc(p.repo)}">${esc(p.name)}</option>`).join('');
    choice.value = selectedRepo;
    $('#project-paths').innerHTML = state.projects.map(p => `<option value="${esc(p.repo)}">${esc(p.name)}</option>`).join('');
    $('#project-list').innerHTML = state.projects.map(p => `<div class="project-row"><button type="button" data-project="${esc(p.id)}"><strong>${esc(p.name)}</strong><small>${esc(p.repo)}</small></button><div class="project-row-actions"><button type="button" data-project-settings="${esc(p.id)}">${markupText("Réglages")}</button><button type="button" data-remove-project="${esc(p.id)}" aria-label="${markupText("Retirer {0} de la liste", esc(p.name))}">${markupText("Retirer")}</button></div></div>`).join('') || `<p class="hint">${markupText("Aucun projet enregistré.")}</p>`;
    const orphanRepos = [...new Set(state.cards.filter(c => c.repo && !c.archived && !state.projects.some(p => sameRepository(p.repo, c.repo, state.settings?.platform))).map(c => c.repo))];
    if (orphanRepos.length) $('#project-list').innerHTML += `<h3>${markupText("Tâches sans projet enregistré")}</h3><p class="hint">${markupText("Ces tâches restent visibles dans le tableau. Vous pouvez les archiver sans supprimer leur historique.")}</p>` + orphanRepos.map(repo => `<div class="project-row"><div style="flex:1;min-width:0"><small>${esc(repo)}</small></div><button type="button" data-archive-repo="${esc(repo)}">${markupText("Archiver les tâches")}</button></div>`).join('');
    document.querySelectorAll('[data-archive-repo]').forEach(button => button.onclick = () => action(async () => {
      if (!(await confirmAction(msg("Archiver les tâches de ce répertoire ? Elles restent restaurables dans Archives. Aucun fichier ni ticket Jira ne sera supprimé."), msg("Archiver les tâches ?"), msg("Archiver les tâches")))) return;
      await api('/api/projects/archive-tasks', 'POST', { repo: button.dataset.archiveRepo }); await refresh(); toast(msg("Tâches déplacées dans Archives."));
    }));
    document.querySelectorAll('[data-project-settings]').forEach(button => button.onclick = () => openProjectSettings(button.dataset.projectSettings));
    document.querySelectorAll('[data-project]').forEach(button => button.onclick = () => { onSelect(button.dataset.project, true); });
    document.querySelectorAll('[data-remove-project]').forEach(button => button.onclick = () => action(async () => { if (!(await confirmAction(msg("Retirer ce projet et archiver ses tâches ? Elles quitteront le tableau et resteront restaurables dans Archives. Aucun fichier ni ticket Jira ne sera supprimé."), msg("Retirer le projet ?"), msg("Retirer et archiver")))) return; await api('/api/projects/' + encodeURIComponent(button.dataset.removeProject), 'DELETE', { archiveTasks: true }); await refresh(); toast(msg("Projet retiré. Ses tâches sont dans Archives.")); }));
    document.querySelectorAll('input[name="repo"]').forEach(input => input.setAttribute('list', 'project-paths'));
  }
  $('#project-form').onsubmit = e => { e.preventDefault(); action(async () => { const button = e.target.querySelector('button.primary'); button.disabled = true; try { const project = await api('/api/projects', 'POST', Object.fromEntries(new FormData(e.target))); e.target.reset(); await refresh(); onSelect(project.id); toast(msg("Projet prêt. Vous pouvez créer sa première tâche.")); } finally { button.disabled = false; } }); };
  $('#browse-project').onclick = () => action(async () => {
    const button = $('#browse-project');
    button.disabled = true; button.textContent = msg("Sélection du dossier…");
    try {
      const result = await api('/api/folders/choose', 'POST', {});
      if (result.path) { $('#project-form').elements.repo.value = result.path; if (!$('#project-form').elements.name.value.trim()) $('#project-form').elements.name.value = result.path.split(/[\\/]/).filter(Boolean).at(-1) || msg("Mon projet"); }
    } finally { button.disabled = false; button.textContent = msg("Parcourir…"); }
  });
  return { render, invalidate() { projectsSignature = ''; } };
}
