# Créer un connecteur Atelier

Les connecteurs sont des modules ES Node.js. Ils déclarent leurs paramètres,
leurs capacités et leurs onglets. Aucun changement de `server.mjs`, du HTML
ou du registre central n’est nécessaire pour installer un module tiers.

## Essayer en cinq minutes

1. Copier `examples/connectors/demo` dans `connectors/demo`.
2. Redémarrer Atelier.
3. Ouvrir **Paramètres → Modules**, choisir le connecteur de démonstration pour
   les tickets et enregistrer.
4. Configurer son titre dans sa rubrique de paramètres.
5. Revenir au tableau et synchroniser les tickets. Le module importe une carte.
6. Ouvrir **Paramètres → Diagnostic démo** : son action compte les cartes importées.

L’exemple n’appelle aucun service externe, n’exige aucun secret et peut être testé
avec `node --test --test-isolation=none test/connectors.test.mjs`.

## Contrat, version 1

Chaque dossier expose un `index.mjs` avec un export par défaut :

```js
export default {
  id: 'mon-service',             // stable, minuscules, chiffres et tirets
  name: 'Mon service',
  apiVersion: 1,
  description: 'À quoi sert ce connecteur',
  fields: [
    { id: 'url', label: 'Adresse du service', type: 'url' },
    { id: 'token', label: 'Jeton API', type: 'password' },
  ],
  capabilities: { tickets: { sync, reset } },
  tabs: [{ id: 'diagnostic', label: 'Diagnostic',
    actions: [{ id: 'check', label: 'Vérifier' }] }],
  actions: { check },
};
```

Les identifiants sont uniques. Une version inconnue, une capacité incomplète ou
une action manquante désactive le module et produit une erreur dans
**Paramètres → Modules**, sans empêcher le démarrage. Une erreur d’import est
traitée de la même façon. En cas d’identifiant dupliqué, le premier module valide
est conservé (les modules fournis précèdent les extensions personnelles).
Corrigez ou retirez le module signalé, puis redémarrez Atelier. L’identifiant
reste stable lorsqu’on renomme le module : il identifie ses paramètres et secrets.

### Champs de configuration

Types disponibles : `text`, `textarea`, `url`, `password`, `checkbox`, `select`.
Un champ peut avoir une valeur `default`. Un `select` fournit
`options: [{ value: 'cloud', label: 'Cloud' }]`.
Les chaînes sont limitées à 4 000 caractères. `validateSettings(values)` peut
refuser une combinaison de valeurs en levant une erreur compréhensible.

Les champs `password` sont enregistrés dans le coffre chiffré et ne sont jamais
renvoyés à l’interface. Laisser vide conserve le secret ; **Oublier ce secret**
le supprime. Modifier une URL invalide les secrets non ressaisis. Utiliser
`credentialScope: true` pour une autre valeur qui change l’identité de la connexion
(compte, organisation, type de serveur).

Un module peut déclarer `credentialDependencies: ['autre-module']` : changer
l’identité de cette connexion invalide aussi ses secrets (cas de Tempo et Jira).
La fonction optionnelle `check(context, input)` retourne un message de diagnostic
ou lève une erreur. **Tester les connexions** vérifie les fournisseurs sélectionnés,
en plus des outils locaux, sans contacter les fournisseurs remplacés.

### Contexte des fonctions

Chaque fonction reçoit `(context, input)` et peut retourner une promesse.

- `context.settings` : paramètres propres au module, sans ses secrets.
- `context.secret('token')` : secret du module, côté serveur uniquement.
- `context.tickets.upsert(ticket)` : importer ou mettre à jour un ticket.
- `context.tickets.attach(cardId, ticket)` : rattacher un ticket créé à une carte locale, sans créer une deuxième carte ni écraser une autre liaison.
- `context.tickets.list()` : cartes importées par ce module uniquement.
- `context.tasks` et `context.projects` : cas d’usage de l’application.
- `context.time.get(id)` et `context.time.save(entry)` : saisies locales.
- `context.changed()` : signaler un changement aux navigateurs.

`upsert` attend `{ id, key, title, description, url, status, instance? }`. `id` identifie
durablement le ticket dans l’instance distante, `status` décrit son état distant.
L’identité locale combine le connecteur, l’instance et cet identifiant : changer
d’instance ne remplace pas les tickets déjà importés. Par défaut, l’instance est
l’origine de `url` (protocole, hôte et port). Si plusieurs instances ou espaces
partagent cette origine, fournissez toujours leur URL de base dans `instance`,
par exemple `https://service.example/client-a`. Elle doit partager l’origine du
ticket, sans identifiants, paramètres ni fragment ; le slash final est ignoré.
`attach` applique la même règle et refuse de remplacer une liaison d’une autre
instance. Les anciennes cartes sans `instance` restent reconnues par l’origine
de leur URL. Introduire ensuite un chemin d’instance crée une identité distincte ;
cela évite de réattribuer automatiquement un historique ambigu.
`upsert` conserve la colonne et le travail local lors d’une nouvelle synchronisation,
refuse de modifier une carte occupée et n’exécute aucun agent. Les URL doivent
être HTTP(S), sans identifiants intégrés. La carte stocke sa provenance dans
`ticket.connector`, indépendamment du fournisseur choisi pour les imports suivants.

Ne journalisez jamais les secrets. Utilisez des délais d’expiration pour les appels
réseau et vérifiez `response.ok`. Le serveur et ses modules tournent sur Node.js 24 ;
`fetch` et `AbortSignal.timeout` sont disponibles sans dépendance supplémentaire.

