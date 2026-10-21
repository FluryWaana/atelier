# Architecture et évolution

Atelier reste un monolithe local : un processus Node.js, SQLite, une interface web et les CLI du poste. Tâches, projets, réglages, exécution d’agents, workflows, synchronisation des tickets, suivi du temps et mises à jour séparent leurs cas d’usage des adaptateurs, sans dépendance npm supplémentaire à l’exécution.

Les connecteurs sont décrits par le contrat versionné de `domain/connectors.mjs`, orchestrés par `application/connectors.mjs` et chargés depuis `connectors/*/index.mjs` par l’adaptateur `infrastructure/connector-loader.mjs`. Les modules fournis résident dans `lib/connectors/`. `public/views/settings.mjs` affiche les champs et onglets déclarés sans exécuter de code de module dans le navigateur. Le [guide de contribution](connectors.md) décrit le contrat public et l’exemple installé sans modification du cœur.

Les cartes utilisent désormais `ticket` avec l’identifiant du connecteur ; les PR et exports conservent aussi leur fournisseur. Cette évolution préproduction porte le format de données à la version 2, sans migration des anciens objets Jira. Le répertoire des connecteurs installés n’appartient pas aux entrées remplacées par les mises à jour ; les exemples livrés, eux, sont gérés par l’application.

## Dépendances des mises à jour

```text
public/updates.mjs → HTTP → lib/http/update-routes.mjs
                                    ↓
                         lib/application/updates.mjs
                                    ↓
                            lib/domain/releases.mjs

server.mjs assemble les cas d’usage et leurs adaptateurs :
  lib/infrastructure/github-releases.mjs  → GitHub Releases public
  lib/infrastructure/package-updater.mjs → téléchargement, extraction, fichiers
  lib/infrastructure/tar-package.mjs     → extraction POSIX contrôlée
  scripts/update-worker.mjs              → remplacement, redémarrage, restauration
```

Le domaine ne dépend ni de Node.js ni de HTTP. Le cas d’usage reçoit les ports `releases.latest`, `installer.prepare/start`, `assertIdle` et `backup`. Il ne connaît pas GitHub, PowerShell ou SQLite. Les adaptateurs s’injectent par des objets et fonctions JavaScript ; aucune hiérarchie de classes n’est nécessaire.

## Principes SOLID appliqués

- **Responsabilité unique** : comparaison des versions, orchestration, HTTP, accès GitHub et remplacement des fichiers ont chacun leur module.
- **Ouvert/fermé** : une autre source de publication peut remplacer l’adaptateur GitHub sans changer le cas d’usage.
- **Substitution** : les doubles de test respectent les mêmes méthodes asynchrones et erreurs que les adaptateurs réels.
- **Interfaces petites** : le cas d’usage reçoit uniquement les opérations utilisées ; pas de service global donnant accès à tout le serveur.
- **Inversion des dépendances** : `server.mjs` compose les dépendances. Le domaine et l’application n’importent pas leurs adaptateurs.

## Tâches, projets et vues

`lib/domain/tasks.mjs` porte les statuts, les dépendances entre tâches et les exigences de review. Les options d’agent, les checklists et les validations partagées sont également dans le domaine. Les anciens modules réexportent ces fonctions pour conserver les imports existants ; seuls les modules nécessaires sont explicitement servis au navigateur.

`lib/application/tasks.mjs` orchestre la création, l’édition, la duplication, l’archivage, la suppression et les modèles. `lib/application/projects.mjs` gère les projets, la détection des paramètres de lancement et les changements d’emplacement. Ils reçoivent leurs opérations de stockage et leurs services sous forme de fonctions, sans importer SQLite, HTTP ou les adaptateurs système.

`lib/infrastructure/board-repositories.mjs` adapte le stockage existant et contient les transactions SQL. Les lectures doivent fournir des objets indépendants du stockage : les cas d’usage peuvent les modifier avant validation, mais seule une écriture explicite les persiste. Les transactions sont synchrones, comme le stockage SQLite actuel. Un changement d’emplacement sauvegarde les données avant la transaction ; une erreur annule les écritures.

`lib/http/task-routes.mjs` et `project-routes.mjs` dispatchent les requêtes. `server.mjs` compose les dépendances, applique les protections HTTP et traduit les erreurs métier en réponses HTTP. Les règles de blocage pendant une exécution, les chronomètres et l’historique restent conservés.

