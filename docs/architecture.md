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

Les modules de `public/views/` reçoivent explicitement l’état et les actions de l’interface. `card.mjs` possède la sélection et le journal de la fiche ; `brainstorm.mjs` possède la discussion et ses brouillons ; `time.mjs` possède les filtres, les formulaires et les chronomètres affichés. `task-editor.mjs` partage les champs d’agent et les brouillons entre création et édition. Le tableau, les projets et les processus conservent leurs vues dédiées. `public/app.js` compose ces vues et conserve la navigation et les filtres globaux. Les décisions métier restent contrôlées côté application. Les tests couvrent les cas d’usage avec un stockage en mémoire, les routes avec SQLite temporaire et les événements des vues importées directement avec des doubles DOM ; ils n’exécutent plus de fragments découpés dans le source.

`task-workspace.mjs` organise une page de tâche intégrée à la navigation, avec les onglets tâche, activité, diff et fichiers. La navigation conserve le menu latéral et une URL de tâche rechargeable. Le diff utilise une liste compacte avec des icônes de statut ; les fichiers se parcourent dans une arborescence de dossiers dépliables. Les onglets conservent les formulaires montés pour préserver les saisies. Les lectures passent par les workflows de cartes puis `infrastructure/task-files.mjs` : le diff compare le worktree au point commun avec son commit de départ, et inclut les modifications locales et fichiers non suivis. L’explorateur lit le worktree, ou le dépôt source en son absence, sans fichiers ignorés ni lecture de liens symboliques. La lecture directe d’un fichier est bornée à 512 Kio et les aperçus affichent au plus 10 000 lignes ; aucune commande ne modifie le dépôt.

L’inspection vérifie la branche active et le répertoire Git commun avec le dépôt source avant de lire un worktree. Une suppression indexée et un fichier recréé non suivi restent deux entrées distinctes : le paramètre `untracked` de l’aperçu identifie la version sélectionnée. Chaque inspection neutralise `core.fsmonitor` et les programmes de filtres Git configurés (`clean`, `smudge`, `process`, `required`) par des options de commande, sans réécrire la configuration ni l’index. Les diffs externes et `textconv` sont également désactivés. Les fichiers utilisant LFS ou des filtres personnalisés sont donc comparés sans ces conversions ; l’aperçu peut différer du diff habituel du développeur. Ces protections ne constituent pas une sandbox contre un processus local qui remplace simultanément les fichiers ou la configuration Git.

## Autres cas d’usage et points de composition

- `application/runs.mjs` orchestre les agents, leurs événements, les limites du journal et les résultats. Les exécutables, worktrees, identifiants et transformations d’événements lui sont injectés.
- `application/card-workflows.mjs` orchestre les commandes communes des cartes et appelle les fournisseurs sans leurs secrets ni leur protocole. `application/card-worktrees.mjs` gère les worktrees et services locaux. `lib/jira-card-workflows.mjs` conserve la création et les transitions propres à Jira ; les routes historiques restent disponibles.
- `application/jira-sync.mjs` conserve le verrou de synchronisation, les règles de correspondance et les mises à jour transactionnelles. L’adaptateur `infrastructure/sync-repository.mjs` fournit la transaction SQLite.
- `application/settings.mjs` valide les réglages et conserve les identifiants hors des réponses publiques. Une transaction enregistre ensemble réglages et coffre chiffré. Les jetons en mémoire changent uniquement après succès.
- `application/agent-providers.mjs` gère les profils, leurs références et le verrou de mise à jour. Les diagnostics, l’installation et la notification des changements sont injectés par `server.mjs` ; les règles de validation et de comparaison des versions restent dans le domaine. `http/agent-provider-routes.mjs` traduit les requêtes sans lancer de processus.
- Les adaptateurs `agent-account.mjs`, `claude-account.mjs` et `agent-updates.mjs` portent les protocoles et commandes des fournisseurs. `agent-credentials.mjs` partage la lecture locale bornée des fichiers de connexion. La vue `public/views/agent-providers.mjs` sépare le rendu des quotas et de l’installation de l’action asynchrone de mise à jour.
- `application/automation.mjs` porte le parcours automatique, avec vérification, publication, review et délai injectés. `lib/automation.mjs` conserve l’ancienne interface de composition.
- `application/time-tracking.mjs` gère les saisies et le chronomètre local avec une horloge injectée. `lib/tempo-export.mjs` porte les appels Jira/Tempo et les protections contre les doubles envois. `lib/time-tracking.mjs` compose ces services et le minuteur ; `http/time-routes.mjs` traduit les routes.

Les modules historiques de Git, Jira, Azure, Tempo, stockage et processus restent les adaptateurs concrets. `server.mjs` et les petits modules de compatibilité les assemblent ; les cas d’usage n’importent pas leurs implémentations. Les réexports évitent une migration simultanée de tous les consommateurs. `test/architecture.test.mjs` interdit les imports vers l’extérieur dans le domaine et l’application, ainsi que l’accès direct à HTTP, SQLite et aux variables système.

## Windows, Linux et macOS

Node.js et SQLite restent adaptés à une application locale multiplateforme. Le domaine et les cas d’usage doivent rester indépendants du système ; le lancement de processus, le stockage sécurisé des jetons, les chemins et l’installation relèvent des adaptateurs.

La chaîne de publication produit Windows x64, Linux x64/ARM64 et macOS Intel/Apple Silicon, chacun avec son runtime Node. GitHub Releases sélectionne le paquet correspondant au système et à l’architecture. Le manifeste est vérifié avant remplacement ; le même worker restaure les fichiers et les données en cas d’échec. Windows utilise un ZIP contrôlé par PowerShell, Linux/macOS un tar.gz au format ustar restreint, sans liens ni chemins sortants.

`domain/paths.mjs` compare les dépôts selon le système fourni ; le serveur utilise les chemins réels. La présentation reçoit la plateforme dans les réglages publics et applique la même règle. Windows compare sans tenir compte de la casse ; POSIX conserve la casse des chemins réels. Le trousseau utilise DPAPI sous Windows, le Trousseau d’accès sous macOS et Secret Service via `secret-tool` sous Linux. Les jetons POSIX sont chiffrés par AES-256-GCM, avec une clé conservée uniquement dans le trousseau. Sans trousseau disponible, leur sauvegarde échoue sans repli en clair.

La compatibilité native doit être validée par la matrice GitHub avant diffusion. Les tests avec doubles système ne prouvent ni le comportement d’un trousseau interactif, ni celui de toutes les distributions Linux.

## Migration du reste du projet

Les prochaines évolutions de présentation doivent conserver l’état et les actions explicites, les routes existantes, la protection locale, les validations et les tests. Les comportements système réels et les intégrations externes doivent être vérifiés sur chaque plateforme avant diffusion.

Chaque extraction doit laisser une application utilisable. Une migration de données ne se mélange pas à un déplacement de code. La version du format de données (`dataVersion`) est actuellement 2 ; l’installation automatique refuse un paquet d’un autre format. Une future évolution du format nécessite d’abord des migrations testées et une politique de restauration explicite.
