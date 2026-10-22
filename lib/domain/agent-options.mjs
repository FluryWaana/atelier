export const effortLevels = {
  codex: { none: 'Aucune', minimal: 'Minimale', low: 'Faible', medium: 'Moyenne', high: 'Élevée', xhigh: 'Très élevée' },
  claude: { low: 'Faible', medium: 'Moyenne', high: 'Élevée', xhigh: 'Très élevée', max: 'Maximale' },
  command: {},
};

export function agentProviders(settings = {}) {
  return settings.agentProviders ?? [
    { id: 'codex', name: 'Codex', type: 'codex', auth: 'cli', model: settings.codexModel || '', effort: settings.codexEffort || '' },
    { id: 'claude', name: 'Claude Code', type: 'claude', auth: 'cli', model: settings.claudeModel || '', effort: settings.claudeEffort || '' },
  ];
}

export function agentProvider(settings, id) {
  const provider = agentProviders(settings).find(provider => provider.id === id);
  if (!provider) throw new Error('Agent invalide.');
  return provider;
}

export function providerSettings(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) throw new Error('Fournisseur invalide.');
  const text = (value = '', max = 2000) => {
    if (typeof value !== 'string' || value.length > max || /[\x00-\x1f]/.test(value)) throw new Error('Réglage du fournisseur invalide.');
    return value.trim();
  };
  const id = text(body.id, 64), name = text(body.name, 100), type = body.type, auth = body.auth ?? 'cli';
  if (!/^[a-z][a-z0-9-]{0,63}$/.test(id) || ['constructor', 'prototype', '__proto__'].includes(id) || !name || !Object.hasOwn(effortLevels, type)) throw new Error('Fournisseur invalide.');
  if (!['cli', 'api'].includes(auth) || auth === 'api' && type !== 'codex') throw new Error('Authentification invalide.');
  const args = (value = []) => {
    if (!Array.isArray(value) || value.length > 100) throw new Error('Les arguments doivent être une liste JSON.');
    return value.map(value => { if (typeof value !== 'string' || value.length > 4000 || value.includes('\0')) throw new Error('Argument invalide.'); return value; });
  };
  const command = text(body.command), configDir = text(body.configDir), accountLabel = text(body.accountLabel, 200), npmPackage = text(body.npmPackage, 200);
  if ([command, configDir].some(path => /^[\\/]{2}/.test(path))) throw new Error('Les chemins réseau et périphériques ne sont pas autorisés.');
  if (configDir && !/^(?:[a-zA-Z]:[\\/]|\/|\\\\)/.test(configDir)) throw new Error('Le dossier de configuration doit être absolu.');
  if (command && /[\\/]/.test(command) && !/^(?:[a-zA-Z]:[\\/]|\/|\\\\)/.test(command)) throw new Error('Le chemin de l’exécutable doit être absolu.');
  if (npmPackage && !/^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/.test(npmPackage)) throw new Error('Paquet npm invalide.');
  if (command.startsWith('-') || type === 'command' && !command) throw new Error('Exécutable requis.');
  return { id, name, type, auth, command, configDir, accountLabel, ...agentOptions(type, body),
    ...(type === 'command' ? { analyzeArgs: args(body.analyzeArgs), editArgs: args(body.editArgs), versionArgs: args(body.versionArgs ?? ['--version']), statusArgs: args(body.statusArgs), npmPackage } : {}),
  };
}

export function agentOptions(agent, { model = '', effort = '' } = {}, settings) {
  const type = settings ? agentProvider(settings, agent).type : agent;
  if (!Object.hasOwn(effortLevels, type)) throw new Error('Agent invalide.');
  if (typeof model !== 'string' || model.length > 180 || model.trim() && !/^[a-zA-Z0-9][a-zA-Z0-9._:/-]*$/.test(model.trim())) throw new Error('Modèle invalide.');
  if (typeof effort !== 'string' || effort !== '' && (type === 'command' ? !/^[a-zA-Z0-9_-]{1,40}$/.test(effort) : !Object.hasOwn(effortLevels[type], effort))) throw new Error('Niveau de réflexion invalide pour cet agent.');
  return { model: model.trim(), effort };
}

export function resolveAgentOptions(settings, card, body) {
  const agent = body.agent;
  const provider = agentProvider(settings, agent);
  const options = agentOptions(agent, { model: body.model ?? (agent === card.agent ? card.model : '') ?? '', effort: body.effort ?? (agent === card.agent ? card.effort : '') ?? '' }, settings);
  return agentOptions(agent, { model: options.model || provider.model || '', effort: options.effort || provider.effort || '' }, settings);
}
