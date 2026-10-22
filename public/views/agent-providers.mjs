import { msg, markupText, locale } from '../i18n.mjs';
import { esc } from '../view-helpers.mjs';
import { effortLevels, agentBadge, agentConnection } from '../agent-options.mjs';

const dateText = value => value && Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString(locale, { dateStyle: 'short', timeStyle: 'short' }) : '';
const providerType = provider => provider.auth === 'api' ? msg('Clé API via Codex CLI') : provider.type === 'command' ? msg('CLI personnalisé') : provider.type === 'claude' ? 'Claude Code CLI' : 'Codex CLI';
const compactVersion = status => status?.version?.match(/\d+\.\d+\.\d+(?:-[\w.-]+)?/)?.[0] || '—';
const badge = status => `<span class="provider-status ${status?.connected ? 'is-connected' : ''}"><span class="online-dot ${status?.connected ? '' : 'offline'}"></span>${esc(agentConnection(status))}</span>`;

export function providerCardMarkup(provider, status = {}) {
  return `<button type="button" class="provider-card" data-open-provider="${esc(provider.id)}"><span class="provider-card-top">${agentBadge(provider.type)}<span class="provider-card-name"><strong>${esc(provider.name)}</strong><small>${esc(providerType(provider))}</small></span><span class="agent-chevron" aria-hidden="true">↗</span></span>${badge(status)}<span class="provider-card-account">${esc(status.account || provider.accountLabel || msg('Compte non communiqué'))}</span><span class="provider-card-footer"><span>${status.subscription ? esc(status.subscription) : markupText('Compte & quotas')}</span><span class="${status.updateState === 'outdated' ? 'provider-update' : ''}">${status.updateState === 'outdated' ? markupText('Mise à jour disponible') : 'v' + esc(compactVersion(status))}</span></span></button>`;
}

const updatePhaseLabel = phase => msg(phase === 'installing' ? 'Installation de la mise à jour…' : phase === 'verifying' ? 'Contrôle de la version installée…' : 'Vérification de la mise à jour…');

function quotaMarkup(quota) {
  const remaining = Number.isFinite(quota.remainingPercent) ? Math.max(0, Math.min(100, quota.remainingPercent)) : null;
  if (remaining === null) return '';
  const expired = quota.resetsAt && Date.parse(quota.resetsAt) <= Date.now();
  const window = quota.windowMinutes ? quota.windowMinutes % 1440 === 0 ? msg('{0} jours', quota.windowMinutes / 1440) : quota.windowMinutes % 60 === 0 ? msg('{0} heures', quota.windowMinutes / 60) : msg('{0} minutes', quota.windowMinutes) : msg(quota.kind === 'secondary' ? 'Fenêtre secondaire' : 'Fenêtre principale');
  return `<div class="provider-quota"><div class="provider-quota-label"><strong>${esc(window)}</strong><span>${esc(quota.name)}</span></div><p class="provider-quota-value">${expired ? '—' : esc(remaining) + '<span>%</span>'}<small>${markupText(expired ? 'À actualiser' : 'restants')}</small></p>${expired ? '' : `<progress class="${remaining <= 20 ? 'quota-low' : ''}" max="100" value="${remaining}" aria-label="${esc(msg('{0} : {1} % restants', window, remaining))}"></progress>`}<p class="hint">${quota.resetsAt ? markupText('Réinitialisation : {0}', esc(dateText(quota.resetsAt))) : markupText('Réinitialisation non communiquée')}</p></div>`;
}

