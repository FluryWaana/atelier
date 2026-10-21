import adapter from '../forges/github.mjs';
import { createForgeRepositories, forgeUrl } from '../forge-repositories.mjs';

export default {
  id: 'github', name: 'GitHub', apiVersion: 1, builtIn: true,
  description: 'Pull requests GitHub et GitHub Enterprise. Session gh CLI ou jeton personnel ; Git utilise ses identifiants habituels.',
  fields: [
    { id: 'url', label: 'Adresse de GitHub', type: 'url', default: 'https://github.com' },
    { id: 'token', label: 'Jeton facultatif (sinon : session gh CLI)', type: 'password' },
  ],
  validateSettings(settings) { settings.url = forgeUrl(settings.url, 'github'); },
  check: (context, input) => createForgeRepositories(adapter, context).check(input),
  capabilities: { repositories: Object.fromEntries(['preview', 'publish', 'refresh', 'review'].map(operation => [operation,
    (context, input) => createForgeRepositories(adapter, context)[operation](input.card, input.body),
  ])) },
};
