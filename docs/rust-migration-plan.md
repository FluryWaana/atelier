# Plan de migration complète vers Rust

Date : 23 octobre 2026. Référence examinée : commit `843d3ea`, version applicative `0.1.9`.

Statut : **M00 validé comme référence locale Windows avec exclusions documentées ; M01 validé pour la faisabilité, vraie réouverture requalifiée dans les quatre navigateurs ; M02 en cours pour les données et protections ; M03 en cours, règles pures, duplication et chronomètres locaux portés**. La [matrice de parité](rust-migration-parity.md) et les relevés [M00](rust-migration-baseline.md), [M01](rust-migration-m01.md), [M02](rust-migration-m02.md) et [M03](rust-migration-m03.md) consignent les preuves et leurs limites. Relever à nouveau le commit, l'état de travail et les versions au début de chaque incrément.

## 1. Résultat attendu et périmètre

Migrer tout le code applicatif maintenu par Atelier vers Rust : serveur, domaine, cas d'usage, stockage, intégrations, interface, lanceur, icônes système, worker de mise à jour, outils de fabrication et tests applicatifs. Conserver les fonctionnalités, les données, le français et l'anglais, ainsi que les cinq cibles natives actuellement livrées.

L'interface reste dans le navigateur et devient du Rust compilé en WebAssembly. Conserver HTML, CSS/SCSS, images et traductions comme ressources ; le JavaScript de liaison produit par les outils WebAssembly est un artefact généré, pas une seconde implémentation à maintenir.

Le résultat final ne requiert Node ni npm pour développer, construire, tester ou exécuter Atelier. Cette exigence concerne Atelier : Git, les CLI d'agents et les projets lancés par l'utilisateur gardent leurs dépendances. Les composants externes, SQLite, le compilateur Sass et les API système ne sont pas à réécrire en Rust. Les actions hébergées de CI peuvent avoir leur propre runtime.

Les fichiers déclaratifs d'installation (`.iss`, `.spec`, `.desktop`), YAML et les petits points d'entrée imposés par Windows ou les gestionnaires de paquets peuvent rester dans leur format natif. Toute logique applicative de ces points d'entrée doit appeler les commandes Rust. Retirer les helpers C#, Swift, Python et PowerShell propres au produit une fois leurs remplaçants qualifiés.

Ne pas ajouter de nouvelle interface native, de serveur distant, de synchronisation cloud, de nouvelles intégrations ou de refonte visuelle pendant ce chantier. La production continue d'utiliser la version Node jusqu'à qualification complète de la version Rust.

## 2. Constats qui conditionnent la migration

| Constat dans le dépôt | Conséquence |
| --- | --- |
| `lib/domain/release-package.mjs` déclare `dataVersion = 2` | Conserver ce format ; la mention contradictoire de version 1 dans `docs/architecture.md` a été corrigée en M00. |
| `lib/store.mjs` conserve principalement des objets JSON dans SQLite | Préserver schéma, identifiants, champs absents/null, champs additionnels, ordre observable et formats temporels. |
| `lib/infrastructure/connector-loader.mjs` importe des modules ES | Une extension JavaScript ne devient pas un plugin Rust automatiquement ; prévoir un nouveau contrat et un diagnostic de compatibilité. |
| L'updater exige les entrées Node, `package.json` et un runtime embarqué | La première livraison Rust exige une transition testée ; le paquet Rust ne peut pas remplacer directement l'ancien paquet. |
| La signature utilise `atelier-release-v1` et un JSON précis | Préserver les octets signés et la clé publique existante ; ne pas changer le protocole cryptographique implicitement. |
| Le coffre utilise DPAPI ou une enveloppe AES-GCM et un trousseau système | Préserver les encodages, noms de services/comptes et paramètres exacts, pas uniquement le choix de l'algorithme. |
| `installation.json` distingue portable, installation utilisateur et système | Préserver les emplacements de données et les règles de mise à jour propres à chaque mode. |
| Tray Windows en C#, macOS en Swift, Linux en Python | Porter également ces composants et leurs tests graphiques ; compiler le serveur ne suffit pas. |
| La matrice actuelle produit cinq archives et cinq installateurs | Garder Windows x64, Linux x64/ARM64, macOS x64/ARM64 ; Setup Windows, DEB/RPM Linux, macOS portable. |

