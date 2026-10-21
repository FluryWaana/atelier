export default {
  sourceLocale: 'fr-FR',
  translations: { 'en-US': {
    "Préparation, publication et suivi des pull requests Azure Repos.": "Prepare, publish and track Azure Repos pull requests.",
    "PAT facultatif (sinon : session Azure CLI)": "Optional PAT (otherwise: Azure CLI session)"
  } },
  id: 'azure', name: 'Azure DevOps', apiVersion: 1, builtIn: true,
  description: 'Préparation, publication et suivi des pull requests Azure Repos.',
  check: (context, input) => context.builtins.check('Azure Repos', input),
  fields: [{ id: 'token', label: 'PAT facultatif (sinon : session Azure CLI)', type: 'password' }],
  capabilities: { repositories: Object.fromEntries(['preview', 'publish', 'refresh', 'review'].map(operation => [operation,
    (context, input) => context.builtins.repositories[operation](input.card, input.body),
  ])) },
};
