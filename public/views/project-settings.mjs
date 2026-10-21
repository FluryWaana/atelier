import { names, esc } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';

export function createProjectSettings({ document, getState, api, action, refresh, toast, devRunning }) {
  const $ = selector => document.querySelector(selector);
  let editingProject = null;
$('#browse-project-settings').onclick = () => action(async () => {
  const button = $('#browse-project-settings'), form = $('#project-settings-form');
  button.disabled = true;
  try {
    const result = await api('/api/folders/choose', 'POST', {});
    if (result.path) form.elements.repo.value = result.path;
  } finally { button.disabled = form.elements.repo.readOnly; }
});
function launchSettingsFields() {
  return '<p class="hint">Le dépôt principal peut être le Front ou le Back. Pour l’autre application, choisissez son dépôt : Atelier créera un worktree sur la même branche au lancement.</p><button type="button" class="btn btn-primary" data-detect-launch>Détecter la configuration du projet</button><div data-launch-detection-results aria-live="polite"></div><div class="row g-3 launch-settings-grid">' + [['front', 'Front', 'front', 'npm run dev -- --port {port}'], ['back', 'Back', 'back', 'dotnet run --no-launch-profile -- --urls http://127.0.0.1:{port}']].map(([key, label, directory, command]) => `<div class="col-12 col-md-6"><fieldset class="application-launch-panel"><legend>${label}</legend><label class="form-label">Dépôt Git du ${label}<span class="input-group"><input class="form-control" name="${key}Repo" list="project-paths" maxlength="2000" placeholder="Vide = dépôt principal"><button type="button" class="btn" data-browse-service-repo="${key}" aria-label="Choisir le dépôt du ${label}">Parcourir…</button></span></label><p class="form-text">Autre dépôt : un worktree distinct, avec le même nom de branche.</p><button type="button" class="btn" data-detect-role="${key}">Détecter le ${label}</button><div data-role-detection-results="${key}" aria-live="polite"></div><label class="form-label">Sous-dossier de l’application<input class="form-control" name="${key}Directory" maxlength="2000" placeholder="${directory} · vide = racine du worktree"></label><label class="form-label">Commande de démarrage<textarea class="form-control" name="${key}Command" maxlength="2000" rows="3" placeholder="${command}"></textarea></label><label class="form-label">Port fourni par Atelier<input class="form-control" name="${key}Port" type="number" min="1" max="65535" placeholder="Automatique : port libre"></label></fieldset></div>`).join('') + '</div><p class="hint">Une commande vide désactive le service. Atelier remplace {port} par un port libre, ou par le port saisi. L’URL du back et le CORS restent à configurer dans le projet. Conservez les serveurs au premier plan. Ne renseignez pas de secrets.</p>';
}
function bindLaunchSettings(form, projectId) {
  form.querySelector('[data-detect-launch]').onclick = () => action(() => detectLaunchSettings(form, projectId));
  form.querySelectorAll('[data-detect-role]').forEach(button => button.onclick = () => action(() => detectLaunchSettings(form, projectId, button.dataset.detectRole)));
  form.querySelectorAll('[data-browse-service-repo]').forEach(button => button.onclick = () => action(async () => {
    button.disabled = true;
    try {
      const result = await api('/api/folders/choose', 'POST', {});
      if (result.path && form.dataset.launchProjectId === projectId) form.elements[button.dataset.browseServiceRepo + 'Repo'].value = result.path;
    } finally { button.disabled = false; }
  }));
}
async function detectLaunchSettings(form, projectId, role = '') {
  const sources = { frontRepo: form.elements.frontRepo?.value || '', backRepo: form.elements.backRepo?.value || '' };
  if (form.elements.repo) sources.repo = form.elements.repo.value;
  if (role) sources.role = role;
  const button = form.querySelector(role ? `[data-detect-role="${role}"]` : '[data-detect-launch]'), results = form.querySelector(role ? `[data-role-detection-results="${role}"]` : '[data-launch-detection-results]');
  button.disabled = true;
  try {
    const detected = await api('/api/projects/' + encodeURIComponent(projectId) + '/detect-launch', 'POST', sources);
    if (form.dataset.launchProjectId !== projectId || form.querySelector(role ? `[data-detect-role="${role}"]` : '[data-detect-launch]') !== button || !form.closest('dialog').open || sources.frontRepo !== (form.elements.frontRepo?.value || '') || sources.backRepo !== (form.elements.backRepo?.value || '') || sources.repo !== form.elements.repo?.value) return;
    results.innerHTML = (detected.candidates.length ? '<p class="hint">Choisissez l’application et, pour .NET, son profil. Les champs vides sont préremplis si le choix est unique.</p>' + (role ? [role] : ['front', 'back']).map(role => `<label class="form-label">Application ${role === 'front' ? 'Front' : 'Back'}<select class="form-select" data-detected-role="${role}"><option value="">Choisir une application détectée</option>${detected.candidates.flatMap((candidate, index) => candidate.role === role ? [`<option value="${index}" ${candidate.runnable === false ? 'disabled' : ''}>${esc(candidate.technology)} · ${esc(candidate.file)}${candidate.runnable === false ? ' · non exécutable' : candidate.automaticPort ? ' · port automatique' : ' · port à configurer manuellement'}</option>`] : []).join('')}</select></label><div data-detected-profiles="${role}"></div>`).join('') : '<p class="hint">Aucune application reconnue. Vous pouvez renseigner les commandes manuellement.</p>') + detected.warnings.map(warning => `<p class="hint">${esc(warning)}</p>`).join('');
    const apply = (role, candidate) => {
      form.elements[role + 'Directory'].value = candidate.directory;
      form.elements[role + 'Command'].value = candidate.command;
      const host = results.querySelector(`[data-detected-profiles="${role}"]`);
      host.innerHTML = candidate.profiles?.length ? `<label>Profil .NET<select data-launch-profile><option value="">Sans profil</option>${candidate.profiles.map(profile => `<option value="${esc(profile.name)}" ${profile.supported ? '' : 'disabled'}>${esc(profile.name)} · ${esc(profile.commandName)}${profile.supported ? '' : ' (non pris en charge par dotnet run)'}</option>`).join('')}</select></label><p class="hint">URL déclarées avant surcharge : ${esc(candidate.profiles.flatMap(profile => profile.urls).join(' · ') || 'aucune')}. Pour ASP.NET Core, Atelier remplace les URL d’écoute par une URL HTTP locale avec le port choisi. Les variables du profil restent chargées par dotnet.</p>` : '';
      if (candidate.profiles?.length) {
        const profiles = host.querySelector('[data-launch-profile]');
        profiles.value = candidate.defaultProfile;
        profiles.onchange = () => { form.elements[role + 'Command'].value = detectedProfileCommand(candidate, profiles.value); };
      }
    };
    results.querySelectorAll('[data-detected-role]').forEach(select => {
      const role = select.dataset.detectedRole, matches = detected.candidates.filter(candidate => candidate.role === role && candidate.command);
      if (matches.length === 1 && !form.elements[role + 'Command'].value && !form.elements[role + 'Directory'].value) { apply(role, matches[0]); select.value = String(detected.candidates.indexOf(matches[0])); }
      select.onchange = () => { const candidate = detected.candidates[Number(select.value)]; if (select.value !== '' && candidate?.role === role && candidate.runnable !== false) apply(role, candidate); };
    });
  } finally { button.disabled = false; }
}
function detectedProfileCommand(candidate, profileName) {
  if (profileName && !candidate.profiles.some(profile => profile.name === profileName && profile.supported)) throw new Error('Profil .NET non pris en charge.');
  return candidate.baseCommand + (profileName ? ` --launch-profile "${profileName}"` : ' --no-launch-profile') + (candidate.automaticPort ? ' -- --urls http://127.0.0.1:{port}' : '');
}
let editingLaunchProject = null;
function openLaunchSettings(id) {
  const state = getState();
  const project = state.projects.find(project => project.id === id);
  if (!project) return;
  editingLaunchProject = id;
  $('#launch-settings-title').textContent = project.name;
  $('#launch-settings-fields').innerHTML = launchSettingsFields();
  const form = $('#launch-settings-form');
  form.dataset.launchProjectId = id;
  for (const name of ['frontCommand', 'backCommand', 'frontRepo', 'backRepo', 'frontDirectory', 'backDirectory', 'frontPort', 'backPort']) form.elements[name].value = project[name] || '';
  bindLaunchSettings(form, id);
  $('#launch-settings-status').textContent = (state.devServers || []).some(session => session.projectIds.includes(id) && devRunning(session.cardId)) ? 'Des services tournent : arrêtez puis relancez l’ensemble pour appliquer les modifications.' : 'Ces réglages s’appliquent à tous les worktrees de ce projet.';
  $('#launch-settings-dialog').showModal();
}
$('#launch-settings-form').onsubmit = event => { event.preventDefault(); action(async () => {
  const form = event.target, button = form.querySelector('button.primary');
  button.disabled = true;
  try {
    await api('/api/projects/' + encodeURIComponent(editingLaunchProject), 'PATCH', Object.fromEntries(new FormData(form)));
    await refresh(); $('#launch-settings-dialog').close(); toast('Lancement enregistré.');
  } finally { button.disabled = false; }
}); };
function openProjectSettings(id) {
  const state = getState();
  const jiraOptions = $("#project-settings-form").elements.jiraMode?.closest?.(".col-12");
  if (jiraOptions) jiraOptions.hidden = state.connectors?.providers?.tickets !== "jira" || !state.connectors?.modules?.some(module => module.id === "jira");
  editingProject = id;
  const project = state.projects.find(p => p.id === id), form = $('#project-settings-form');
  if (!form.elements.frontCommand) {
    const commands = document.createElement('div'); commands.className = 'form-section runtime-settings';
    commands.innerHTML = '<h3>Lancement des applications</h3>' + launchSettingsFields() + '<details><summary>Lancement lié depuis les cartes</summary><label>Projet lié<select name="linkedProjectId"></select></label><p class="hint">Depuis une carte, le projet lié démarre dans son dépôt local. Le lanceur Processus utilise les worktrees choisis et ignore ce lien.</p></details>';
    form.querySelector('.form-actions').before(commands);
  }
  if (!form.elements.automationEnabled) {
    const automation = document.createElement('section'); automation.className = 'form-section';
    automation.innerHTML = '<h3>Développement automatique</h3><label class="check"><input type="checkbox" name="automationEnabled"> Activer le parcours automatique</label><p class="hint">En cours déclenche : développement, vérifications, commit, push, PR brouillon et review. Tous les fichiers non ignorés du worktree sont publiés. La fusion et la validation des critères restent manuelles.</p><label>Branche cible de la PR<input name="prTarget" maxlength="180" placeholder="main ou develop"></label><label>Commande de vérification<textarea name="testCommand" maxlength="2000" rows="2" placeholder="npm test &amp;&amp; npm run build"></textarea></label><p class="hint">Exécutée dans le worktree avec le shell du système. Elle doit se terminer avec un code non nul en cas d’échec. Limite : 10 minutes. Les fichiers doivent rester stables pendant les tests. Les commandes peuvent modifier les dépendances partagées.</p><label class="check"><input type="checkbox" name="autoFix"> Corriger automatiquement les P0/P1 (deux cycles maximum)</label>';
    form.querySelector('.form-actions').before(automation);
  }
  $('#project-settings-title').textContent = project.name;
  form.elements.repo.value = project.repo;
  const hasWorktrees = state.cards.some(card => card.worktree && sameRepository(card.repo, project.repo, state.settings?.platform));
  form.elements.repo.readOnly = hasWorktrees;
  $('#browse-project-settings').disabled = hasWorktrees;
  $('#project-location-help').textContent = hasWorktrees ? 'Retirez les worktrees de ce projet avant de modifier son emplacement.' : 'Choisissez un dossier existant. Aucun fichier ne sera déplacé ; les cartes du projet suivront le nouveau chemin.';
  form.dataset.launchProjectId = id;
  form.querySelector('[data-launch-detection-results]').innerHTML = '';
  const detectButton = form.querySelector('[data-detect-launch]');
  detectButton.disabled = false;
  form.querySelectorAll('[data-role-detection-results]').forEach(results => { results.innerHTML = ''; });
  bindLaunchSettings(form, id);
  form.elements.agent.value = project.agent || ''; form.elements.baseBranch.value = project.baseBranch || ''; form.elements.jiraMode.value = project.jiraMode || 'inherit';
  const providers = state.connectors?.modules.filter(module => module.capabilities.includes('repositories')) || [];
  const general = providers.find(module => module.id === state.connectors?.providers?.repositories)?.name || 'aucun module actif';
  form.elements.repositoryConnector.innerHTML = `<option value="">Hériter du fournisseur général (${esc(general)})</option>` + providers.map(module => `<option value="${esc(module.id)}">${esc(module.name)}</option>`).join('') + (project.repositoryConnector && !providers.some(module => module.id === project.repositoryConnector) ? `<option value="${esc(project.repositoryConnector)}" disabled>Module indisponible : ${esc(project.repositoryConnector)}</option>` : '');
  form.elements.repositoryConnector.value = project.repositoryConnector || '';
  $('#project-repository-connector').hidden = !providers.length && !project.repositoryConnector;
  for (const name of ['frontCommand', 'backCommand', 'frontRepo', 'backRepo', 'frontDirectory', 'backDirectory', 'frontPort', 'backPort']) form.elements[name].value = project[name] || '';
  for (const name of ['prTarget', 'testCommand']) form.elements[name].value = project[name] || '';
  form.elements.automationEnabled.checked = Boolean(project.automationEnabled);
  form.elements.autoFix.checked = project.autoFix ?? true;
  form.elements.linkedProjectId.innerHTML = '<option value="">Aucun projet lié</option>' + (project.linkedProjectId && !state.projects.some(item => item.id === project.linkedProjectId) ? `<option value="${esc(project.linkedProjectId)}">Projet retiré — choisir un autre projet</option>` : '') + state.projects.filter(item => item.id !== id).map(item => `<option value="${esc(item.id)}">${esc(item.name)}</option>`).join('');
  form.elements.linkedProjectId.value = project.linkedProjectId || '';
  $('#project-jira-map').innerHTML = Object.entries(names).map(([key,label]) => `<label>${label} →<input name="${key}" maxlength="100" value="${esc(project.jiraStatusMap?.[key] || '')}" placeholder="Nom exact ou ID du statut Jira"></label>`).join('');
  $('#project-jira-map').hidden = form.elements.jiraMode.value !== 'custom';
  form.elements.jiraMode.onchange = () => $('#project-jira-map').hidden = form.elements.jiraMode.value !== 'custom';
  $('#project-settings-dialog').showModal();
}
$('#project-settings-form').onsubmit = event => { event.preventDefault(); action(async () => {
  const form = event.target, body = Object.fromEntries(new FormData(form));
  for (const key of ['automationEnabled', 'autoFix']) body[key] = form.elements[key].checked;
  body.jiraStatusMap = Object.fromEntries(Object.keys(names).map(key => [key, form.elements[key].value]));
  await api('/api/projects/' + encodeURIComponent(editingProject), 'PATCH', body); await refresh(); $('#project-settings-dialog').close(); toast('Réglages du projet enregistrés.');
}); };



  return { openProjectSettings, openLaunchSettings };
}
