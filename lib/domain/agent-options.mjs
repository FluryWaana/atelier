export const effortLevels = {
  codex: { none: 'Aucune', minimal: 'Minimale', low: 'Faible', medium: 'Moyenne', high: 'Élevée', xhigh: 'Très élevée' },
  claude: { low: 'Faible', medium: 'Moyenne', high: 'Élevée', xhigh: 'Très élevée', max: 'Maximale' },
};

export function agentOptions(agent, { model = '', effort = '' } = {}) {
  if (!Object.hasOwn(effortLevels, agent)) throw new Error('Agent invalide.');
  if (typeof model !== 'string' || model.length > 180 || model.trim() && !/^[a-zA-Z0-9][a-zA-Z0-9._:/-]*$/.test(model.trim())) throw new Error('Modèle invalide.');
  if (typeof effort !== 'string' || effort !== '' && !Object.hasOwn(effortLevels[agent], effort)) throw new Error('Niveau de réflexion invalide pour cet agent.');
  return { model: model.trim(), effort };
}

export function resolveAgentOptions(settings, card, body) {
  const agent = body.agent;
  const options = agentOptions(agent, { model: body.model ?? (agent === card.agent ? card.model : '') ?? '', effort: body.effort ?? (agent === card.agent ? card.effort : '') ?? '' });
  return agentOptions(agent, { model: options.model || settings[agent + 'Model'] || '', effort: options.effort || settings[agent + 'Effort'] || '' });
}