function installationMarkup(provider, status, loading, updating, phase) {
  const updateLink = provider.type === 'claude' ? 'https://code.claude.com/docs/en/setup#update-manually' : provider.type === 'codex' ? 'https://learn.chatgpt.com/docs/cli' : '';
  const updateAction = status.updateState === 'outdated' ? provider.canUpdate ? `<button type="button" class="primary" data-update-provider ${loading || updating ? 'disabled' : ''}>${markupText(updating && phase ? 'Mise à jour en cours…' : 'Mettre à jour le CLI')}</button><small class="hint">${markupText('La mise à jour suit le canal configuré dans le CLI.')}</small>` : `<p class="hint">${markupText(provider.updateReason === 'custom' ? 'La mise à jour des exécutables personnalisés reste manuelle.' : 'Cette installation utilise son propre gestionnaire de mises à jour.')}</p>${updateLink ? `<a href="${updateLink}" target="_blank" rel="noopener noreferrer">${markupText('Instructions de mise à jour')} ↗</a>` : ''}` : '';
  return `<section class="provider-installation"><div><h3>${markupText('Installation')}</h3><p>${markupText('Version installée')} <strong>${esc(compactVersion(status))}</strong></p></div><div><span class="${status.updateState === 'outdated' ? 'provider-update' : 'hint'}">${markupText(status.updateState === 'outdated' ? 'Mise à jour disponible' : status.updateState === 'current' ? 'À jour' : 'Vérification de mise à jour indisponible')}</span>${status.latestVersion ? `<small>${markupText('Dernière version stable : {0}', esc(status.latestVersion))}</small>` : ''}</div></section>${updateAction ? `<div class="provider-update-actions" role="status">${updateAction}${updating && phase ? `<div class="provider-update-progress" role="status" aria-live="polite"><progress aria-label="${esc(msg('Mise à jour du CLI'))}"></progress><span data-update-phase>${esc(updatePhaseLabel(phase))}</span></div>` : ''}</div>` : ''}`;
}

export function providerDetailMarkup(provider, status = {}, loading = false, updating = false, phase = 'checking') {
  const api = provider.auth === 'api' || status.quotaState === 'api' || /^(api|apikey|api_key|api_key_helper)$/i.test(status.authMethod || '');
  const usageUrl = provider.type === 'codex' ? api ? 'https://platform.openai.com/usage' : 'https://chatgpt.com/codex/settings/usage' : provider.type === 'claude' ? api ? 'https://platform.claude.com/usage' : 'https://claude.ai/settings/usage' : '';
  const billingUrl = provider.type === 'codex' ? api ? 'https://platform.openai.com/settings/organization/billing/overview' : 'https://chatgpt.com/#settings/Account' : provider.type === 'claude' ? api ? 'https://platform.claude.com/settings/billing' : 'https://claude.ai/settings/billing' : '';
  const quotas = status.quotas || [];
  const explanation = status.quotaState === 'rate-limited' ? msg('Anthropic limite les vérifications. Prochaine tentative après le {0}.', dateText(status.retryAt)) : status.quotaState === 'disconnected' && provider.type === 'claude' ? msg('La connexion OAuth locale est absente ou expirée. Reconnectez Claude Code, puis actualisez.') : status.manualCheckRequired ? msg('Lancez une vérification pour lire le compte de cet exécutable personnalisé.') : api ? msg('La facturation API est distincte des abonnements. Consultez les dépenses et limites dans votre espace fournisseur.') : provider.type === 'claude' ? msg('La lecture des quotas Claude est indisponible. Réessayez plus tard ou consultez votre utilisation sur Claude.') : msg('Les quotas ne sont pas disponibles. Vérifiez la connexion et la version du CLI, ou consultez votre espace fournisseur.');
  return `<button type="button" class="provider-back" data-provider-back>← ${markupText('Tous les fournisseurs')}</button><div class="provider-detail-heading">${agentBadge(provider.type)}<div><h2 tabindex="-1" data-provider-title>${esc(provider.name)}</h2><p class="hint">${esc(providerType(provider))}</p></div>${badge(status)}</div><div class="provider-account-grid"><section class="provider-metric"><h3>${markupText('Compte connecté')}</h3><p>${esc(status.account || msg('Compte non communiqué'))}</p><small>${esc(status.authMethod || provider.auth || 'CLI')}</small></section><section class="provider-metric"><h3>${markupText('Abonnement')}</h3><p>${esc(api ? msg('Facturation à l’usage') : status.subscription || msg('Non communiqué'))}</p><small>${provider.accountLabel ? esc(provider.accountLabel) : markupText('Information fournie par le CLI')}</small></section></div><section class="provider-quota-panel" aria-busy="${loading}"><div class="provider-section-heading"><div><h3>${markupText('Capacité restante')}</h3><p class="hint">${markupText('Limites du compte, partagées avec vos autres sessions.')}</p></div><button type="button" data-refresh-account ${loading || updating ? 'disabled' : ''}>${markupText(loading ? 'Actualisation…' : 'Actualiser')}</button></div><div role="status">${loading ? `<p class="provider-empty">${markupText('Lecture du compte et des quotas…')}</p>` : quotas.length ? `<div class="provider-quota-grid">${quotas.map(quotaMarkup).join('')}</div>` : `<div class="provider-empty"><strong>${markupText('Quota non communiqué')}</strong><p>${esc(explanation)}</p></div>`}</div>${(status.quotaCheckedAt || status.checkedAt) ? `<p class="provider-checked hint">${markupText('Dernière vérification : {0}', esc(dateText(status.quotaCheckedAt || status.checkedAt)))}</p>` : ''}${usageUrl ? `<div class="provider-links"><a href="${usageUrl}" target="_blank" rel="noopener noreferrer">${markupText('Voir l’utilisation')} ↗</a><a href="${billingUrl}" target="_blank" rel="noopener noreferrer">${markupText('Gérer l’abonnement')} ↗</a></div>` : ''}</section>${installationMarkup(provider, status, loading, updating, phase)}<div class="provider-detail-actions"><button type="button" data-edit-provider="${esc(provider.id)}" ${loading || updating ? 'disabled' : ''}>${markupText('Configurer')}</button><button type="button" class="danger" data-remove-provider="${esc(provider.id)}" ${updating ? 'disabled' : ''}>${markupText('Retirer')}</button></div>${provider.type === 'command' || provider.command ? `<p class="hint">${markupText('Actualiser exécute les commandes de diagnostic avec les droits de votre compte.')}</p>` : ''}`;
}

