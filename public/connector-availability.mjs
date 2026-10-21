import { sameRepository } from '../lib/domain/paths.mjs';

export function connectorSupports(catalog, capability, operation, id = catalog?.providers?.[capability]) {
  return Boolean(id && catalog?.modules?.find(module => module.id === id)?.operations?.[capability]?.includes(operation));
}

export function repositoryProvider(state, card) {
  return card?.pullRequest?.connector || (card?.pullRequest ? 'azure' : state.projects?.find(project => sameRepository(project.repo, card?.repo || state.settings?.repo, state.settings?.platform))?.repositoryConnector || state.connectors?.providers?.repositories);
}
