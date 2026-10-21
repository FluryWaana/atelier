import { esc } from '../view-helpers.mjs';

export function createSettingsView({ document, api, action, toast, refresh }) {
  const host = document.querySelector('#connector-settings');
  let catalog, activeTab = 'general';
  function selectTab(id) {
    if (![...document.querySelectorAll('[data-settings-panel]')].some(panel => panel.dataset.settingsPanel === id)) id = 'providers';
    activeTab = id;
    document.querySelectorAll('[data-settings-panel]').forEach(panel => panel.hidden = panel.dataset.settingsPanel !== id);
    document.querySelectorAll('[data-settings-tab]').forEach(button => {
      button.setAttribute('aria-pressed', String(button.dataset.settingsTab === id));
      button.classList.toggle('selected-tab', button.dataset.settingsTab === id);
    });
  }
  function fieldMarkup(field, module) {
    const value = module.values[field.id], name = esc(field.id);
    let control;
    if (field.type === 'select') control = `<select name="${name}">${field.options.map(option => `<option value="${esc(option.value)}" ${value === option.value ? 'selected' : ''}>${esc(option.label)}</option>`).join('')}</select>`;
    else if (field.type === 'textarea') control = `<textarea name="${name}" rows="3" maxlength="4000">${esc(value || '')}</textarea>`;
    else control = `<input name="${name}" type="${field.type}" ${field.type === 'checkbox' ? (value ? 'checked' : '') : `maxlength="4000" value="${field.type === 'password' ? '' : esc(value || '')}"`} ${field.type === 'password' ? 'autocomplete="new-password"' : ''}>`;
    return `<label ${field.type === 'checkbox' ? 'class="check"' : ''}>${esc(field.label)}${control}</label>${field.type === 'password' ? `<p class="hint">${module.secrets[field.id] ? 'Secret enregistré. Laisser vide pour le conserver.' : 'Aucun secret enregistré.'}</p><button type="button" class="danger" data-forget-secret="${name}" ${module.secrets[field.id] ? '' : 'disabled'}>Oublier ce secret</button>` : ''}`;
  }
  function render() {
    const labels = { tickets: 'Tickets', repositories: 'Dépôts Git', time: 'Suivi du temps' };
    const repositoryProviders = [...(catalog.projectRepositoryProviders || []), ...(catalog.pullRequestProviders || [])];
    const activeModules = catalog.modules.filter(module => !module.capabilities.length || Object.values(catalog.providers).includes(module.id) || repositoryProviders.includes(module.id));
    const availableCapabilities = Object.entries(labels).filter(([key]) => catalog.modules.some(module => module.capabilities.includes(key)));
    const moduleLabel = module => Object.entries(labels).filter(([key]) => catalog.providers[key] === module.id || key === 'repositories' && repositoryProviders.includes(module.id)).map(([, label]) => label).join(' · ') || module.name;
    document.querySelector('#settings-navigation').innerHTML = `<button type="button" class="view-tab" data-settings-tab="general">Général</button><button type="button" class="view-tab" data-settings-tab="providers">Modules</button>` + activeModules.map(module => `<button type="button" class="view-tab" data-settings-tab="${esc(module.id)}">${esc(moduleLabel(module))}${module.capabilities.includes('repositories') && activeModules.filter(item => item.capabilities.includes('repositories')).length > 1 ? ' · ' + esc(module.name) : ''}</button>${module.tabs.map(tab => `<button type="button" class="view-tab" data-settings-tab="${esc(module.id + ':' + tab.id)}">${esc(tab.label)}</button>`).join('')}`).join('');
    host.innerHTML = `<section data-settings-panel="providers"><h2>Modules et fournisseurs</h2><p class="hint">Choisissez le module utilisé pour chaque fonction. Les réglages des fournisseurs utilisés par les projets ou les PR existantes restent accessibles.</p>${(catalog.errors || []).length ? `<div role="alert"><h3>Modules non chargés</h3><ul>${catalog.errors.map(error => `<li>${esc(error.id)} : ${esc(error.message)}</li>`).join('')}</ul><p>Corrigez ou retirez ces modules, puis redémarrez Atelier.</p></div>` : ''}<form id="connector-providers">${availableCapabilities.map(([key, label]) => `<label>${label}<select name="${key}"><option value="" ${!catalog.providers[key] ? 'selected' : ''}>Aucun module actif</option>${catalog.modules.filter(module => module.capabilities.includes(key)).map(module => `<option value="${esc(module.id)}" ${catalog.providers[key] === module.id ? 'selected' : ''}>${esc(module.name)}</option>`).join('')}</select></label>`).join('')}${availableCapabilities.length ? '<div class="form-actions"><button class="primary">Enregistrer les fournisseurs</button></div>' : '<p class="hint">Aucun fournisseur installé.</p>'}</form></section>` + activeModules.map(module => `<section data-settings-panel="${esc(module.id)}"><h2>${esc(moduleLabel(module))} · ${esc(module.name)}</h2><p class="hint">${esc(module.description)}</p><form data-connector-form="${esc(module.id)}">${module.fields.map(field => fieldMarkup(field, module)).join('')}<div class="form-actions"><button class="primary">Enregistrer ${esc(module.name)}</button></div></form></section>${module.tabs.map(tab => `<section data-settings-panel="${esc(module.id + ':' + tab.id)}"><h2>${esc(tab.label)}</h2><p>${esc(tab.description || '')}</p>${(tab.actions || []).map(item => `<button type="button" data-module="${esc(module.id)}" data-connector-action="${esc(item.id)}">${esc(item.label)}</button>`).join('')}<pre class="connector-result" role="status"></pre></section>`).join('')}`).join('');
    document.querySelectorAll('[data-settings-tab]').forEach(button => button.onclick = () => selectTab(button.dataset.settingsTab));
    host.querySelector('#connector-providers').onsubmit = event => {
      event.preventDefault(); action(async () => { catalog = await api('/api/connectors/providers', 'PUT', Object.fromEntries(new FormData(event.target))); await refresh(); render(); toast('Fournisseurs enregistrés.'); });
    };
    host.querySelectorAll('[data-connector-form]').forEach(form => {
      form.onsubmit = event => { event.preventDefault(); action(async () => {
        const button = form.querySelector('.primary'); button.disabled = true;
        try {
          const body = Object.fromEntries(new FormData(form));
          form.querySelectorAll('input[type=checkbox]').forEach(input => body[input.name] = input.checked);
          catalog = await api('/api/connectors/' + form.dataset.connectorForm + '/settings', 'PUT', body);
          form.querySelectorAll('input[type=password]').forEach(input => input.value = '');
          await refresh(); render(); toast('Connexion enregistrée.');
        } finally { button.disabled = false; }
      }); };
      form.querySelectorAll('[data-forget-secret]').forEach(button => button.onclick = () => action(async () => {
        catalog = await api('/api/connectors/' + form.dataset.connectorForm + '/secrets/' + button.dataset.forgetSecret, 'DELETE', {});
        await refresh(); render(); toast('Secret oublié.');
      }));
    });
    host.querySelectorAll('[data-connector-action]').forEach(button => button.onclick = () => action(async () => {
      button.disabled = true;
      try {
        const result = await api('/api/connectors/' + button.dataset.module + '/actions/' + button.dataset.connectorAction, 'POST', {});
        button.parentElement.querySelector('.connector-result').textContent = typeof result === 'string' ? result : JSON.stringify(result, null, 2);
        await refresh();
      } finally { button.disabled = false; }
    }));
    selectTab(activeTab);
  }
  return { async open(section = 'general') {
    catalog = await api('/api/connectors');
    activeTab = ['general', 'providers', ...catalog.modules.map(module => module.id)].includes(section) ? section : 'general';
    render();
  } };
}