export function createAgentProvidersView({ document, api, action, refresh, toast, confirmAction }) {
  let providers = [], statuses = {}, selected = '', updating = '', updatePhase = 'checking';
  const accounts = new Map(), loading = new Set();
  const host = () => document.querySelector('#agent-provider-settings');
  function render() {
    const provider = providers.find(item => item.id === selected);
    host().innerHTML = (provider ? providerDetailMarkup(provider, accounts.get(selected) || statuses[selected], loading.has(selected), Boolean(updating || provider.updating), updating === provider.id ? updatePhase : provider.updatePhase || '') : `<div class="provider-section-heading"><div><h2>${markupText('Fournisseurs IA')}</h2><p class="hint">${markupText('Vos comptes, abonnements et capacités au même endroit.')}</p></div><button type="button" class="primary" data-add-provider>＋ ${markupText('Ajouter')}</button></div><div class="provider-grid">${providers.map(item => providerCardMarkup(item, accounts.get(item.id) || statuses[item.id])).join('')}</div><div class="provider-list-footer"><p class="hint">${markupText('Ouvrez un fournisseur pour consulter son compte et ses quotas.')}</p><button type="button" data-refresh-providers>${markupText('Vérifier les installations')}</button></div>`) + '<div data-provider-editor></div>';
    const add = host().querySelector('[data-add-provider]');
    if (add) add.onclick = () => edit({ type: 'codex', auth: 'cli' });
    const check = host().querySelector('[data-refresh-providers]');
    if (check) check.onclick = () => action(async () => { check.disabled = true; try { statuses = await api('/api/agents/check', 'POST', {}); accounts.clear(); render(); } finally { check.disabled = false; } });
    host().querySelectorAll('[data-open-provider]').forEach(button => button.onclick = () => select(button.dataset.openProvider));
    const back = host().querySelector('[data-provider-back]');
    if (back) back.onclick = () => { selected = ''; render(); host().querySelector('[data-open-provider]')?.focus(); };
    const update = host().querySelector('[data-refresh-account]');
    if (update) update.onclick = () => action(() => loadAccount(selected));
    const install = host().querySelector('[data-update-provider]');
    if (install) install.onclick = () => action(() => updateProvider(provider));
    host().querySelectorAll('[data-edit-provider]').forEach(button => button.onclick = () => edit(providers.find(provider => provider.id === button.dataset.editProvider)));
    host().querySelectorAll('[data-remove-provider]').forEach(button => button.onclick = () => action(async () => {
      if (!await confirmAction(msg('Sa clé enregistrée sera supprimée. Les profils utilisés par des tâches ou des projets doivent d’abord être remplacés.'), msg('Retirer ce fournisseur ?'), msg('Retirer'))) return;
      providers = await api('/api/agent-providers/' + button.dataset.removeProvider, 'DELETE'); accounts.clear(); selected = ''; await refresh(); render();
    }));
  }
  async function updateProvider(provider) {
    const id = provider.id, status = accounts.get(id) || statuses[id];
    if (!await confirmAction(msg('Mettre à jour {0} avec sa commande officielle ? Fermez aussi ses sessions ouvertes en dehors d’Atelier. Le canal configuré dans le CLI sera conservé.', provider.name), msg('Mettre à jour le CLI'), msg('Mettre à jour'))) return;
    updating = id; updatePhase = 'checking'; render();
    let polling = false;
    const timer = setInterval(async () => {
      if (polling) return;
      polling = true;
      try {
        const state = await api('/api/agent-providers/' + id + '/update');
        if (updating && state.providerId === id && ['checking', 'installing', 'verifying'].includes(state.phase)) {
          updatePhase = state.phase;
          const label = host().querySelector('[data-update-phase]');
          if (selected === id && label) label.textContent = updatePhaseLabel(state.phase);
        }
      } catch { /* The installation response reports failures; keep progress visible while waiting. */ }
      finally { polling = false; }
    }, 1000);
    try {
      const result = await api('/api/agent-providers/' + id + '/update', 'POST', { version: status.latestVersion });
      statuses[id] = result.status;
      if (accounts.has(id)) accounts.set(id, { ...accounts.get(id), ...result.status });
      toast(msg(result.outcome === 'updated' ? 'CLI mis à jour. Nouvelle version vérifiée.' : 'La commande est terminée, mais la version attendue n’est pas active. Vérifiez le canal ou le gestionnaire d’installation.'), result.outcome !== 'updated');
    } finally { clearInterval(timer); updating = ''; providers.forEach(item => { item.updating = false; item.updatePhase = ''; }); render(); }
  }
  async function loadAccount(id) {
    if (loading.has(id)) return;
    loading.add(id); render();
    try { accounts.set(id, await api('/api/agents/' + id + '/account', 'POST', {})); }
    finally { loading.delete(id); if (selected === id) render(); }
  }
  function select(id) {
    selected = id; render(); host().querySelector('[data-provider-title]')?.focus();
    const provider = providers.find(item => item.id === id);
    if (provider && !updating && !provider.updating && !provider.command && provider.type !== 'command') action(() => loadAccount(id));
  }
  function edit(provider) {
    const editor = host().querySelector('[data-provider-editor]');
    editor.innerHTML = `<form class="form-section" id="agent-provider-form"><h3>${markupText(provider.id ? 'Configurer le fournisseur' : 'Ajouter un fournisseur')}</h3><div class="form-row"><label>${markupText('Nom')}<input name="name" required maxlength="100" value="${esc(provider.name || '')}"></label><label>${markupText('Identifiant')}<input name="id" required pattern="[a-z][a-z0-9-]{0,63}" maxlength="64" ${provider.id ? 'readonly' : ''} value="${esc(provider.id || '')}" placeholder="codex-travail"></label></div><label>${markupText('Type')}<select name="type" ${provider.id ? 'disabled' : ''}>${[['codex', 'Codex CLI'], ['claude', 'Claude Code CLI'], ['command', 'CLI personnalisé']].map(([value, label]) => `<option value="${value}" ${value === provider.type ? 'selected' : ''}>${markupText(label)}</option>`).join('')}</select></label><div data-provider-options></div><div class="form-actions"><button type="button" data-cancel-provider>${markupText('Annuler')}</button><button class="primary">${markupText('Enregistrer')}</button></div></form>`;
    const form = editor.querySelector('form');
    function options(value = {}) {
      const type = form.elements.type.value === 'command' ? 'command' : form.elements.type.value === 'claude' ? 'claude' : 'codex';
      form.querySelector('[data-provider-options]').innerHTML = `<label>${markupText('Exécutable (facultatif pour Codex et Claude)')}<input name="command" maxlength="2000" ${type === 'command' ? 'required' : ''} value="${esc(value.command || '')}" placeholder="${type === 'command' ? '/chemin/agent' : type}"></label><p class="hint">${markupText('Chemin de l’exécutable, sans arguments. Aucun shell intermédiaire.')}</p>${type !== 'command' ? `<label>${markupText('Dossier de configuration (facultatif)')}<input name="configDir" maxlength="2000" value="${esc(value.configDir || '')}"></label><p class="hint">${markupText('Utilise CODEX_HOME ou CLAUDE_CONFIG_DIR pour sélectionner une configuration et une connexion existantes.')}</p>` : ''}<label>${markupText('Libellé du compte (facultatif)')}<input name="accountLabel" maxlength="200" value="${esc(value.accountLabel || '')}"></label><div class="form-row"><label>${markupText('Modèle')}<input name="model" maxlength="180" value="${esc(value.model || '')}"></label><label>${markupText('Niveau de réflexion')}${type === 'command' ? `<input name="effort" maxlength="40" value="${esc(value.effort || '')}">` : `<select name="effort"><option value="">${markupText('Automatique (CLI)')}</option>${Object.entries(effortLevels[type]).map(([key, label]) => `<option value="${key}" ${key === value.effort ? 'selected' : ''}>${esc(label)}</option>`).join('')}</select>`}</label></div>${type === 'codex' ? `<label>${markupText('Authentification')}<select name="auth"><option value="cli">${markupText('Connexion CLI existante')}</option><option value="api" ${value.auth === 'api' ? 'selected' : ''}>${markupText('Clé API via Codex CLI')}</option></select></label><div data-api-fields><label>${markupText('Clé API')}<input type="password" name="apiKey" autocomplete="new-password" maxlength="4000"></label><p class="hint">${markupText(value.apiKeySet ? 'Secret enregistré. Laisser vide pour le conserver.' : 'Aucun secret enregistré.')}</p><label class="check"><input type="checkbox" name="clearApiKey">${markupText('Oublier la clé API')}</label></div>` : ''}${type === 'command' ? `<p class="hint">${markupText('Le prompt est transmis sur l’entrée standard, la réponse est lue sur la sortie standard. Définissez les arguments en listes JSON ; {model} et {effort} sont remplacés. Le CLI doit lui-même respecter les permissions d’analyse et de modification.')}</p>${[['analyzeArgs', 'Arguments pour analyser'], ['editArgs', 'Arguments pour modifier'], ['versionArgs', 'Arguments pour lire la version'], ['statusArgs', 'Arguments pour lire le compte']].map(([key, label]) => `<label>${markupText(label)}<textarea name="${key}" rows="2">${esc(JSON.stringify(value[key] ?? (key === 'versionArgs' ? ['--version'] : [])))}</textarea></label>`).join('')}<p class="hint">${markupText('La commande de compte doit renvoyer un objet JSON : loggedIn (booléen), email et authMethod (facultatifs). Laissez [] si indisponible.')}</p><label>${markupText('Paquet npm pour vérifier les mises à jour (facultatif)')}<input name="npmPackage" maxlength="200" value="${esc(value.npmPackage || '')}" placeholder="@organisation/agent"></label>` : ''}`;
      if (form.elements.auth) {
        const toggle = () => { form.querySelector('[data-api-fields]').hidden = form.elements.auth.value !== 'api'; };
        form.elements.auth.onchange = toggle; toggle();
      }
    }
    options(provider);
    form.elements.type.onchange = () => options();
    form.querySelector('[data-cancel-provider]').onclick = () => editor.replaceChildren();
    form.onsubmit = event => { event.preventDefault(); action(async () => {
      const body = { ...Object.fromEntries(new FormData(form)), type: form.elements.type.value, clearApiKey: form.elements.clearApiKey?.checked || false };
      if (body.auth !== 'api') delete body.apiKey;
      for (const key of ['analyzeArgs', 'editArgs', 'versionArgs', 'statusArgs']) if (body[key] !== undefined) {
        try { body[key] = JSON.parse(body[key]); } catch { throw new Error(msg('Les arguments doivent être une liste JSON.')); }
      }
      const button = form.querySelector('.primary'); button.disabled = true;
      try {
        providers = await api('/api/agent-providers', 'POST', body); accounts.clear();
        if (form.elements.apiKey) form.elements.apiKey.value = '';
        await refresh(); statuses = await api('/api/agents'); render(); toast(msg('Fournisseur enregistré.'));
      } finally { button.disabled = false; }
    }); };
    form.elements.name.focus();
  }
  return { async open(id = '') { accounts.clear(); [providers, statuses] = await Promise.all([api('/api/agent-providers'), api('/api/agents')]); select(id); } };
}