`public/views/board.mjs`, `projects.mjs`, `processes.mjs` et `project-settings.mjs` reçoivent explicitement l’état et les actions de l’interface. Ils possèdent le rendu et les événements du tableau, de la liste des projets, des processus et des réglages de projet. `public/app.js` conserve la coordination, les filtres globaux et les écrans de fiche, brainstorming et temps. Ce code reste dans la couche de présentation ; les décisions métier sont contrôlées côté application. Les tests couvrent les cas d’usage avec un stockage en mémoire, les routes avec SQLite temporaire et les événements des vues avec des doubles DOM.

## Autres cas d’usage et points de composition

- `application/runs.mjs` orchestre les agents, leurs événements, les limites du journal et les résultats. Les exécutables, worktrees, identifiants et transformations d’événements lui sont injectés.
- `application/card-workflows.mjs` expose les commandes des cartes : préparation, brainstorming, Jira, PR, review, worktrees, services et file. `http/card-workflow-routes.mjs` traduit les méthodes HTTP en commandes ; l’application ne reçoit pas de requête HTTP.
- `application/jira-sync.mjs` conserve le verrou de synchronisation, les règles de correspondance et les mises à jour transactionnelles. L’adaptateur `infrastructure/sync-repository.mjs` fournit la transaction SQLite.
- `application/settings.mjs` valide les réglages et conserve les identifiants hors des réponses publiques. Une transaction enregistre ensemble réglages et coffre chiffré. Les jetons en mémoire changent uniquement après succès.
- `application/automation.mjs` porte le parcours automatique, avec vérification, publication, review et délai injectés. `lib/automation.mjs` conserve l’ancienne interface de composition.
- `application/time-tracking.mjs` porte le chronomètre et les protections contre les doubles envois Tempo. `domain/time.mjs` valide les saisies et `http/time-routes.mjs` traduit les routes. `lib/time-tracking.mjs` compose les adaptateurs historiques.

Les modules historiques de Git, Jira, Azure, Tempo, stockage et processus restent les adaptateurs concrets. `server.mjs` et les petits modules de compatibilité les assemblent ; les cas d’usage n’importent pas leurs implémentations. Les réexports évitent une migration simultanée de tous les consommateurs. `test/architecture.test.mjs` interdit les imports vers l’extérieur dans le domaine et l’application, ainsi que l’accès direct à HTTP, SQLite et aux variables système.

## Windows, Linux et macOS

Node.js et SQLite restent adaptés à une application locale multiplateforme. Le domaine et les cas d’usage doivent rester indépendants du système ; le lancement de processus, le stockage sécurisé des jetons, les chemins et l’installation relèvent des adaptateurs.

La chaîne de publication produit Windows x64, Linux x64/ARM64 et macOS Intel/Apple Silicon, chacun avec son runtime Node. GitHub Releases sélectionne le paquet correspondant au système et à l’architecture. Le manifeste est vérifié avant remplacement ; le même worker restaure les fichiers et les données en cas d’échec. Windows utilise un ZIP contrôlé par PowerShell, Linux/macOS un tar.gz au format ustar restreint, sans liens ni chemins sortants.

`domain/paths.mjs` compare les dépôts selon le système fourni ; le serveur utilise les chemins réels. La présentation reçoit la plateforme dans les réglages publics et applique la même règle. Windows compare sans tenir compte de la casse ; POSIX conserve la casse des chemins réels. Le trousseau utilise DPAPI sous Windows, le Trousseau d’accès sous macOS et Secret Service via `secret-tool` sous Linux. Les jetons POSIX sont chiffrés par AES-256-GCM, avec une clé conservée uniquement dans le trousseau. Sans trousseau disponible, leur sauvegarde échoue sans repli en clair.

La compatibilité native doit être validée par la matrice GitHub avant diffusion. Les tests avec doubles système ne prouvent ni le comportement d’un trousseau interactif, ni celui de toutes les distributions Linux.

## Migration du reste du projet

Les prochaines évolutions de présentation peuvent extraire la fiche, le brainstorming et le suivi du temps de `public/app.js` lorsqu’elles touchent ces écrans. Garder l’état et les actions explicites, les routes existantes, la protection locale, les validations et les tests. Les comportements système réels et les intégrations externes doivent être vérifiés sur chaque plateforme avant diffusion.

Chaque extraction doit laisser une application utilisable. Une migration de données ne se mélange pas à un déplacement de code. La version du format de données (`dataVersion`) est actuellement 1 ; l’installation automatique refuse un paquet d’un autre format. Une future évolution du format nécessite d’abord des migrations testées et une politique de restauration explicite.