Sources locales : [architecture](architecture.md), [connecteurs](connectors.md), [livraisons](releases.md), [fournisseurs d'agents](ai-providers.md), [dépôts](repository-connectors.md), [traductions](localization.md), [workflow de release](../.github/workflows/release.yml).

## 3. Architecture retenue

Créer un workspace Cargo avec quatre crates, sans multiplier les bibliothèques par fonctionnalité :

```text
Cargo.toml / Cargo.lock / rust-toolchain.toml
crates/
  atelier-core/src/
    domain/                 règles pures
    application/            cas d'usage et ports nécessaires aux tests
    contracts/              DTO publics, sans secrets
  atelier-app/src/
    main.rs                 serveur, start, stop, import, tray, update-worker
    http/                   routes et protections HTTP
    infrastructure/         SQLite, fichiers, processus, trousseaux, OS
    connectors/             intégrations fournies et hôte des extensions
  atelier-ui/src/           composants Leptos, état et navigation
xtask/                      fabrication, signatures, vérifications, paquets
```

Les sous-commandes exactes seront alignées sur les usages existants en M01. Le même exécutable peut être lancé comme tray ou worker dans un autre processus ; pour remplacer un binaire Windows actif, exécuter le worker depuis une copie dans le répertoire de staging. Aucun nouveau service permanent séparé n'est nécessaire.

Décisions techniques :

- Axum et Tokio pour HTTP, événements et processus asynchrones. Axum est conçu pour Tokio : [documentation](https://docs.rs/axum/latest/axum/).
- `rusqlite` avec SQLite embarqué et sauvegardes pour conserver les données : [documentation](https://docs.rs/rusqlite/latest/rusqlite/). Une connexion détenue par un thread dédié, avec file bornée, évite de bloquer les tâches Tokio. Chaque transaction reste une opération indivisible ; aucun aller-retour réseau au milieu d'une transaction SQL.
- Serde pour les échanges et les documents persistés ; modèles publics séparés des paramètres secrets. Les formats Rust ne doivent pas modifier les noms et valeurs sur le fil.
- Leptos en rendu client, avec Trunk pour fabriquer les assets : [déploiement CSR](https://book.leptos.dev/deployment/csr.html). Le serveur embarque ou distribue ces assets sans serveur de développement en production.
- Bibliothèques maintenues pour HTTP sortant et cryptographie ; sélectionner et verrouiller leurs versions en M01, après vérification des licences, des cibles et des fonctions effectivement utilisées. Ne pas implémenter les primitives cryptographiques.
- APIs système ou bindings Rust pour les trousseaux, les fenêtres de sélection et les trays. Valider les backends Linux et les contraintes de thread principal macOS dès M01.
- Git reste exécuté via le CLI existant ; pas de remplacement par une nouvelle implémentation Git pendant le portage.
- Sass autonome pour conserver les SCSS, avec sources Bootstrap figées et licences conservées ; pas de nouveau système graphique. [Distribution Sass autonome](https://sass-lang.com/install/).

`atelier-core` ne dépend pas des adaptateurs, de Tokio, de HTTP ou de SQLite. L'interface ne dépend que des contrats publics et des règles pures utiles ; conditionner la compilation des cas d'usage serveur si nécessaire. Les secrets et adaptateurs natifs ne doivent jamais entrer dans le bundle WebAssembly. Préserver des interfaces petites ; aucun conteneur générique d'injection.

Le backend Rust reste seul responsable des validations et transitions. Les invariants de concurrence actuels doivent être conservés explicitement : le passage d'une boucle Node à plusieurs tâches Rust ne fournit pas automatiquement les mêmes garanties.

### Règles de conception et de revue : Clean Code, SOLID, KISS, DRY, YAGNI

Ces principes s'appliquent à chaque lot, aux tests et aux outils de livraison. Ils doivent produire du code plus lisible et plus simple à modifier, sans ajouter de couches pour satisfaire un modèle théorique.

- **Clean Code** : noms explicites, fonctions cohérentes autour d'une responsabilité, effets de bord visibles et erreurs contextualisées. Commenter les raisons et invariants difficiles à déduire du code. Éviter les booléens ambigus, valeurs magiques et états invalides représentables lorsque des types simples les éliminent. Aucune limite arbitraire de lignes n'impose un découpage artificiel.
- **SOLID — responsabilité unique (S)** : séparer règles métier, orchestration, transport, stockage et rendu selon les frontières du dépôt. Un module doit avoir une raison cohérente de changer.
- **SOLID — ouvert/fermé (O)** : utiliser les points d'extension réellement nécessaires aux fournisseurs et adaptateurs. Ajouter une implémentation sans dupliquer les workflows ; ne pas créer de framework d'extension pour des besoins hypothétiques.
- **SOLID — substitution (L)** : les adaptateurs et doubles respectent les mêmes contrats, erreurs, invariants et garanties transactionnelles. Les tests de contrat vérifient cette substituabilité.
- **SOLID — ségrégation des interfaces (I)** : chaque cas d'usage reçoit uniquement les opérations dont il a besoin. Éviter les traits géants et objets donnant accès à tous les services.
- **SOLID — inversion des dépendances (D)** : domaine et cas d'usage définissent leurs besoins ; les adaptateurs les implémentent et le point d'entrée assemble les dépendances. Employer traits, fonctions ou types concrets selon le besoin réel, sans imposer de hiérarchie orientée objet en Rust.
- **KISS** : retenir la solution la plus simple qui couvre entièrement les comportements et cas d'erreur. Préférer composition, structures explicites et outils existants aux macros, génériques ou couches supplémentaires sans bénéfice démontré.
- **DRY** : une règle métier ou un contrat possède une source de vérité. Mutualiser les duplications sémantiques avérées ; deux fragments qui se ressemblent mais évoluent pour des raisons différentes ne justifient pas une abstraction commune. La coexistence Node/Rust est transitoire et se termine en M11.
- **YAGNI** : aucune option, dépendance, abstraction ou fonctionnalité « pour plus tard ». Toute nouvelle couche doit répondre à un besoin de ce plan, d'un appelant existant ou d'une validation nécessaire.
- **Rust idiomatique** : utiliser `Result` et une propagation d'erreurs explicite aux frontières ; éviter `unwrap`/`expect` sur des données externes et chemins de production récupérables. Justifier les blocs `unsafe` par leurs invariants de sûreté ; ne pas ajouter de clones, conversions ou allocations pour contourner un problème de conception sans examiner la solution simple.

**Critère de sortie de chaque lot :** relire les appelants et tests concernés, vérifier les responsabilités et dépendances, supprimer le code devenu inutile et justifier chaque nouvelle abstraction ou dépendance. Corriger les violations relevées avant de valider le lot. Rustfmt et Clippy complètent cette revue ; ils ne prouvent ni SOLID ni la simplicité. La réduction du code ne doit jamais supprimer validation, sécurité, accessibilité ou gestion d'erreur nécessaire.

## 4. Compatibilité et invariants non négociables

### Données et restauration

- Aucun développement ni test destructif sur `data/` utilisateur. Utiliser des bases synthétiques et des répertoires temporaires séparés.
- Ne jamais démarrer Node et Rust sur le même répertoire de données. M02 définit et teste un verrou commun d'instance et de données, acquis avant l'ouverture de SQLite et conservé jusqu'à sa fermeture ; M09 l'intègre à N1. Tester deux ports et deux chemins aliasant le même dossier. N0 ne respecte pas ce nouveau verrou : la migration depuis N0 exige son arrêt vérifié avant accès aux données ; un verrou Rust seul ne suffit pas. Les essais de coexistence utilisent toujours des copies séparées.
- Conserver `dataVersion = 2` uniquement tant que la compatibilité est prouvée. Toute nécessité de changer le format devient un lot de migration explicite avec historique et restauration, pas un ajustement caché.
- Tester Node → Rust → Node sur la même copie de données, avec arrêt entre exécutions. Les champs additionnels sont conservés à la réécriture ; les anciennes valeurs optionnelles ne sont pas normalisées sans nécessité.
- Sauvegarder SQLite par son API de sauvegarde, avec état WAL cohérent. Une copie à chaud du seul fichier `.sqlite` est insuffisante.
- Conserver l'historique, les temps, les états d'export, les liens de tickets/PR et les chemins des projets. La restauration utilisateur conserve la règle actuelle d'effacement des identifiants ; la restauration transactionnelle d'une mise à jour conserve sa sémantique distincte.
- Une restauration automatique après échec a lieu avant réouverture aux utilisateurs. Après utilisation réussie d'une nouvelle version, revenir à un ancien instantané pourrait perdre des écritures : ne pas promettre ce retour arrière sans perte.

### Sécurité et système

- Écouter uniquement sur loopback ; conserver Host/Origin, preuve HMAC, session, protections des mutations et limites de requêtes.
- Garder les secrets hors des réponses publiques, journaux, arguments de processus visibles et erreurs utilisateur. Tester explicitement les chemins SSE et d'ouverture du navigateur.
- Reproduire les garanties de lecture par descripteur, permissions, chemins canoniques et liens symboliques. Inclure les jonctions/reparse points Windows et les courses de remplacement de fichiers.
- Conserver la neutralisation des filtres Git, diff externe et `textconv` pour l'explorateur. Conserver les bornes de taille et les vérifications du worktree.
- Ne pas lancer de shell pour concaténer des arguments non fiables. Les commandes personnalisées explicitement configurées conservent leurs règles actuelles.
- Refuser une sauvegarde de secrets si le trousseau est indisponible ; aucun repli en clair. Vérifier les coffres sur le même compte OS, sans promettre leur portabilité entre systèmes.
- Conserver l'arrêt contrôlé, les restrictions pendant opérations occupées, l'absence d'effacement des données à la désinstallation et le respect d'APT/DNF.

### Comportements observables

- Maintenir routes, méthodes, codes HTTP, noms JSON, messages traduits et événements pendant le remplacement du backend.
- Maintenir les brouillons, URL rechargeables, confirmations, actions clavier et restauration du focus.
- Conserver les états `sending`, `exported`, `uncertain` et le rapprochement avant réessai. Un timeout ne prouve pas qu'une écriture distante a échoué.
- Une réexécution de workflow après crash ne doit pas relancer aveuglément une publication, une transition distante ou un agent.

## 5. Contrat des extensions

La cible est un manifeste déclaratif et un exécutable par plateforme, communiquant avec l'hôte via JSON-RPC 2.0 sur stdin/stdout, un message JSON par ligne. Les connecteurs fournis sont des modules Rust intégrés ; les extensions installables utilisent le même contrat fonctionnel via un adaptateur de processus.

En M05, spécifier seulement les capacités existantes : tickets, dépôts, temps, diagnostics, paramètres, traductions et actions déclaratives. Préserver les identifiants de connecteurs, références aux secrets et fournisseurs historiques. Aucune ABI Rust dynamique, magasin de plugins ou runtime JavaScript embarqué.

Le contrat inclut :

1. Version de protocole et négociation de capacités ; rejet propre d'une version inconnue.
2. Identifiants de requêtes, réponses et erreurs ; gestion des réponses tardives, doublons et JSON invalide.
3. Taille maximale des messages, délais, annulation, fin de flux, arrêt et crash. Stdout réservé au protocole ; stderr borné et traité comme potentiellement sensible.
4. Méthodes explicites permettant les appels retour utiles vers les cas d'usage (`tickets`, `tasks`, `projects`, `time`, notification), avec portée de connecteur vérifiée par l'hôte. Aucun accès brut à SQLite.
5. Validation des manifestes, doublons d'identifiants, emplacement de l'exécutable et choix OS/architecture. Installation locale explicite, sans téléchargement automatique.
6. Conservation des dépendances de secrets, de `credentialScope`, des validations de paramètres et des traductions. Paramètres et résultats restent rendus comme du texte côté navigateur.

Ces processus restent du code de confiance exécuté sous le compte utilisateur ; le protocole n'est pas une sandbox. Les secrets envoyés se limitent au fournisseur et à l'opération concernés, sans prétendre isoler un exécutable malveillant du compte local.

Les anciens `connectors/*/index.mjs` sont incompatibles avec la cible. La transition doit les inventorier avant bascule, préserver leurs fichiers et réglages et afficher un diagnostic actionnable. Bloquer la mise à jour automatique si un connecteur actif ou requis par l'historique n'a pas de remplaçant compatible. Une migration manuelle peut conserver un fournisseur indisponible, mais ne doit jamais effacer son historique. Livrer le portage de `examples/connectors/demo` et un guide précis de conversion ; ne pas prétendre convertir automatiquement du JavaScript arbitraire.

## 6. Transition entre les formats de livraison

Trois états sont à distinguer : N0 (version Node actuelle), N1 (version Node de transition) et R1 (première version Rust qualifiée).

- N1 reste un paquet accepté par N0 et conserve son application Node. Son updater sait vérifier les deux formats et lancer la transition vers R1.
- R1 porte un format de paquet explicite et une version minimale de l'updater, distincts de `dataVersion`. N1 valide un ensemble strict d'entrées pour chaque format ; ne pas simplement autoriser des fichiers arbitraires.
- N0 ne sait pas nécessairement lire ces nouveaux champs ni trouver N1 une fois R1 devenu la dernière release. Ne pas supposer un passage automatique par toutes les versions : documenter l'installation explicite de N1 ou la migration manuelle depuis N0. Tester ce cas avant diffusion.
- Le contenu signé conserve exactement le protocole `atelier-release-v1` : ordre des champs JSON, encodage UTF-8, normalisation et nom d'archive. Le manifeste fait partie de l'archive dont l'empreinte est signée. Utiliser des vecteurs croisés Node/Rust et une clé de test ; jamais la clé privée de publication.
- Préserver la vérification HTTPS/redirections, empreinte, signature, dépôt, plateforme, architecture, espace disponible et restrictions d'archives. Refuser chemins sortants, liens, doublons ambigus et volumes excessifs.
- Utiliser un journal durable des étapes et un worker indépendant. Sur reprise, vérifier l'identité des processus avant toute action ; ne pas utiliser un PID seul comme preuve.
- Après arrêt et sauvegarde, remplacer seulement les entrées applicatives autorisées. Garder données, extensions et marqueur d'installation hors du remplacement. Conserver l'ancien runtime jusqu'à validation ou restauration de la transition.
- Le nouveau serveur démarre en phase de validation fermée aux mutations utilisateur. Vérifier identité, version et données attendues ; terminer la transaction puis ouvrir l'application. En cas d'échec, arrêter le candidat avant restauration et relancer la version précédente.
- Tester coupure après chaque étape durable, panne disque, fichier verrouillé, échec de démarrage et restauration elle-même interrompue. Un échec de restauration doit laisser un état diagnostiquable et une procédure de reprise.
- Linux installé reste mis à jour par ses paquets. Les hooks de paquets doivent reconnaître à la fois les anciens processus Node et les nouveaux processus Rust, y compris ceux d'autres sessions.

Une version de transition ne doit être publiée qu'après validation de la chaîne complète. Ce plan n'autorise pas à publier une release, pousser un tag ou utiliser les données réelles.

## 7. Ordre d'exécution et critères de sortie

Les lots sont découpables en petites PR. Les dépendances désignent les critères nécessaires avant validation d'un lot, pas une autorisation de supprimer des fonctions encore manquantes. Une exploration limitée d'un risque peut avoir lieu avant le lot complet.

| Lot | Dépendances | Résultat démontrable | Statut |
| --- | --- | --- | --- |
| M00 | — | Référence fonctionnelle et matrice de parité | Validé — référence Windows et exclusions dans le relevé M00 |
| M01 | M00 | Socle Rust et faisabilité des cinq cibles | Validé — preuves natives et quatre navigateurs, vraie réouverture corrigée et requalifiée |
| M02 | M01 | Données, secrets et session compatibles | En cours — UTF-16, coffres verrouillés/entre comptes et SSE prouvés ; ouverture SQLite et intégrations restantes ouvertes |
| M03 | M02 | Domaine et API métier équivalents | En cours — règles pures, duplication/archivage/suppression/modèles HTTP et chronomètres locaux ; autres cas d'usage/API et prérequis M02 non clos |
| M04 | M03 | Git, processus et agents équivalents | À faire |
| M05 | M03, M04 | Connecteurs et protocole d'extensions | À faire |
| M06 | M03, M04, M05 | Automatisation et reprise cohérentes | À faire |
| M07 | M02 à M06 | Interface complète Rust/WASM | À faire |
| M08 | M02, M04 | Lanceur, trays et intégration OS | À faire |
| M09 | M05, M06, M07, M08 | Updater et transition N0/N1/R1 | À faire |
| M10 | M07, M08, M09 | Paquets et contrôles natifs complets | À faire |
| M11 | M00 à M10 | Qualification finale et retrait du code ancien | À faire |

### M00 — Référence et inventaire exhaustif

Lire les instructions du dépôt et relever son état sans écraser de modifications. Exécuter la suite Node complète avec le runtime épinglé, les vérifications de release applicables et relever les échecs préexistants. Ne pas lancer les tests d'installation sur le compte de travail.

Créer `docs/rust-migration-parity.md` : une ligne par fonctionnalité et contrat observable, avec sources actuelles, appels/imports, tests actuels, équivalent Rust, scénarios d'erreur, preuve et statut. Couvrir chaque module applicatif et chaque test ; l'inventaire ci-dessous est un point de départ, pas une preuve d'exhaustivité.

Créer des fixtures synthétiques, des faux CLI et serveurs HTTP locaux. Inventorier aussi les variables d'environnement, options CLI, clés de stockage navigateur, routes exposées depuis `server.mjs`, sélecteurs utilisés par les vues, formats de journaux et fichiers de session.

Mesurer sur une configuration documentée : démarrage à froid/chaud jusqu'à serveur prêt et interface utilisable, mémoire au repos, taille installée/archives, chargement d'un tableau et suivi de logs. Fixer les scénarios, taille des jeux de données et budgets de régression avant comparaison ; aucun gain n'est présumé. Établir une estimation des lots après cet inventaire, pas à partir du nombre de fichiers.

**Sortie :** suite de référence exécutée, exceptions identifiées, matrice sans domaine oublié, fixtures sans secrets, mesures reproductibles. Un échec existant n'est ni ignoré ni attribué arbitrairement à la migration.

### M01 — Workspace, dépendances et premières preuves de faisabilité

Créer les quatre crates, fixer toolchain, lockfile et outils WASM ; définir l'alias `cargo xtask` dans `.cargo/config.toml`. Adapter `AGENTS.md` pour documenter la coexistence Rust/Node et conserver ses invariants. Ajouter formatage, Clippy, tests et compilation WASM à la CI sans retirer les contrôles existants. Garder le paquet officiel Node intact.

Construire une petite tranche complète sur données temporaires : interface Rust, appel local authentifié, création d'une tâche, lecture après redémarrage. Les éléments de stockage ou session introduits ici restent limités et doivent être durcis en M02 avant tout usage réel.

Prouver tôt : compilation/exécution sur les cinq cibles ; chargement WASM sous CSP sans ouvrir largement les scripts ; tray sur les trois OS ; accès à des secrets de test ; lancement/arrêt d'un descendant ; remplacement d'un exécutable Windows via worker séparé. Les cibles indisponibles localement restent à valider en CI ou sur machines de test, jamais déclarées acquises.

**Sortie :** parcours vertical démontré, dépendances justifiées et verrouillées, risques de plateforme documentés avec preuves. Pas de publication du prototype.

### M02 — Stockage, coffre et session locale

Porter `store`, repositories, transactions, backups, import portable, installation, token-vault, local-session, local-http et checked-file. Implémenter le modèle de connexion SQLite défini plus haut, des erreurs explicites et une file bornée.

Tester documents anciens et additionnels, absence/null, précision des temps, ordre de lecture, logs bornés, sauvegarde avec WAL, base invalide, disque plein, transaction annulée, coffre verrouillé, compte différent et protections de fichiers. Reproduire exactement les identifiants du trousseau existant avec des secrets de test dédiés.

**Sortie :** aller-retour Node/Rust/Node sans perte sur fixtures, tests OS réels des coffres et refus des accès HTTP non autorisés. Aucun secret exposé au frontend ou aux logs.

### M03 — Domaine, cas d'usage et API

Porter les règles de `lib/domain/`, projets, tâches, réglages, profils d'agents, saisies et chronomètres locaux, puis leurs routes. Conserver interfaces d'erreur, valeurs par défaut et sérialisation. Définir des sections critiques courtes pour réserver une opération et revalider son état après les I/O, sans maintenir un verrou global pendant un appel distant.

Faire fonctionner l'interface actuelle avec le backend Rust sur un port de test et des données séparées. Les routes de fonctions non encore portées retournent une indisponibilité explicite ; aucune réponse de succès factice.

**Sortie :** chaque contrat de ce lot a un test équivalent ; scénarios HTTP comparés à Node, dépendances/blocages/chronomètres conservés et mutations concurrentes cohérentes.

### M04 — Git, fichiers, processus et agents

Porter worktrees, contexte agent, inspection de fichiers/diffs, launch-detection, ports, dev-servers, diagnostics, vérification, exécution d'agents, interactions, usage/quota, installation/mise à jour des CLI et lecture des comptes. Préserver les commandes configurables et comportements Windows/POSIX.

Assurer lecture simultanée stdout/stderr, limites de lignes/volume, timeout, annulation, attente de terminaison et nettoyage des descendants. Tester les groupes de processus POSIX et la stratégie Windows, y compris le cas où un processus se détache. Les garanties réellement possibles doivent être documentées. Préserver l'écriture des configurations de contexte sans écraser celles de l'utilisateur.

**Sortie :** dépôts temporaires et faux agents couvrent succès, crash, débit élevé, entrée invalide, arrêt et redémarrage. Aucun appel d'agent payant. Aucun descendant du scénario de test encore vivant après l'arrêt complet.

### M05 — Intégrations et extensions

Porter connecteurs intégrés Jira, Azure DevOps, GitHub, GitLab et Tempo, leurs adaptateurs et routes. Implémenter le contrat de processus de la section 5 avec un connecteur de démonstration compilé.

Vérifier pagination, erreurs d'authentification, délais, limitation de débit, changements d'instance, collisions d'identifiants, secrets invalidés, fournisseurs par projet, fournisseurs historiques et désactivation de module. Les opérations d'écriture distante ne sont pas réessayées automatiquement lorsqu'elles peuvent avoir réussi.

**Sortie :** tests de contrat sur services simulés, extension installable fonctionnelle, manifeste erroné non bloquant au démarrage, diagnostic d'extension ancienne et guide de portage. Couvrir les appels retour de l'extension vers les cas d'usage, pas seulement un ping.

### M06 — Workflows, synchronisation et reprises

Porter card-workflows, publication/revue de PR, Jira sync, brainstorm, préparation de tickets, file de tâches, automatisation et export/réconciliation du temps. Conserver l'ordre des effets, verrous et notifications observables.

Tester publication concurrente, changement de paramètres pendant une opération, tâche archivée/supprimée, échec de sauvegarde, crash avant/après effet externe, annulation et reprise au démarrage. Vérifier que la nouvelle concurrence Rust ne transforme pas une réservation Node en double exécution.

**Sortie :** parcours carte → préparation → agent → vérification → review → publication et parcours temps → envoi incertain → rapprochement validés de bout en bout avec doubles locaux.

### M07 — Interface Rust complète

Porter dans l'ordre : session/navigation/i18n/composants, aperçu/tableau/projets, édition et fiche de tâche, activité/diff/fichiers, agents/brainstorm, processus/automatisation, temps, paramètres/modules/mises à jour. Inclure les helpers de workflow, actions, coloration syntaxique et disponibilité des connecteurs.

Utiliser une seule source d'état par écran, en conservant les frontières actuelles des vues. Les deux interfaces peuvent être accessibles séparément en environnement de test ; ne pas ajouter une préférence utilisateur permanente pour la migration.

Préserver clés de brouillon et filtres, navigation arrière, URL de tâche, rafraîchissement, focus, dialogues, clavier, libellés, langues et textes échappés. Tester l'expiration de session, reconnexion des événements, erreurs réseau, flux rapides, gros logs et chargement différé des fichiers. Versionner les assets pour éviter les mélanges après mise à jour.

**Sortie :** mêmes parcours et captures comparables dans de vrais navigateurs ; tests WASM et scénarios E2E pilotés depuis Rust. Vérifier Edge/Chromium sous Windows, Firefox et Safari macOS ; ne pas confondre test DOM isolé et test de l'application complète. Les JS générés ne contiennent ni logique native ni secrets.

### M08 — Lanceur et intégration système

Porter `launcher.mjs`, ouverture du navigateur, sélection de dossier, import, détection d'instance, récupération de mise à jour, enregistrement de version Windows et composants tray. Remplacer `WindowsTray.cs`, `MacTray.swift`, `linux-tray.py` et `scripts/installers/AtelierKanban.cs` par leurs équivalents Rust.

Garder les commandes Start/Stop existantes comme points d'entrée minces. Couvrir chemins avec espaces/Unicode, port occupé par un tiers, processus obsolète, refus d'arrêt si occupé et absence de bureau/tray. Le tray utilise la même authentification que le lanceur et respecte les contraintes des boucles graphiques de chaque OS.

**Sortie :** portable et installation utilisateur testés ; menus ouvrir/quitter, aucune fenêtre console parasite sous Windows, fonctionnement local sans tray sous Linux, aucune fuite de session dans les arguments POSIX.

### M09 — Mise à jour et retour arrière

Implémenter la section 6, y compris le paquet N1 temporaire, le worker Rust et les modifications des contrôles de manifeste, archive et signature. Réutiliser le protocole de santé authentifié, sans ouvrir les mutations pendant la vérification du candidat.

Construire dès ce lot les paquets de test N1/R1/R2 avec manifeste, empreinte et signature par clé de test, en introduisant le minimum nécessaire de `xtask package`. Ces vrais paquets sont nécessaires aux critères de sortie de M09 ; M10 complète et qualifie la fabrication industrielle et les installateurs. Les essais du worker peuvent commencer avant M07, mais la validation de R1 attend l'interface complète. La vérification d'identité des processus et les coupures à chaque étape durable sont des garanties à ajouter : les contrôles actuels d'existence des PID et les tests de restauration Node ne les prouvent pas.

**Sortie :** scénarios N0 → N1 → R1, N1 → R1 en échec, R1 → R2 en échec et récupération après coupure exécutés sur paquets réels. Pour N0 face directement à R1, échec sûr et procédure manuelle vérifiés. Exécuter aussi les scénarios avec données externes au dossier applicatif et extensions conservées. Aucun mécanisme de transition non testé n'est livré.

### M10 — Fabrication, installateurs et contrôles de sécurité

Porter package, package-installers, sign-release, check-release et vérification SARIF dans `xtask` lorsqu'encore nécessaires. Remplacer l'extraction/compression PowerShell par les chemins Rust validés ; adapter les tests PowerShell et Python applicatifs. Les hooks système ne gardent que les appels indispensables aux commandes natives.

Produire les cinq archives et les cinq installateurs avec leurs dix empreintes et dix signatures. Conserver les noms attendus ou documenter explicitement leur transition. Le manifeste est la référence du paquet Rust ; ne plus dépendre de `package.json` pour son identité. Épingler les outils, vérifier leurs téléchargements et conserver les licences, y compris SQLite/Bootstrap et bibliothèques liées.

Mettre en place audit Rust des dépendances, vérification des licences, analyse statique adaptée et contrôles des workflows. Vérifier la couverture effective des outils au moment du choix ; Clippy n'est pas à lui seul une analyse de sécurité. Conserver les analyses JavaScript tant que N1 ou des sources JS restent livrées. Les contrôles échouent fermement si rapport ou artefact requis manque.

Tester install/update/uninstall dans comptes/VM/conteneurs dédiés : Windows sans installation existante, Ubuntu DEB et Fedora RPM pour x64/ARM64, macOS portable. Les tests natifs et les contrôles de publication restent obligatoires ; une compilation croisée seule ne suffit pas. Vérifier également les dépendances OS sur un poste sans outils de développement.

**Sortie :** matrice native verte, dix artefacts et métadonnées vérifiés, tests de restauration passés, CI PR/push sans publication, signature réelle réservée au job de release autorisé. Aucune suppression de garde de sécurité pour obtenir un build vert.

### M11 — Qualification et suppression de l'ancien code

Terminer la matrice de parité : chaque scénario ancien possède une preuve actuelle ou une décision explicite de rupture compatible avec ce plan. Corriger les différences restantes ; ne pas remplacer une fonctionnalité par un TODO.

Mesurer les performances dans les mêmes conditions que M00. Investiguer les régressions et renseigner les budgets retenus ; ne pas annoncer de gain non mesuré. Exécuter des parcours avec plusieurs agents simulés, synchronisation, chronomètre et navigateur reconnecté pendant une période prolongée.

Retirer les sources applicatives JS, runtime Node, helpers remplacés, scripts npm, lockfile npm et `.node-version` une fois les équivalences prouvées. L'historique Git et les fixtures documentées gardent la référence Node ; le produit final ne contient pas N1. Les tests de transition peuvent utiliser une release Node historique isolée comme fixture, sans exiger Node installé dans l'environnement final ; distinguer cette qualification historique des tests Rust courants.

Mettre à jour README, AGENTS, architecture, releases, connecteurs, agents, localisation, exemples, licences et instructions de contribution. Contrôler par inventaire tous les fichiers restant dans les anciens langages : justifier chaque ressource générée ou script natif minimal.

**Sortie :** checklist finale remplie avec preuves, environnement propre sans Node/npm, documentation exacte et procédure de retour arrière exploitable. Préparer les artefacts de release ; la publication effective reste une action distincte demandée par l'utilisateur.

## 8. Carte minimale de couverture

En M00, développer cette carte en matrice exhaustive. Les noms de tests ci-dessous sont des repères existants, pas une liste à recopier aveuglément.

| Domaine | Sources principales | Tests de référence |
| --- | --- | --- |
| Architecture / domaine | `lib/domain/`, `lib/application/`, `server.mjs` | `architecture`, `board-use-cases`, `project-rules`, `agent-options` |
| Projets / tâches | application tasks/projects, repositories, routes et vues | `card-*`, `project-*`, `workflow*`, `default-agent` |
| Données / secrets | store, backups, token-vault, installation, portable-import | `token-vault`, `installation`, `automation-backups`, `settings-use-cases` |
| HTTP / fichiers | local-session, local-http, checked-file, task-files | `http-security`, `local-http`, `browser-session`, `checked-file`, `file-read-races`, `task-files` |
| Git / processus | worktrees, dev-servers, launch-*, process-inspection | `worktrees`, `application-worktrees`, `worktree-agent-context`, `dev-servers`, `launch-*`, `process-inspection` |
| Agents | agents, account/credentials/updates/usage/interactions, providers | `agent-*`, `claude-*`, `run-preparation`, `verification` |
| Intégrations | connectors, forges, jira, tempo, pull-requests | `connector*`, `jira-*`, `azure-cli`, `forge-repositories`, `pull-requests` |
| Automatisation / temps | automation, task-queue, time-tracking, tempo-export | `automation*`, `time-*`, `agent-time*`, `card-publication-races`, `jira-sync-races` |
| Brainstorm / revue | brainstorm, ticket-preparation, reviews, vues | `brainstorm*`, `ticket-preparation`, `reviews`, `workflow-ui` |
| UI / traductions | `public/`, `public/views/`, locales et styles | `*-ui`, `*-view`, `ui-composition`, `task-workspace`, `confirm-*`, `i18n`, `static-assets` |
| OS / tray / import | launcher, browser, folders, tray, helpers natifs | `browser`, `platform`, `*tray`, `windows-installer-launch`, `installation` |
| Livraison | updater, release-*, worker, package, installateurs, CI | `updates*`, `update-worker`, `tar-package`, `UpdateArchive.Tests.ps1`, `release-*`, `installer-smoke`, `linux-installation.sh`, `security-pipeline` |

Inclure les exports de compatibilité, fixtures et exemples dans l'inventaire de suppression. Ne retirer un test Node que lorsque ses comportements, y compris ses erreurs, ont un équivalent prouvé.

## 9. Stratégie de vérification

| Niveau | Méthode | Ce que cela ne prouve pas seul |
| --- | --- | --- |
| Règles pures | Tests Rust déterministes, horloge et identifiants contrôlés | Compatibilité HTTP et OS |
| Contrats Node/Rust | Même scénario, données initiales séparées, effets simulés, comparaison des résultats | Services externes réels |
| SQLite / coffre | Fixtures, aller-retour entre versions, rollback, secrets de test sur OS réel | Portabilité des secrets entre comptes |
| Concurrence | Barrières contrôlées et fautes injectées autour des écritures/effets | Toutes les courses possibles |
| Agents / processus | Faux CLI compilés en Rust, descendants, flux importants et annulations | Comptes d'agents réels |
| UI | Tests WASM et navigateur piloté depuis Rust, screenshots, clavier et session | Tous les navigateurs sans exécution dédiée |
| Paquets | Extraction, lancement, update et rollback du vrai artefact | Bureaux/trousseaux interactifs non exercés |
| Qualification manuelle | Comptes/VM dédiés, vrais bureaux et trousseaux | Appels payants ou écritures distantes non autorisés |

Les tests comparatifs ignorent uniquement les différences non fonctionnelles documentées (identifiants aléatoires ou dates contrôlées, par exemple). Ne pas neutraliser les champs ou erreurs qui révèlent une régression. Ils n'effectuent jamais deux publications réelles pour comparer les implémentations.

Les tests WASM peuvent utiliser [wasm-bindgen-test](https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/index.html) ; leur runner doit être configuré pour le navigateur, sans dépendance Node. Choisir en M01 un pilote WebDriver côté Rust pour les parcours complets, vérifier sa couverture Safari/Firefox/Chromium et verrouiller les versions d'outils. Les tests d'interface ne doivent pas dépendre d'un script applicatif JavaScript maintenu à la main.

Commandes cibles, à rendre disponibles progressivement et à ne pas présenter comme déjà exécutables :

```text
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
cargo xtask test-ui
cargo xtask check-release
cargo xtask package
cargo xtask test-release
cargo xtask package-installers
cargo xtask test-installers
```

Adapter les commandes Clippy/check aux features natives et WASM dès M01 ; `cargo test` natif seul ne compte pas comme validation de l'interface. Les commandes de paquets doivent échouer lorsque les artefacts requis manquent, sans transformer le contrôle en test ignoré.

## 10. Risques et conditions de blocage

| Risque | Prévention / preuve requise | Lot qui ferme le risque |
| --- | --- | --- |
| Perte de champs ou de précision lors de la sérialisation | Aller-retour de fixtures enrichies, comparaison de contenu après écriture | M02 |
| Coffre illisible malgré le même algorithme | Compatibilité des enveloppes et identifiants, test sur chaque OS | M02 |
| Doubles effets introduits par la concurrence | Réservations atomiques, invariants et fautes injectées | M03–M06 |
| Descendants survivant à l'arrêt | Tests de groupes/arbre de processus natifs | M04 |
| Extensions utilisateur perdues | Diagnostic avant bascule, fichiers conservés et contrat testé | M05/M09 |
| Régression clavier, brouillon ou traduction | Parcours réels et comparaison visuelle | M07 |
| Ancien updater incapable d'atteindre R1 | Test de N0 face au dernier paquet, N1 et procédure manuelle | M09 |
| Restauration qui perd des écritures récentes | Validation du candidat avant réouverture aux mutations | M09 |
| Desktop Linux ou trousseau non couvert par CI | Essais natifs dédiés et limites déclarées | M08/M10 |
| Retrait de Node prématuré | Parité exhaustive et build/test sans Node installé | M11 |

Un lot ne passe pas à « validé » si une preuve obligatoire manque. Continuer les travaux indépendants possibles, mais ne pas présenter la migration complète si une cible, un installateur ou une protection reste non testé. Une difficulté ne justifie pas de supprimer la fonctionnalité ni d'abaisser silencieusement les garanties.

## 11. Suivi et reprise entre sessions

Mettre à jour la table des lots et ajouter une entrée ci-dessous après chaque incrément significatif. États autorisés : à faire, en cours, implémenté à vérifier, validé, bloqué avec cause précise. Un fichier présent ou une compilation réussie ne suffit pas à déclarer un lot validé.

Format d'entrée :

```text
Date / commit / lot :
Comportement livré et fichiers principaux :
Commandes exécutées et résultats :
Preuves natives / captures / artefacts :
Écarts de parité et risques encore ouverts :
Prochaine action concrète :
```

À chaque reprise : lire ce document et la matrice de parité si elle existe, vérifier `git status`, comparer les faits au suivi, puis prendre le premier travail dont les dépendances sont satisfaites. Préserver les modifications de l'utilisateur. Si le code a évolué depuis `843d3ea`, réconcilier l'inventaire avant d'implémenter une hypothèse périmée.

État initial : plan créé, migration non commencée, aucun test de référence exécuté dans le cadre de sa rédaction. Aucun chiffrage de durée ni gain de performance validé.

### 23 octobre 2026 / base `843d3ea`, incrément commis dans `ec6dbe4` / M00

- **Comportement livré :** aucune modification du produit ; `dataVersion` documentaire corrigée, dépendances M09 et verrou Node/Rust précisés. Matrice de parité et inventaire reproductible de 274 fichiers (93 fichiers de tests), fixtures synthétiques et benchmark isolé. Voir [matrice](rust-migration-parity.md), [inventaire](rust-migration-inventory.md), [preuves et protocole](rust-migration-baseline.md), [mesures brutes](rust-migration-measurements.json).
- **Commandes exécutées :** inventaire généré et vérifié ; quatre tests de référence ; suite complète avec `ATELIER_REQUIRE_RELEASE=1` : 425 tests, 422 réussis, zéro échec, trois exclus ; preflight release réussi ; huit cas PowerShell d'archives réussis ; fabrication Windows puis quatre scénarios sur vrai paquet réussis. Benchmark final serveur/API/logs/mémoire/Edge réussi après la suite. Aucun installateur activé, aucune publication.
- **Preuves :** paquet Windows local non signé identifié par SHA256 dans le relevé ; tests portable/données externes et rollback réels ; cinq processus serveur, 100 lectures état, cinq lectures de 10 000 logs, cinq navigations Edge, mutation SSE et rendu des logs. Mesures de démarrage `createApp` et navigation séparées ; pas de chronométrage du clic Start jusqu'au bureau complet.
- **Écarts et risques ouverts :** aucune parité Rust acquise. Installateur Windows et trays POSIX exclus de cette exécution ; coffres POSIX simulés, autres OS/navigateurs, endurance et coût du lanceur/tray à qualifier dans leurs lots. Cache OS non purgé. Le plan distingue maintenant ces garanties des preuves N0 existantes ; aucun gain Rust revendiqué. Budgets et estimation indicative consignés dans le relevé.
- **Revue :** aucun ajout de dépendance ni changement aux couches applicatives. Outillage de référence transitoire, assertions et tests, données temporaires uniquement. Inventaire lexical complété par la matrice comportementale ; ne vaut pas analyse sémantique automatique.
- **Prochaine action :** M01, créer le workspace à quatre crates, verrouiller les outils et démontrer la tranche UI → API authentifiée → tâche persistée, puis les prototypes natifs requis. Les exclusions de qualification restent obligatoires avant validation de leurs lots ; M00 n'autorise pas la bascule de production.

### 23 octobre 2026 / base `ec6dbe4`, incrément commis dans `c161512` / M01, tranche verticale

- **Comportement livré :** workspace de quatre crates, toolchain 1.99.1 et dépendances verrouillées ; formulaire Leptos/WASM, API locale authentifiée, SQLite dédiée, session DPAPI Windows et verrou de dossier. Commandes `serve-prototype`, `cargo xtask build-ui` et `cargo xtask test-ui`. Coexistence documentée dans AGENTS ; workflow Rust ajouté sans modifier celui de livraison Node. Voir [périmètre et reproduction](rust-migration-m01.md).
- **Commandes exécutées :** compilation native MSVC et WASM ; formatage et Clippy natif/WASM sans avertissements ; 10 tests Rust réussis, zéro ignoré ; parcours Edge réel réussi. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 425 tests, 422 réussis, zéro échec, trois exclus, 82,65 s. Les exclusions sont les mêmes qu'en M00 (installateur Windows, trays Linux/macOS). Aucun installateur ni publication activé.
- **Preuves :** compatibilité DPAPI avec PowerShell dans les deux directions ; vecteur HMAC Node/Rust ; refus HTTP, limites et rotation ; champs additionnels/null et ordre SQLite après fermeture ; refus d'un dossier Node et verrou d'alias ; création/relecture après redémarrage d'un vrai processus dans Edge 155.0.4283.62. Captures synthétiques `.artifacts/rust-ui-created.png` et `rust-ui-reopened.png` relues. CSP avec WASM ciblé, chargeur externalisé et texte HTML échappé.
- **Revue :** pas de nouvelle crate par fonctionnalité ; core sans adaptateur natif, UI sans secret de coffre ; primitives cryptographiques fournies par bibliothèques ; dépendances inutilisées retirées et verrou fourni par la bibliothèque standard. `Cargo.lock` et versions des outils épinglés. Aucun fichier utilisateur utilisé.
- **Écarts ouverts :** prototype limité au titre et aux cartes ; aucune parité métier complète. Les protections fichiers/fermetures exceptionnelles et le verrou N1 sont à durcir en M02. La nouvelle CI cinq cibles n'a pas encore été exécutée ; trays, trousseaux POSIX, Firefox/Safari, descendant et worker Windows restent à prouver. M01 reste en cours, M02–M11 non validés.
- **Prochaine action :** poursuivre M01 avec les prototypes natifs Windows (descendant, worker copié et tray), puis recueillir les résultats CI et bureaux Linux/macOS. Aucun retrait de Node ni accès aux données réelles avant les qualifications prévues.

### 23 octobre 2026 / base `c161512` / M01, prototypes Windows

- **Comportement livré :** helper Rust synthétique dans `xtask`, deux tests natifs Windows et `cargo xtask test-tray`. Aucun changement de l'application Node ou du serveur prototype. Le workspace conserve quatre crates ; `default-run` maintient l'alias `cargo xtask` malgré le binaire de fixture.
- **Preuves locales :** Job Object fermant le parent et son descendant, témoin externe au job conservé ; worker copié vérifiant PID + date de création puis attendant le handle avant remplacement ; candidat réellement relancé. Icône tray ajoutée, version 4 acceptée, rectangle natif trouvé puis disparition vérifiée. Détails, sources Microsoft et limites dans le [relevé M01](rust-migration-m01.md).
- **Vérifications :** 12 tests Rust réussis, zéro ignoré ; formatage et Clippy natif/WASM réussis. Suite Node avec `ATELIER_REQUIRE_RELEASE=1` : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 82,89 s. Inventaire étendu aux supports de tests Rust. Répertoires temporaires et processus créés par les essais uniquement.
- **Limites :** les fixtures prouvent les primitives, pas encore les adaptateurs complets M04/M08/M09. Pas de preuve Linux/macOS, trousseaux POSIX ou Firefox/Safari ; pas de nouvelle exécution CI distante. M01 reste en cours ; M02 n'est pas déclaré prêt ou validé tant que ses prérequis restent ouverts.
- **Prochaine action :** exécuter la nouvelle CI sur les cinq runners et compléter les prototypes tray/trousseau sur des bureaux Linux et macOS dédiés. Aucun retrait de Node ni migration de données réelles.

### 24 octobre 2026 / base `cca14ae` / reprise et CI native

- **Décision :** poursuivre les implémentations indépendantes pendant les preuves natives encore ouvertes, conformément à la section 10. M01 reste en cours ; un travail préparatoire M02 ne valide pas ses prérequis manquants.
- **État :** dépôt propre à la reprise, aucun changement distant supplémentaire après fetch. GitHub accessible avec l'authentification existante ; Ubuntu 26.04 x64 présent dans WSL, sans toolchain de développement détectée.
- **Changement :** branche `codex/rust-migration-2026-10-24` et déclenchement du seul workflow Rust sur `codex/rust-migration-*`. Aucun tag ni publication de release. Quatre tests Node de pipeline/security réussis ; inventaire régénéré.
- **Suite :** recueillir les résultats des cinq runners et du navigateur Linux ; avancer sur le schéma SQLite, transactions et sauvegardes synthétiques de M02.

### 24 octobre 2026 / base `371a1a9` / M02, stockage préparatoire

- **Livré :** schéma complet de `store.mjs`, documents JSON sans perte, paramètres, logs paginés et bornés en UTF-16, transactions et sauvegarde SQLite incluant le WAL sans écraser un fichier existant. Fermeture du thread avant libération de la base. Aucun accès aux données réelles autorisé.
- **Preuves :** 16 tests Rust réussis et Clippy natif propre ; comparaison `cargo xtask test-storage-parity` Node → Rust → Node → Rust sur 810 documents et 10 002 logs, sauvegarde relue par Node. Suite Node : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 82,11 s ; parcours Edge réussi. Voir [relevé M02](rust-migration-m02.md).
- **CI :** push autorisé explicitement par l'utilisateur après le refus initial de la revue automatique ; [exécution M01 sur `371a1a9`](https://github.com/FluryWaana/atelier/actions/runs/40031351769). Les résultats de cette exécution ne qualifient pas les modifications M02 encore locales.
- **Limites et suite :** restauration utilisateur, coffre complet, protections des fichiers, session, imports et verrou N1 encore à porter ; M01 et M02 restent en cours.

### 24 octobre 2026 / base `52dcb43` / M02, restauration et fichiers

- **Livré :** restauration transactionnelle avec secours, protections Tempo, invalidation du coffre/reviews/automatisation ; lecture de session par descripteur et refus des liens finaux. Fonctions internes, aucun accès aux données personnelles ni nouvelle route de restauration.
- **Preuves :** 20 tests Rust réussis, Clippy natif propre ; comparateur stockage et restauration Node/Rust réussi, y compris contenu du secours ; suite Node avec paquet réel : 425 tests, 422 réussis, zéro échec, trois exclusions, 81,80 s.
- **CI :** `371a1a9` et `52dcb43` passent sur les cinq runners natifs et Chrome Linux. Ajout de Firefox Linux, Safari macOS et comparaison historique POSIX ; résultats de ces nouveaux scénarios encore attendus.
- **Limites et suite :** terminer catalogue/imports, chemins, coffre, session et verrou commun. Trays et trousseaux POSIX encore ouverts. M01/M02 restent en cours ; aucune qualification globale ni retrait de Node.

### 24 octobre 2026 / base `f9b1598` / M02, coffre compatible

- **Livré :** coffre SQLite, AES-GCM POSIX et DPAPI Windows ; préparation chiffrée avant transaction, commandes système bornées et clé sur stdin. Réutilisation des dépendances ring/base64 déjà verrouillées. Probes natifs isolés Linux/macOS ajoutés à la CI.
- **Preuves :** 25 tests Rust locaux réussis et Clippy propre, processus synthétiques bornés et arrêtés au délai compris ; comparaison historique stockage/restauration/coffre réussie, enveloppes lues dans les deux directions, DPAPI Windows réel. Suite Node : 425 tests, 422 réussis, zéro échec, trois exclusions, 81,89 s. Aucun accès au trousseau de production.
- **Limites :** résultats natifs POSIX du nouveau probe attendus ; comptes différents, parcours réglages et coffre verrouillé natif encore à qualifier. Catalogue/imports, chemins, session complète et verrou commun toujours à terminer.

### 24 octobre 2026 / base `33d594a` / M02, import et catalogue

- **Livré :** catalogue de sauvegardes et restauration par nom strict ; import privé avec snapshot WAL, fichiers annexes et sauvegarde ; modes d'installation et emplacements de données. Fonctions internes, aucune ouverture des données réelles.
- **Preuves :** 29 tests Rust locaux réussis, Clippy propre ; import et sauvegarde comparés à Node sur le corpus complet. Suite Node : 425 tests, 422 réussis, zéro échec, trois exclusions, 82,18 s.
- **CI acquise :** cinq runners natifs de `33d594a` réussis, dont trousseaux Linux/macOS x64/ARM64 ; Chrome/Firefox Linux réussis sur `f9b1598`. Safari encore en cours. Voir les liens dans les relevés.
- **Suite :** terminer verrou commun N1/Rust et protocole local du lanceur ; qualification des chemins concurrents et registre Windows encore ouverte. Les trays POSIX et la validation complète M01/M02 restent nécessaires.

### 24 octobre 2026 / base `f9f53cf` / M02, contrôle local authentifié

- **Livré :** client loopback borné sans proxy/redirection, santé HMAC avant Bearer, session chargée après réponse, bootstrap navigateur privé POSIX. Parcours UI réutilisant santé et arrêt communs.
- **Preuves :** 33 tests Rust réussis, Clippy propre, Edge réel réussi ; suite Node avec paquets : 425 tests, 422 réussis, zéro échec, trois exclusions, 82,70 s. Le test de socket fermé a révélé puis vérifié le traitement explicite de Hyper `IncompleteMessage`.
- **Suite :** verrou N1/Rust avec exclusion réciproque et libération après crash ; chemins concurrents, intégration serveur/lanceur et preuves GUI encore ouvertes.

### 24 octobre 2026 / base `4afa807` / M02, verrou commun et correction POSIX

- **Livré :** transaction SQLite exclusive sur un fichier de coordination dédié, acquise avant la base applicative et libérée après sa fermeture ; adaptateur Node N1 préparé, intégration de livraison en M09. Imports excluant ces fichiers. Correction des permissions du staging POSIX détectée par la CI.
- **Preuves :** 34 tests Rust réussis, Clippy propre ; comparaison croisée avec acquisition refusée, alias, ports et arrêts brutaux Node/Rust réussie. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,39 s.
- **CI :** Safari réussi, `f9b1598` entièrement vert. L'import de `f9f53cf` échoue sur les quatre POSIX car `tempfile` créait un dossier trop permissif ; mode 0700 fixé dès création, assertion conservée. La nouvelle CI exécute les comparaisons historiques sur les cinq runners ; résultats attendus pour ce correctif et le verrou.
- **Suite :** recueillir ces preuves ; compléter les trays POSIX et les protections de chemins. M01/M02 restent en cours, N0 doit toujours être arrêté avant migration.

### 24 octobre 2026 / base `44dd895` / M02, registre Windows

- **Livré :** synchronisation de version installée par API Windows, limitée à l'inscription correspondant au dossier courant ; aucun shell. Branchement au lanceur en M08.
- **Preuves :** 35 tests Rust locaux réussis, Clippy propre ; test natif sur clé temporaire supprimée, y compris rollback, autre installation et chaînes développées. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,39 s.
- **CI acquise :** les cinq jobs natifs de `44dd895` passent avec comparaison historique, verrou interopérable et staging privé POSIX corrigé. Trays POSIX et qualifications complémentaires des chemins/comptes restent ouverts.
- **Suite :** prototypes tray Linux/macOS sur bureau natif CI, puis intégration progressive des règles et API métier sans retirer les références Node.

### 24 octobre 2026 / base `22545c2` / M01, prototypes tray POSIX

- **Comportement livré :** contrôles natifs Linux StatusNotifier et macOS AppKit dans le helper de test existant ; panneau XFCE isolé en CI, dépendances épinglées et cache des outils WASM. Détails, licences et sources dans le relevé M01.
- **Vérification locale :** Clippy des probes Linux/macOS en compilation croisée sans avertissement ; formatage, Clippy natif et 35 tests Rust réussis. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 81,97 s. Inventaire régénéré.
- **Limites :** les probes POSIX doivent encore réussir sur leurs vrais bureaux CI ; la vérification des types depuis Windows ne les qualifie pas. Aucun menu ni lanceur complet ajouté, aucune donnée personnelle ni publication.
- **Suite :** relever les résultats natifs, poursuivre les règles métier M03 indépendantes et les preuves de fichiers/coffres M02. M01/M02 restent en cours.

### 24 octobre 2026 / base `fec382f` / M03, règles pures préparatoires

- **Comportement livré :** critères, dépendances et profils d'agents dans core, sans adaptateur ni dépendance supplémentaire ; 175 vecteurs capturés des vraies fonctions Node et comparaison historique en CI. Voir [relevé M03](rust-migration-m03.md).
- **Vérification :** Clippy natif/WASM sans avertissement ; 37 tests Rust réussis, dont parité des résultats/messages et chaîne de dépendances de 10 000 cartes. Suite Node complète et inventaire vérifiés avant commit ; résultat détaillé dans le relevé M03.
- **Preuves complémentaires :** CI de [44dd895](https://github.com/FluryWaana/atelier/actions/runs/40036942056) et [22545c2](https://github.com/FluryWaana/atelier/actions/runs/40037800280) entièrement réussies, cinq runners natifs et Chrome/Firefox/Safari. Les premiers probes tray `fec382f` passent sur Linux ARM64 et macOS ARM64 ; résultats des autres cibles encore attendus.
- **Limites et suite :** règles seules, pas de remplacement de l'API ni d'exécution d'agent ; terminer les preuves natives ouvertes et porter les cas d'usage métier. Aucun lot globalement déclaré validé.

### 24 octobre 2026 / base `05dbfc8` / M03, réglages de projets et clôture M01

- **Comportement livré :** règles de réglages/héritage de projets et comparaison de dépôts dans core ; corpus historique porté à 246 cas. Aucune dépendance supplémentaire. Voir le relevé M03.
- **Vérification :** formatage et Clippy natif/WASM propres ; 37 tests Rust réussis et 246 contrats Node comparés. Suite Node avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 83,19 s. Inventaire à jour.
- **Preuve M01 :** les cinq jobs natifs de [fec382f](https://github.com/FluryWaana/atelier/actions/runs/40038851567) réussissent, incluant présence/retrait des trays Linux et macOS sur les deux architectures. Avec les parcours navigateurs antérieurs et les probes Windows réussis, les critères de faisabilité M01 sont satisfaits. Cela ne valide pas les menus/lanceurs produit de M08 ni la migration de données M02.
- **Écart découvert :** la duplication Node peut couper une paire UTF-16 avec `.slice(0,300)` et persister un substitut isolé. Un probe synthétique confirme que `serde_json::Value` le refuse. Les corpus précédents couvraient Unicode valide seulement. Il faut conserver ces unités, sans remplacement silencieux ; voir le relevé M02.
- **Suite :** traiter cette compatibilité de chaînes puis reprendre les cas d'usage et routes M03. Les données utilisateur restent hors périmètre des essais.

### 24 octobre 2026 / base `038e794` / M02, documents JSON sans perte

- **Comportement livré :** `Document`/`JsonString`, basés sur les fonctions raw/WTF-8 existantes de Serde JSON, conservation des clés et valeurs UTF-16 atypiques dans SQLite et restauration ; aucune dépendance externe supplémentaire. La comparaison historique couvre le vrai titre tronqué par Node, les réécritures Rust, la restauration et le secours.
- **Vérification :** 40 tests Rust réussis ; Clippy natif/WASM propre ; comparaison historique stockage/coffre/import/verrou/UTF-16 réussie. Suite Node complète : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,32 s.
- **CI :** `fec382f` entièrement réussi. `05dbfc8` échoue uniquement au comparateur historique Windows : Git transforme le JSON de fixture en CRLF, alors que le contrôle comparait les octets du fichier. Correction : comparaison des valeurs JSON complètes, sans suppression de champs ni normalisation du contenu. Reproduction CRLF locale réussie ; revalidation CI attendue.
- **Suite :** adapter les contrats HTTP/UI et règles encore fondés sur `String`/`Value` ; poursuivre les protections natives M02 et les cas d'usage M03. Aucun accès aux données utilisateur.

### 24 octobre 2026 / base `93f8276` / M02, UTF-16 dans HTTP et WASM ; correction de preuve M01

- **Comportement livré :** contrats de carte, lecture SQLite et API de création/lecture conservent les unités UTF-16 et champs inconnus sans passer par `Value`. Le titre est affecté au `textContent` du DOM avec les bindings JS standards ; aucun HTML injecté, aucun JavaScript applicatif manuel. `js-sys` 0.3.106, déjà verrouillé, devient dépendance directe UI.
- **Vérification :** 41 tests Rust réussis ; Clippy natif/WASM propre. Nouveau test HTTP de titre isolé et clés JSON atypiques après redémarrage. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,68 s ; contrôle d'inventaire final réussi. Parcours Edge réel et capture relue : unités du titre dans le DOM vérifiées numériquement après réouverture.
- **Correction de preuve :** le précédent scénario changeait seulement le fragment de session et pouvait conserver l'ancien DOM après redémarrage. L'ajout d'une seconde carte par API a révélé ce faux positif. Le scénario quitte maintenant la page vers `about:blank`, puis ouvre la nouvelle session ; les deux cartes doivent apparaître. M01 repasse à « implémenté à vérifier » en attendant Chrome/Firefox/Safari avec ce scénario corrigé. Les preuves serveur/natives antérieures restent valables.
- **CI précédente :** le contrôle Windows de `93f8276` passe, correction CRLF et comparaison UTF-16 comprises ; les jobs natifs Linux et macOS ARM64 passent également. Le résultat macOS Intel était encore attendu à ce relevé.
- **Suite :** recueillir les parcours corrigés sur les autres navigateurs ; poursuivre les cas d'usage métier avec la représentation sans perte et les preuves natives M02 restantes.

### 24 octobre 2026 / base `aa9bf62` / M03, duplication HTTP

- **Comportement livré :** duplication pure dans core et route authentifiée `POST /api/cards/{id}/duplicate` ; contenu éditable seulement, critères décochés, statut todo, titre UTF-16 borné. Lecture/écriture sérialisées dans le même job SQLite.
- **Vérification :** 42 tests Rust réussis, Clippy natif/WASM propre ; copie avec critères et suppression des liens/états vérifiée ; deux appels HTTP concurrents, 404 et persistance sans modification de l'original. Comparaison historique UTF-16 utilisant réellement la duplication Rust réussie, ainsi que les comparaisons stockage/restauration/coffre/import/verrou. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,69 s.
- **Preuves natives :** CI `93f8276` entièrement réussie, Windows corrigé compris. Les deux jobs navigateur de [aa9bf62](https://github.com/FluryWaana/atelier/actions/runs/40041776924) réussissent avec le scénario de vraie réouverture : Chrome, Firefox et Safari requalifiés ; Edge avait passé localement. M01 est de nouveau validé avec ces preuves corrigées.
- **Suite :** compléter les refus natifs des coffres et protections de chemins M02, puis poursuivre les cas d'usage et routes M03. Aucun agent ou service externe réel sollicité.

### 24 octobre 2026 / base `3aef62d` / M02, refus natif Linux préparé

- **Livré :** scénario de verrouillage du trousseau Linux, refus du chiffrement/déchiffrement et conservation de la clé après déverrouillage ; bus et dossiers XDG jetables en CI. Aucune modification du coffre de production ni dépendance supplémentaire.
- **Vérification locale :** 42 tests Rust réussis, formatage et Clippy natif vérifiés ; suite Node avec paquets : 426 tests, 423 réussis, zéro échec, trois exclusions, 81,45 s ; inventaire vérifié. Aucun essai sur le trousseau personnel. Le scénario Linux doit encore passer sur ses runners natifs, aucune validation anticipée.
- **CI précédente :** [aa9bf62](https://github.com/FluryWaana/atelier/actions/runs/40041776924) entièrement réussi, cinq cibles et navigateurs. `3aef62d` passe les deux jobs navigateur et quatre cibles natives ; macOS Intel encore en cours au relevé.
- **Suite :** recueillir le résultat natif Linux, compléter macOS verrouillé et comptes différents, puis protections de chemins et cas d'usage métier. M02/M03 restent en cours.

### 24 octobre 2026 / base `098cbb8` / M02, macOS verrouillé et reprise Linux

- **Livré :** scénario macOS sur trousseau temporaire dédié, refus verrouillé et relecture de la clé initiale après déverrouillage ; chemins avec espaces/Unicode et secrets uniquement sur stdin.
- **Correction :** le probe Linux révèle que `gnome-keyring-daemon --unlock` démarre un autre démon. Déverrouillage désormais adressé au démon du scénario via l'interface interne GNOME, réservée à cette fixture ; assertion de conservation maintenue. Les nouveaux résultats POSIX restent attendus.
- **Vérification :** 43 tests Rust réussis, Clippy natif et contrôle de types D-Bus Linux propres ; suite Node complète : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,03 s. La [CI de duplication `3aef62d`](https://github.com/FluryWaana/atelier/actions/runs/40042339237) est entièrement réussie.
- **Suite :** recueillir les quatre résultats natifs de coffre verrouillé ; comptes différents, courses de chemins et intégration aux cas d'usage restent ouverts. Aucune qualification globale M02.

### 24 octobre 2026 / base `de8083a` / M03, saisies de temps

- **Livré :** validation pure des saisies de temps, conversions de durée compatibles, dates/heures et erreurs historiques, descriptions UTF-16 sans perte. Aucune dépendance supplémentaire ni envoi Tempo.
- **Vérification :** 44 tests Rust réussis, 305 contrats Node/Rust comparés (246 précédents inchangés), Clippy natif/WASM propre ; suite Node complète : 426 tests, 423 réussis, zéro échec, trois exclusions, 82,01 s ; inventaire vérifié.
- **CI coffres :** macOS ARM64 réussit le scénario verrouillé. Linux signale `items_after_test_module` avant le probe ; déplacement du module et contrôle croisé `--all-targets` corrigent cet écart, sans désactivation de Clippy. Reprise Linux et macOS Intel encore attendues au relevé.
- **Suite :** cas d'usage/chronomètres/routes de temps, autres opérations métier et preuves M02 restantes. La validation pure ne qualifie pas leurs transactions ou concurrence.

### Incrément M02 — isolation DPAPI entre comptes, après `492fa78`

- **Preuves recueillies :** CI `492fa78` entièrement réussie ; refus des coffres verrouillés Linux x64/ARM64 et macOS x64/ARM64 vérifiés dans les journaux natifs.
- **Livré :** probe Windows à deux comptes jetables, identités natives et contrôles positifs, refus réciproque et conservation du chiffré initial. Provisionnement exclusivement sur le runner CI.
- **Vérification locale :** 45 tests Rust réussis ; Clippy natif tous targets ; syntaxe PowerShell du scénario vérifiée sans provisionnement local ; suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions documentées, 79,673 s.
- **Attendu :** exécution Windows entre comptes en CI. Les preuves POSIX entre comptes, courses de chemins et intégrations M02 restent ouvertes.

### Incrément M02 — copie ancrée des pièces jointes

- **Preuve DPAPI recueillie :** le job Windows de `2d7f86a` passe, journal vérifié ; deux comptes natifs distincts, contrôles positifs et refus réciproque.
- **Livré :** copie récursive relative à des dossiers ouverts, sans suivre les liens, avec validation et lecture sur le même descripteur ; dépendances natives `cap-std`/`cap-fs-ext` 4.0.3 justifiées dans le relevé M02.
- **Vérification :** 47 tests Rust réussis, Clippy natif tous targets ; parité stockage/import/restauration Node/Rust réussie. Suite Node avec contrôle release : 426 tests, 423 réussis, zéro échec, trois exclusions, 80,633 s ; inventaire vérifié.
- **Suite :** preuve POSIX du remplacement d'ancêtres en CI, isolation des coffres POSIX entre comptes, acquisition des racines/ouverture SQLite/publication finale et autres protections M02. Le prototype reste interdit aux données réelles.

### Incrément M02 — scénario Linux entre comptes

- **Livré :** probe à UID distincts, contrôles positifs et refus réciproque, deux bus/coffres privés sur comptes CI jetables ; nettoyage des seuls processus/comptes/fichiers créés et délai global.
- **Vérification :** 48 tests Rust locaux réussis ; Clippy natif et contrôle de types/Clippy Linux tous targets réussis ; syntaxe Bash et refus hors CI vérifiés sous WSL sans provisionnement. Suite Node complète avec contrôle release : 426 tests, 423 réussis, aucun échec, trois exclusions, 80,955 s.
- **Preuves recueillies :** copie ancrée `7c729a0` réussie sur Linux x64/ARM64, journaux des remplacements de chemins relus.
- **Attendu :** scénario à deux comptes Linux en CI et preuve macOS correspondante ; autres protections M02 non closes.

### Incrément M02 — scénario macOS entre comptes

- **Livré :** trousseaux privés par UID, tentatives croisées avec sa propre clé et ouverture explicite du trousseau de l'autre compte ; contrôles de conservation avant nettoyage. Provisionnement exclusivement CI, sans compte administrateur ni mot de passe de connexion.
- **Vérification :** 48 tests Rust locaux réussis ; Clippy natif et contrôles de types/Clippy Linux/macOS ARM64 tous targets ; syntaxe Bash et refus local vérifiés. Suite Node avec contrôle release : 426 tests, 423 réussis, zéro échec, trois exclusions, 79,778 s ; inventaire vérifié.
- **Preuves recueillies :** CI `7c729a0` entièrement réussie, journaux des courses de copie vérifiés sur les quatre cibles POSIX. Comptes Linux ARM64 `513f728` validés, journal relu.
- **Attendu :** Linux x64 et macOS entre comptes ; autres courses de fichiers M02 et intégrations restantes.

### Incrément M02 — session et marqueur d'installation ancrés

- **Livré :** helper de lecture relative sans liens partagé par sessions, installation et pièces jointes ; publication/nettoyage de session dans le dossier ouvert.
- **Vérification :** 51 tests Rust, Clippy natif tous targets, parcours Edge avec redémarrage/rotation réussis. Suite Node complète avec contrôle release : 426 tests, 423 réussis, zéro échec, trois exclusions, 80,419 s ; inventaire vérifié.
- **Preuves recueillies :** CI `513f728` entièrement réussie ; isolation native Linux x64/ARM64 entre comptes confirmée dans les journaux.
- **Attendu :** nouveaux tests de chemins POSIX et isolation macOS entre comptes. Acquisition initiale des racines, sauvegardes/import SQLite et intégrations M02 restent ouverts.

### Correction du scénario macOS entre comptes

- La CI ARM64 `83ec5da` refuse le provisionnement Directory Service avant le probe. L'écriture directe de `Password` est retirée ; diagnostic de ligne ajouté, sans affaiblissement des contrôles du coffre.
- Syntaxe Bash et refus local vérifiés sous WSL sans provisionnement ; aucune preuve native macOS entre comptes revendiquée. L'application reste au niveau vérifié de `4405a65`.

### Incrément M02 — sauvegardes ancrées

- **Livré :** catalogue/création/restauration/secours relatifs au dossier ouvert, snapshot privé et publication atomique native sans remplacement ; nettoyage des temporaires après refus.
- **Vérification :** 53 tests Rust, Clippy natif et contrôles de types/Clippy POSIX ; parité stockage/import/restauration Node/Rust réussie. Suite Node avec contrôle release : 426 tests, 423 réussis, zéro échec, trois exclusions, 80,045 s.
- **CI macOS :** le diagnostic identifie `GeneratedUID`, pas `Password`, comme attribut refusé. Le GUID attribué est maintenant lu sans modification ; garde-fou local vérifié.
- **Suite :** exécution native POSIX des sauvegardes et macOS entre comptes ; publication finale/import SQLite et acquisition initiale des racines. Mesurer le coût de copie supplémentaire et qualifier les systèmes de fichiers avant livraison.

### Incrément M02 — publication d'import ancrée

- **Livré :** staging, sauvegarde initiale, publication sans écrasement et nettoyage dans le parent de destination ouvert. Le snapshot terminé est copié par descripteur.
- **Vérification :** 54 tests Rust, Clippy natif, parité stockage/import/restauration Node/Rust ; suite Node complète avec contrôle release : 426 tests, 423 réussis, zéro échec, trois exclusions, 80,563 s.
- **Preuves recueillies :** sauvegardes ancrées sur Linux x64/ARM64 et macOS ARM64 (`5332203`), journaux relus. Coffres macOS ARM64 entre comptes réussis, y compris conservation finale du second secret.
- **Suite :** macOS Intel, nouveaux scénarios POSIX d'import ; ouverture SQLite de la source et acquisition des racines restent à qualifier.

### Incrément M02 — identité de la source SQLite et preuves macOS Intel

- **Livré :** contrôle de l'identité source ancrée avant SQLite, refus natif des liens et durée de vie des descripteurs préservée jusqu'à fermeture SQLite.
- **Vérification :** 55 tests Rust, Clippy natif, parité stockage/import/restauration ; suite Node complète avec contrôle release : 426 tests, 423 réussis, aucun échec, trois exclusions, 80,031 s.
- **Preuves recueillies :** CI `5332203` entièrement réussie, journal macOS Intel des sauvegardes et coffres entre comptes relu ; publication d'import `a457449` validée dans le journal macOS ARM64.
- **Suite :** nouvelles branches POSIX en CI, ouverture SQLite encore non atomiquement ancrée et intégrations HTTP M02, notamment notifications SSE. M02 demeure en cours.

### Incrément M02 — notifications HTTP authentifiées

- **Livré :** SSE historique, notifications après mutations réussies, heartbeat, canal borné, reconnexion et fermeture des abonnés lors de l'arrêt.
- **Vérification :** 57 tests Rust et Clippy natif tous targets ; contrat SSE Node ajouté. Suite Node complète avec contrôle release : 427 tests, 424 réussis, aucun échec, trois exclusions, 80,166 s.
- **Preuves recueillies :** CI `a457449` entièrement réussie ; journaux des scénarios d'import ancré relus sur les quatre cibles POSIX.
- **Suite :** CI des refus source et de SSE, producteurs internes futurs, ouverture SQLite et intégrations restantes ; aucune qualification globale M02.

### Incrément M03 — saisies et chronomètres locaux

- **Livré :** cas d'usage, ports stockage/horloge, routes locales, transactions, réveil/SSE, fermeture et reprise. Export/reconcile explicitement 503 jusqu'à M05 ; interface de saisie en M07.
- **Vérification :** 60 tests Rust, Clippy natif/WASM, 46 transitions Node/Rust sur SQLite, 305 contrats de domaine et parité stockage/import/restauration. Concurrence HTTP et rollback d'un tick après faute SQL vérifiés.
- **Référence Node :** suite complète avec contrôle release : 427 tests, 424 réussis, zéro échec, trois exclusions, 81,145 s.
- **Preuves M02 recueillies :** CI `43ba63a` et `3b4302e` entièrement réussies ; journaux des refus source et de SSE relus sur les quatre cibles POSIX. Les limites d'ouverture SQLite restent documentées, sans qualification atomique revendiquée.
- **Suite :** CI de ce nouvel incrément ; autres cas d'usage/API M03 et protections/intégrations M02 restantes. La migration n'est pas qualifiée globalement.

### Incrément M02 — préparation privée du dossier prototype

- **Livré :** préparation relative au dossier ouvert, contrôle du marqueur et du type de base ; propriétaire et permissions privées POSIX avant ouverture SQLite.
- **Vérification :** 61 tests Rust Windows, Clippy natif, comparaison stockage/import/restauration. Suite Node complète avec contrôle release : 427 tests, 424 réussis, aucun échec, trois exclusions, 80,072 s.
- **Preuves recueillies :** chronomètres `6529ea2` sur Linux x64/ARM64 et macOS ARM64, journaux des 46 transitions et scénarios HTTP/rollback relus.
- **Suite :** nouvelles branches POSIX en CI, résultats Windows/macOS Intel des chronomètres, autres protections/intégrations M02 et cas d'usage M03. L'ouverture SQLite par chemin reste une limite explicite.

### Correction CI — fins de ligne de la référence de temps

- Le job Windows `6529ea2` passe Rust mais échoue au comparateur textuel CRLF/LF. Comparaison des valeurs JSON à la place du texte ; attentes métier inchangées.
- Les 46 transitions sont vérifiées localement en CRLF et LF, octets originaux restaurés sans diff. Nouvelle preuve Windows attendue ; l'incrément d'archivage/suppression en travail n'est pas inclus dans cette correction.

### Incrément M03 — archivage et suppression des cartes

- **Livré :** gardes historiques dans core, routes transactionnelles avec 400/404/409, conservation UTF-16 et notification après commit. Branchement aux activités/files futures M04/M06 encore requis.
- **Vérification :** 63 tests Rust, Clippy natif/WASM ; scénario Node équivalent ajouté et parcours HTTP complet avec temps/reprise. Suite Node complète avec contrôle release : 428 tests, 425 réussis, aucun échec, trois exclusions, 83,291 s.
- **Preuves recueillies :** chronomètres macOS Intel `6529ea2` ; préparation privée de racine Linux/macOS ARM64 `07691c8`, journaux relus. Revalidation Windows CRLF et nouvelles cibles toujours attendues.
- **Suite :** création/édition complètes, modèles, projets/réglages et autres cas d'usage M03 ; protections/intégrations M02 restantes. Aucun lot globalement qualifié par cet incrément.

### Incrément M03 — modèles de tâches

- **Livré :** création/retrait de modèles transactionnels, critères réinitialisés, 50 modèles maximum, conservation des réglages et de l'UTF-16. Application à la création complète et interface encore ouvertes.
- **Preuves :** 65 tests Rust, Clippy natif/WASM ; test Node équivalent et scénario HTTP concurrent avec reprise. Résultat complet Node dans le relevé M03.
- **CI recueillie :** `ec85b26` entièrement réussie ; journaux des cinq cibles natifs relus pour les chronomètres et l'archivage/suppression, quatre POSIX pour le dossier privé. Correction CRLF Windows confirmée. Aucun nouveau lot globalement clos.
- **Suite :** création/édition complètes, projets/réglages ; protections/intégrations M02 restantes.

### Incrément M03 — contrat de création complet dans core

- **Livré :** création de tâches/brainstorms et héritage de modèles dans core, avec ports explicites ; critères et prompt sans perte UTF-16. Le branchement HTTP et les accès natifs restent à porter.
- **Vérification :** 77 créations capturées de Node et ordre des appels comparés ; 305 contrats précédents inchangés, 66 tests Rust et Clippy natif/WASM réussis. Suite Node complète consignée dans M03.
- **CI recueillie :** modèles `1b9a9f7` réussis sur les cinq cibles natives ; journaux de concurrence/reprise et conservation des réglages relus.
- **Suite :** brancher la création complète avec héritage de projet et résolution native du dépôt, puis édition/projets/réglages. M02/M03 restent en cours.

## 12. Définition finale de terminé

- [ ] Chaque fonctionnalité et ancien test est couvert dans la matrice de parité.
- [ ] Chaque lot a passé la revue Clean Code / SOLID / KISS / DRY / YAGNI ; aucune abstraction ou dépendance sans besoin démontré ne reste.
- [ ] Données version 2, secrets, historiques et installations existantes préservés et vérifiés.
- [ ] Serveur et interface Rust complets, sans réponses factices ni fonctions disparues.
- [ ] Extensions intégrées et externes opérationnelles ; incompatibilités anciennes expliquées avant transition.
- [ ] Protections HTTP, fichiers, processus, secrets et effets distants conservées.
- [ ] Trays, lanceurs, import et arrêt fonctionnent sur les trois OS.
- [ ] Transition Node/Rust, futures mises à jour Rust et retours arrière testés sur paquets réels.
- [ ] Cinq archives et cinq installateurs validés sur leurs cibles natives.
- [ ] Tests UI, clavier, traductions et reconnexion validés dans les navigateurs retenus.
- [ ] Contrôles de sécurité, dépendances et licences opérationnels et bloquants.
- [ ] Build, tests courants et exécution sans Node/npm, avec exceptions externes documentées.
- [ ] Mesures avant/après disponibles et régressions résolues ou explicitement acceptées.
- [ ] Ancien code retiré, documentation et procédures de restauration à jour.
- [ ] Aucun secret, fichier utilisateur, base personnelle ou journal ajouté aux artefacts ou au dépôt.

## 13. Prompts de reprise

### Lire et examiner le plan, sans implémenter

```text
Lis AGENTS.md puis docs/rust-migration-plan.md en entier dans ce dépôt.
Lis docs/rust-migration-parity.md s'il existe et vérifie l'état Git.
Confronte les décisions importantes aux sources et tests concernés.
Résume la cible, les dépendances entre lots, les risques non résolus et le
premier lot réellement prêt à commencer. Signale les incohérences avec
l'état actuel du dépôt et les preuves manquantes, sans inventer de validation.
Ne modifie aucun fichier et ne démarre pas la migration à cette étape.
```

### Commencer ou poursuivre l'implémentation quand elle est demandée

```text
Applique docs/rust-migration-plan.md après l'avoir lu intégralement avec
AGENTS.md et la matrice de parité si elle existe. Vérifie d'abord l'état
réel du dépôt et le suivi ; ne recommence pas les lots déjà prouvés.
Commence par M00 s'il n'est pas validé, puis progresse dans l'ordre des
dépendances avec des incréments complets et vérifiés. Préserve toutes les
fonctionnalités, les données, les protections et les modifications existantes.
Mets à jour le suivi et la matrice avec les commandes réellement exécutées,
les résultats, les limites et la prochaine action. Ne marque aucun lot
validé sans ses critères de sortie. Travaille sur données de test ; aucun
appel payant, publication de release ou changement aux données utilisateur.
Si une validation externe manque, termine le travail indépendant possible
et indique précisément ce qui reste à vérifier. Ne déclare pas la migration
terminée tant que la checklist finale n'est pas satisfaite.
```
