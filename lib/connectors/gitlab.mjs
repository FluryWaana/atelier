import adapter from '../forges/gitlab.mjs';
import { createForgeRepositories, forgeUrl } from '../forge-repositories.mjs';

export default {
  sourceLocale: 'fr-FR',
  translations: { 'en-US': {
    "Merge requests GitLab.com et instances privées. Session glab CLI ou jeton personnel ; Git utilise ses identifiants habituels.": "GitLab.com and private instance merge requests. Uses a glab CLI session or personal token; Git uses its usual credentials.",
    "Adresse de GitLab": "GitLab address",
    "Jeton facultatif (sinon : session glab CLI)": "Optional token (otherwise: glab CLI session)"
  } },
  id: 'gitlab', name: 'GitLab', apiVersion: 1, builtIn: true,
  description: 'Merge requests GitLab.com et instances privées. Session glab CLI ou jeton personnel ; Git utilise ses identifiants habituels.',
  fields: [
    { id: 'url', label: 'Adresse de GitLab', type: 'url', default: 'https://gitlab.com' },
    { id: 'token', label: 'Jeton facultatif (sinon : session glab CLI)', type: 'password' },
  ],
  validateSettings(settings) { settings.url = forgeUrl(settings.url, 'gitlab'); },
  check: (context, input) => createForgeRepositories(adapter, context).check(input),
  capabilities: { repositories: Object.fromEntries(['preview', 'publish', 'refresh', 'review'].map(operation => [operation,
    (context, input) => createForgeRepositories(adapter, context)[operation](input.card, input.body),
  ])) },
};
