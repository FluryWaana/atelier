import { msg, markupText } from '../i18n.mjs';
import { esc } from '../view-helpers.mjs';
import { sameRepository } from '../../lib/domain/paths.mjs';
import { effortLevels, agentProviders, agentProvider } from '../agent-options.mjs';

export function createTaskEditor({ document, getState, localStorage, addCriterion }) {
  const $ = selector => document.querySelector(selector);
function agentFields(agent, options = {}, prefix = '', inherited = {}, listId = prefix + 'Model-choices') {
  const modelName = prefix ? prefix + 'Model' : 'model', effortName = prefix ? prefix + 'Effort' : 'effort';
  const type = agentProvider(getState().settings, agent).type;
  const levels = effortLevels[type];
  const models = type === 'command' ? [] : type === 'claude' ? ['sonnet', 'opus', 'haiku'] : ['gpt-5.4', 'gpt-5.3-codex', 'gpt-5.1-codex-mini'];
  return `<div class="form-row"><label>${markupText("Modèle")}<input name="${modelName}" list="${listId}" maxlength="180" value="${esc(options.model || '')}" placeholder="${esc(inherited.model || msg("Configuration CLI"))}"><datalist id="${listId}">${models.map(model => `<option value="${model}"></option>`).join('')}</datalist></label><label>${markupText("Niveau de réflexion")}${type === 'command' ? `<input name="${effortName}" maxlength="40" value="${esc(options.effort || '')}" placeholder="${esc(inherited.effort || '')}">` : `<select name="${effortName}"><option value="">${esc(inherited.effort ? msg("Réglage général : ") + levels[inherited.effort] : msg("Automatique (CLI)"))}</option>${Object.entries(levels).map(([value, label]) => `<option value="${value}" ${options.effort === value ? 'selected' : ''}>${label}</option>`).join('')}</select>`}</label></div>`;
}
function taskAgentFields(form, options = {}) {
  const agent = form.elements.agent.value;
  form.querySelector('[data-agent-fields]').innerHTML = `<p class="section-kicker">${markupText("Modèle & réflexion")}</p>` + agentFields(agent, options, '', agentProvider(getState().settings, agent), form.id + '-models');
  form.elements.agent.onchange = () => taskAgentFields(form);
}
function clearDraft(key) { try { localStorage.removeItem('atelier-draft:' + key); } catch {} }
function attachDraft(form, key, host) {
  host.querySelector('.draft-notice')?.remove();
  const notice = document.createElement('div'); notice.className = 'draft-notice';
  (host.querySelector('.task-heading') || host.querySelector('.dialog-heading')).after(notice);
  const snapshot = () => ({ fields: Object.fromEntries([...form.elements].filter(e => e.name && e.name !== 'status' && e.type !== 'password').map(e => [e.name, e.value])), ...(key !== 'new' ? { blockedBy: [...($('#blocked-by')?.selectedOptions || [])].map(o => o.value), checklist: [...$('#criteria-list').children].map(row => ({ text: row.querySelector('input[type=text]').value, done: row.querySelector('input[type=checkbox]').checked })) } : {}) });
  let saved; try { saved = JSON.parse(localStorage.getItem('atelier-draft:' + key) || 'null'); } catch { clearDraft(key); }
  if (saved?.fields && typeof saved.fields === 'object') {
    notice.innerHTML = `<span>${markupText("Brouillon non enregistré disponible.")}</span><button type="button" data-restore-draft>${markupText("Reprendre")}</button><button type="button" data-discard-draft>${markupText("Oublier")}</button>`;
    notice.querySelector('[data-discard-draft]').onclick = () => { clearDraft(key); notice.textContent = msg("Brouillon supprimé."); };
    notice.querySelector('[data-restore-draft]').onclick = () => {
      if (agentProviders(getState().settings).some(provider => provider.id === saved.fields.agent)) { form.elements.agent.value = saved.fields.agent; taskAgentFields(form); }
      for (const [name,value] of Object.entries(saved.fields)) { const field = form.elements.namedItem(name); if (field && name !== 'status' && !field.readOnly && !field.disabled && typeof value === 'string') field.value = value; }
      if (key !== 'new' && Array.isArray(saved.blockedBy) && $('#blocked-by')) for (const option of $('#blocked-by').options) option.selected = saved.blockedBy.includes(option.value);
      if (key !== 'new' && Array.isArray(saved.checklist)) { $('#criteria-list').replaceChildren(); saved.checklist.slice(0,20).filter(i => typeof i.text === 'string' && typeof i.done === 'boolean').forEach(i => addCriterion(i, false)); }
      notice.textContent = msg("Brouillon repris. Enregistrez la carte pour appliquer les modifications.");
    };
  } else notice.textContent = msg("Vos saisies sont conservées en brouillon dans ce navigateur.");
  host.oninput = event => {
    if (event.target !== form && event.target.form !== form) return;
    try { localStorage.setItem('atelier-draft:' + key, JSON.stringify(snapshot())); notice.textContent = msg("Brouillon conservé dans ce navigateur. Modifications non enregistrées."); }
    catch { notice.textContent = msg("Brouillon indisponible : enregistrez avant de fermer."); }
  };

}


function bind() {
$('#create-button').onclick = () => { $('#new-form').reset(); $('#new-form h2').textContent = msg("Nouvelle tâche"); $('#new-dialog').setAttribute('aria-label', msg("Nouvelle tâche")); $('#new-form button.primary').textContent = msg("Créer la tâche"); $('#new-form .notice-line').textContent = msg("Créer la tâche ne lance pas l’agent. Vous pourrez relire ses instructions avant de démarrer."); $('#template-criteria').textContent = '';  $('#new-form').dataset.status = 'todo'; $('#new-form').elements.repo.value = getState().projects.find(p => p.id === $('#project-filter').value)?.repo || (getState().projects.length === 1 ? getState().projects[0].repo : '') || getState().settings.repo || ''; $('#new-project').value = $('#new-form').elements.repo.value; $('#new-form').elements.repo.closest('details').open = !$('#new-form').elements.repo.value && !getState().projects.length; document.querySelectorAll('[data-template]').forEach(b => b.setAttribute('aria-pressed', 'false')); $('#new-form').elements.agent.value = getState().projects.find(p => sameRepository(p.repo, $('#new-form').elements.repo.value, getState().settings.platform))?.agent || getState().settings.agent || 'codex'; attachDraft($('#new-form'), 'new', $('#new-dialog')); $('#new-dialog').showModal(); $('#new-form').elements.title.focus(); };
$('#new-project').onchange = e => { if (e.target.value) { $('#new-form').elements.repo.value = e.target.value; $('#new-form').elements.agent.value = getState().projects.find(p => p.repo === e.target.value)?.agent || getState().settings.agent || 'codex'; taskAgentFields($('#new-form')); $('#new-form').elements.repo.closest('details').open = false; } };

}

  return { bind, agentFields, taskAgentFields, attachDraft, clearDraft };
}
