import { agentProviders, providerSettings } from '../domain/agent-options.mjs';
import { fail, text } from '../domain/validation.mjs';
import { releaseVersion, isNewer } from '../domain/releases.mjs';

export function createAgentProviders({ settings, store, busy, invalidate, updateInfo = () => ({}), install, probe, changed = () => {} }) {
  let updating = '';
  let progress = { providerId: '', phase: 'idle' };
  const phase = (providerId, value) => { progress = { providerId, phase: value }; changed(); };
  const secretName = id => 'agent:' + id + ':key';
  function save(body) {
    const provider = providerSettings(body), config = settings.config(), providers = agentProviders(config);
    if (body.apiKey !== undefined && typeof body.apiKey !== 'string' || body.clearApiKey !== undefined && typeof body.clearApiKey !== 'boolean') fail('Clé API invalide.');
    const previous = providers.find(item => item.id === provider.id);
    if (previous && previous.type !== provider.type) fail('Le type d’un profil existant ne peut pas changer. Créez un autre profil.');
    if (updating || busy()) fail('Attendez la fin des exécutions et mises à jour avant de modifier les fournisseurs.', 'conflict');
    if (!previous && providers.length >= 50) fail('50 fournisseurs maximum.');
    const key = body.apiKey ? text(body.apiKey, 4000) : '';
    if (key && provider.auth !== 'api') fail('Sélectionnez l’authentification par clé API.');
    if (/[\r\n\0]/.test(key)) fail('Clé API invalide.');
    const secrets = body.clearApiKey || provider.auth !== 'api' ? { [secretName(provider.id)]: '' } : key ? { [secretName(provider.id)]: key } : {};
    settings.saveSecrets(secrets, () => store.saveSettings({ ...config, [provider.id + 'Model']: provider.model, [provider.id + 'Effort']: provider.effort, agentProviders: previous ? providers.map(item => item.id === provider.id ? provider : item) : [...providers, provider] }));
    invalidate();
    return describe();
  }
  function remove(id) {
    const config = settings.config(), providers = agentProviders(config);
    if (!providers.some(item => item.id === id)) fail('Fournisseur introuvable.', 'not-found');
    if (updating || busy() || config.agent === id || store.cards().some(card => card.agent === id) || store.projects().some(project => project.agent === id)) fail('Ce fournisseur est utilisé. Changez les tâches, les projets et le réglage par défaut avant de le retirer.', 'conflict');
    settings.saveSecrets({ [secretName(id)]: '' }, () => store.saveSettings({ ...config, agentProviders: providers.filter(item => item.id !== id) }));
    invalidate();
    return describe();
  }
  async function update(id, requestedVersion) {
    const version = releaseVersion(requestedVersion);
    const provider = agentProviders(settings.config()).find(item => item.id === id);
    if (!provider) fail('Fournisseur introuvable.', 'not-found');
    if (!['codex', 'claude'].includes(provider.type) || provider.command || !updateInfo(provider).canUpdate) fail('Cette installation doit être mise à jour avec son gestionnaire habituel.');
    if (updating || busy()) fail('Arrêtez les agents et attendez la fin des opérations avant de mettre à jour un CLI.', 'conflict');
    updating = id; phase(id, 'checking');
    try {
      const before = await probe(provider);
      if (!before.installed || before.updateState !== 'outdated' || before.latestVersion !== version) fail('La version disponible a changé. Actualisez la fiche avant de lancer la mise à jour.', 'conflict');
      phase(id, 'installing');
      await install(provider);
      phase(id, 'verifying');
      const status = await probe(provider);
      const installed = status.version?.match(/\b(\d+\.\d+\.\d+)(?![\w.-])/u)?.[1];
      const complete = status.installed && installed && (installed === version || isNewer(installed, version));
      phase(id, complete ? 'updated' : 'unchanged');
      return { outcome: complete ? 'updated' : 'unchanged', status };
    } catch (error) { phase(id, 'failed'); throw error; }
    finally { updating = ''; invalidate(); changed(); }
  }
  function describe() { return agentProviders(settings.config()).map(provider => ({ ...provider, ...updateInfo(provider), updating: Boolean(updating), updatePhase: updating === provider.id ? progress.phase : '', apiKeySet: Boolean(settings.token(secretName(provider.id))) })); }
  return { save, remove, describe, update, updateState: () => ({ ...progress, updating: Boolean(updating) }), updating: () => Boolean(updating), key: id => settings.token(secretName(id)) };
}