## Capacités

Une capacité annoncée doit fournir toutes ses opérations obligatoires. Un module
peut en fournir plusieurs ou seulement ajouter des onglets.

| Capacité | Opérations obligatoires | Entrée |
| --- | --- | --- |
| `tickets` | `sync`, `reset` | options de synchronisation |
| `repositories` | `preview`, `publish`, `refresh`, `review` | `{ card, body }` (`body` pour la publication) |
| `time` | `preview`, `send`, `reconcile` | `{ id, body }` |

`tickets.sync` importe les tickets et retourne `{ count }`. `reset` réconcilie le
périmètre importé ; il ne doit pas supprimer l’historique local. L’exemple réimporte
simplement son unique ticket. Les fonctions optionnelles `syncStatus`, `create`,
`transitions`, `transition`, `linkPullRequest` reçoivent `{ card, body }`. Elles
permettent les écritures distantes correspondantes lorsque le module les propose.

Les opérations de dépôt respectent les résultats des workflows existants :
`preview` fournit notamment `title`, `body`, `targetBranch`, `repository`,
`branch`, `dirty`, `status`, `commits`, `diff` ; `publish` fournit la PR avec `url`,
`number`, `title`, `status` ; `refresh` fournit son état et ses contrôles ;
`review` fournit le contexte utilisé par la revue (voir `lib/reviews.mjs`). Une PR
garde son fournisseur d’origine dans `pullRequest.connector` après un changement
de fournisseur global. Les jetons d’un autre fournisseur ne sont pas passés dans
les arguments des opérations.

Pour les temps, `send` exige une confirmation explicite (`body.confirmed`).
`preview` peut fournir `{ message }`, affiché dans la confirmation avant l’envoi.
Les rapprochements utilisent le fournisseur enregistré lors de l’envoi, même
si le fournisseur global a été changé depuis.
Le module doit enregistrer `sending` avant l’appel distant, `exported` au succès,
et `uncertain` si le serveur distant a pu recevoir l’envoi sans réponse certaine.
`reconcile` recherche cet envoi avant toute nouvelle tentative. Ne renvoyez pas
automatiquement une saisie incertaine : cela peut doubler le temps facturé.
Conservez une clé d’idempotence basée sur l’identifiant local. Tempo fournit un
exemple de ce cycle dans `lib/application/time-tracking.mjs`.

## Onglets et actions

Les onglets déclarés apparaissent dans la page Paramètres, avec les réglages des
modules. Le navigateur affiche les libellés et résultats comme du texte, sans
exécuter du HTML ou du JavaScript fourni par le module. Chaque action est un POST
local avec la protection d’origine habituelle. Son résultat est affiché comme
texte ou JSON. Cette version du contrat propose cette interface déclarative ;
elle n’injecte pas de composants arbitraires dans le navigateur.

## Installation, confiance et tests

Un module local est du code de confiance, pas une extension isolée : Node.js lui
donne les mêmes droits système qu’à Atelier. Il peut importer ses propres fichiers
et dépendances. Le chargeur ne télécharge ni n’installe de paquet automatiquement.
Les modules se chargent au redémarrage depuis `connectors/*/index.mjs`. Ce dossier
est distinct des fichiers remplacés par la mise à jour d’Atelier.

Pour tester, injectez `connectorModules: [module]` dans `createApp` avec un dossier
de données temporaire et des services simulés. `test/connectors.test.mjs` et
`test/connectors-api.test.mjs` montrent les tests du contrat et du branchement HTTP.
Les intégrations Jira, Azure DevOps, GitHub, GitLab et Tempo sont dans `lib/connectors/`.
GitHub et GitLab utilisent le workflow Git commun de `lib/forge-repositories.mjs`
et leurs adaptateurs REST dans `lib/forges/`. Les autres adaptateurs réutilisent
les workflows existants. `context.builtins` est réservé aux adaptateurs internes
et ne fait pas partie du contrat public.

Pour configurer Azure DevOps, GitHub ou GitLab, consultez le
[guide des dépôts et de leur authentification](repository-connectors.md).
# Modules optionnels

Les menus utilisent les catégories **Tickets**, **Dépôts Git** et **Suivi du temps**. Les marques apparaissent dans le choix du fournisseur. Choisir « Aucun module actif » masque les réglages et commandes associés ; les fonctions locales restent disponibles. Les PR existantes conservent leur fournisseur d’origine lorsqu’il est installé.

Les modules fournis sont découverts dans `lib/connectors/*.mjs`, les extensions personnelles dans `connectors/*/index.mjs`. Retirer un module puis redémarrer retire ses options du catalogue. Les onglets d’un fournisseur apparaissent lorsqu’il est sélectionné.

Un projet peut définir `repositoryConnector` : un identifiant de module fournissant
la capacité `repositories`, ou une chaîne vide pour hériter du fournisseur général.
Le catalogue expose `projectRepositoryProviders`, la liste des fournisseurs choisis
par les projets, et `pullRequestProviders`, ceux des PR/MR existantes (y compris
les cartes archivées). Leurs paramètres restent accessibles sans modifier le
fournisseur général. Une ancienne PR sans `connector` utilise Azure DevOps.
Les PR/MR existantes gardent la priorité de leur propre champ `connector`.
Le champ `errors` du catalogue contient les identifiants et messages des modules
non chargés ; l’interface les affiche comme du texte.
