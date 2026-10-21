import { validateConnector, connectorCapabilities, optionalConnectorOperations } from '../domain/connectors.mjs';
import { fail } from '../domain/validation.mjs';

export function createConnectors({ modules, errors = [], settings, saveSettings, getSecret, saveSecrets, context, projectRepositoryProviders = () => [], pullRequestProviders = () => [], repositoryProviderFor = () => '' }) {
  const registry = new Map();
  const loadErrors = [...errors];
  let pending = 0;
  for (const module of modules) {
    try {
      validateConnector(module);
      if (registry.has(module.id)) throw new Error(`Connecteur dupliqué : ${module.id}`);
      registry.set(module.id, module);
    } catch (error) { loadErrors.push({ id: module?.id || 'Module inconnu', message: String(error?.message || error) }); }
  }
  const get = id => registry.get(id) || fail('Connecteur introuvable.', 'not-found');
  const selected = capability => {
    const id = settings().connectorProviders?.[capability] ?? ({ tickets: 'jira', repositories: 'azure', time: 'tempo' })[capability];
    return registry.get(id)?.capabilities?.[capability] ? id : '';
  };
  function describe() {
    return { errors: loadErrors, projectRepositoryProviders: [...new Set(projectRepositoryProviders())], pullRequestProviders: [...new Set(pullRequestProviders())], providers: Object.fromEntries(Object.keys(connectorCapabilities).map(key => [key, selected(key)])), modules: [...registry.values()].map(module => ({
      id: module.id, name: module.name, description: module.description || '', apiVersion: module.apiVersion,
      capabilities: Object.keys(module.capabilities || {}), fields: module.fields || [], tabs: module.tabs || [], builtIn: Boolean(module.builtIn),
      operations: Object.fromEntries(Object.entries(module.capabilities || {}).map(([key, handlers]) => [key, Object.keys(handlers)])),
      values: Object.fromEntries((module.fields || []).filter(field => field.type !== 'password').map(field => [field.id, settings().connectorSettings?.[module.id]?.[field.id] ?? field.default ?? (field.type === 'checkbox' ? false : '')])),
      secrets: Object.fromEntries((module.fields || []).filter(field => field.type === 'password').map(field => [field.id, Boolean(getSecret(`connector:${module.id}:${field.id}`))])),
    })) };
  }
  function select(providers) {
    if (pending) fail('Attendez la fin des opérations des connecteurs.', 'conflict');
    if (!providers || typeof providers !== 'object' || Array.isArray(providers)) fail('Fournisseurs invalides.');
    for (const [capability, id] of Object.entries(providers)) {
      if (!Object.hasOwn(connectorCapabilities, capability) || typeof id !== 'string' || id && !get(id).capabilities?.[capability]) fail('Ce connecteur ne fournit pas cette capacité.');
    }
    saveSettings({ ...settings(), connectorProviders: { ...settings().connectorProviders, ...providers } });
    return describe();
  }
  function configure(id, body) {
    if (pending) fail('Attendez la fin des opérations des connecteurs.', 'conflict');
    const module = get(id), values = {}, secrets = {};
    if (!body || typeof body !== 'object' || Array.isArray(body)) fail('Paramètres invalides.');
    const fields = new Map((module.fields || []).map(field => [field.id, field]));
    for (const [key, value] of Object.entries(body)) {
      const field = fields.get(key);
      if (!field) fail('Paramètre inconnu.');
      if (field.type === 'checkbox' ? typeof value !== 'boolean' : typeof value !== 'string' || value.length > 4000) fail('Valeur de paramètre invalide.');
      if (field.type === 'select' && !field.options.some(option => option.value === value)) fail('Choix invalide.');
      if (field.type === 'url' && value) {
        let url; try { url = new URL(value); } catch { fail('Adresse invalide.'); }
        if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password) fail('Adresse invalide.');
      }
      if (field.type === 'password') { if (value) secrets[`connector:${id}:${key}`] = value; }
      else values[key] = value;
    }
    const next = { ...(settings().connectorSettings?.[id] || {}), ...values };
    module.validateSettings?.(next);
    if ((module.fields || []).some(field => (field.type === 'url' || field.credentialScope) && values[field.id] !== undefined && next[field.id] !== settings().connectorSettings?.[id]?.[field.id])) {
      for (const field of module.fields || []) if (field.type === 'password' && !body[field.id]) secrets[`connector:${id}:${field.id}`] = '';
      for (const dependent of registry.values()) if (dependent.credentialDependencies?.includes(id)) {
        for (const field of dependent.fields || []) if (field.type === 'password') secrets[`connector:${dependent.id}:${field.id}`] = '';
      }
    }
    saveSecrets(secrets, () => saveSettings({ ...settings(), connectorSettings: { ...settings().connectorSettings, [id]: next } }));
    return describe();
  }
  function forget(id, field) {
    if (pending) fail('Attendez la fin des opérations des connecteurs.', 'conflict');
    if (!get(id).fields?.some(item => item.id === field && item.type === 'password')) fail('Secret inconnu.');
    saveSecrets({ [`connector:${id}:${field}`]: '' });
    return describe();
  }
  function moduleContext(module) {
    const defaults = Object.fromEntries((module.fields || []).filter(field => field.type !== 'password' && field.default !== undefined).map(field => [field.id, field.default]));
    return { ...context(module.id), settings: { ...defaults, ...settings().connectorSettings?.[module.id] }, secret: field => getSecret(`connector:${module.id}:${field}`) };
  }
  async function invoke(capability, operation, input, id = selected(capability)) {
    if (!Object.hasOwn(connectorCapabilities, capability) || ![...connectorCapabilities[capability], ...(optionalConnectorOperations[capability] || [])].includes(operation)) fail('Opération de connecteur inconnue.', 'not-found');
    if (!id) fail('Aucun module actif pour cette fonction.');
    const module = get(id), handler = module.capabilities?.[capability]?.[operation];
    if (!handler) fail('Opération non prise en charge.');
    pending++;
    try { return await handler(moduleContext(module), input); }
    finally { pending--; }
  }
  async function action(id, name, input) {
    const module = get(id);
    if (!module.tabs?.some(tab => tab.actions?.some(action => action.id === name))) fail('Action inconnue.', 'not-found');
    pending++;
    try { return await module.actions[name](moduleContext(module), input); }
    finally { pending--; }
  }
  async function checkConnections(input = {}) {
    const ids = [...new Set(Object.keys(connectorCapabilities).map(key => key === 'repositories' ? repositoryProviderFor(input.repo) || selected(key) : selected(key)).filter(Boolean))];
    return Promise.all(ids.map(async id => {
      const module = registry.get(id), name = module?.name || id;
      if (!module) return { name, ok: false, message: 'Module non installé.' };
      if (!module.check) return { name, ok: true, message: 'Ce module ne fournit pas de diagnostic de connexion.' };
      pending++;
      try { return { name, ok: true, message: await module.check(moduleContext(module), input) }; }
      catch (error) { return { name, ok: false, message: error.message }; }
      finally { pending--; }
    }));
  }
  return { describe, select, configure, forget, invoke, action, selected, checkConnections, busy: () => pending > 0, supports: (id, capability, operation) => typeof registry.get(id)?.capabilities?.[capability]?.[operation] === 'function' };
}
