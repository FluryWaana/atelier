export const connectorApiVersion = 1;
export const connectorCapabilities = {
  tickets: ['sync', 'reset'],
  repositories: ['preview', 'publish', 'refresh', 'review'],
  time: ['preview', 'send', 'reconcile'],
};
export const optionalConnectorOperations = { tickets: ['syncStatus', 'create', 'transitions', 'transition', 'linkPullRequest'] };

export function validateConnector(module) {
  const identifier = value => typeof value === 'string' && /^[a-z][a-z0-9-]{0,63}$/.test(value);
  if (!module || !identifier(module.id) || typeof module.name !== 'string' || !module.name.trim() || module.apiVersion !== connectorApiVersion) throw new Error('Connecteur invalide : identifiant, nom ou version du contrat.');
  if (module.check !== undefined && typeof module.check !== 'function') throw new Error(`Connecteur ${module.id} : diagnostic invalide.`);
  for (const [capability, handlers] of Object.entries(module.capabilities || {})) {
    if (!connectorCapabilities[capability] || connectorCapabilities[capability].some(operation => typeof handlers[operation] !== 'function')) throw new Error(`Connecteur ${module.id} : capacité ${capability} incomplète.`);
    for (const [operation, handler] of Object.entries(handlers)) if (![...connectorCapabilities[capability], ...(optionalConnectorOperations[capability] || [])].includes(operation) || typeof handler !== 'function') throw new Error(`Connecteur ${module.id} : opération ${operation} invalide.`);
  }
  const localePattern = /^[a-z]{2,3}(?:-[A-Za-z0-9]{2,8})*$/;
  if (module.sourceLocale !== undefined && (typeof module.sourceLocale !== 'string' || !localePattern.test(module.sourceLocale))) throw new Error('Langue source du connecteur invalide.');
  if (module.translations !== undefined) {
    if (!module.translations || typeof module.translations !== 'object' || Array.isArray(module.translations)) throw new Error('Traductions du connecteur invalides.');
    for (const [locale, messages] of Object.entries(module.translations)) {
      if (!localePattern.test(locale) || !messages || typeof messages !== 'object' || Array.isArray(messages) || Object.entries(messages).some(([key, value]) => !key || typeof value !== 'string' || value.length > 20000)) throw new Error('Catalogue du connecteur invalide.');
    }
  }
  const fields = new Set();
  for (const field of module.fields || []) {
    if (!identifier(field.id) || fields.has(field.id) || typeof field.label !== 'string' || !['text', 'url', 'password', 'checkbox', 'select', 'textarea'].includes(field.type)) throw new Error(`Connecteur ${module.id} : champ invalide.`);
    if (field.type === 'select' && (!Array.isArray(field.options) || !field.options.length || field.options.some(option => typeof option.value !== 'string' || typeof option.label !== 'string'))) throw new Error(`Connecteur ${module.id} : choix invalides.`);
    fields.add(field.id);
  }
  const tabs = new Set();
  for (const tab of module.tabs || []) {
    if (!identifier(tab.id) || tabs.has(tab.id) || typeof tab.label !== 'string') throw new Error(`Connecteur ${module.id} : onglet invalide.`);
    tabs.add(tab.id);
    for (const action of tab.actions || []) {
      if (!identifier(action.id) || typeof action.label !== 'string' || typeof module.actions?.[action.id] !== 'function') throw new Error(`Connecteur ${module.id} : action invalide.`);
    }
  }
  return module;
}
