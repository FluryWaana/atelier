export default {
  sourceLocale: 'fr-FR',
  translations: { 'en-US': {
    "Validation et export du temps humain vers Tempo Cloud.": "Approve and export human time to Tempo Cloud.",
    "Jeton Tempo Cloud": "Tempo Cloud token"
  } },
  id: 'tempo', name: 'Tempo', apiVersion: 1, builtIn: true,
  description: 'Validation et export du temps humain vers Tempo Cloud.',
  check: (context, input) => context.builtins.check('Tempo Cloud', input),
  fields: [{ id: 'token', label: 'Jeton Tempo Cloud', type: 'password' }],
  credentialDependencies: ['jira'],
  capabilities: { time: Object.fromEntries(['preview', 'send', 'reconcile'].map(operation => [operation,
    (context, input) => context.builtins.time(operation, input),
  ])) },
};
