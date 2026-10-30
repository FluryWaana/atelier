# Clean code Rust et SOLID dans Atelier Kanban

Le découpage en quatre crates est cohérent avec le produit. Le domaine possède des fonctions pures et des contrats réutilisés par le serveur et l'interface. L'écart principal est le maintien de structures dynamiques et de conversions historiques dans des parcours métier actifs. La qualité n'est donc ni uniformément mauvaise ni complètement conforme à l'objectif de reconstruction typée.

## À corriger

### 2 Des contrats hérités de JavaScript restent actifs

**Priorité : moyenne. Statut : confirmé par lecture et recherche des appelants.**

[document.rs](../../crates/atelier-core/src/document.rs) définit `JsonString(Vec<u16>)`, accepte les surrogates UTF-16 isolés et contient un décodage WTF-8 pour les préserver. `Document::truthy` conserve aussi une interprétation dynamique des valeurs. Le [plan actif](../rust-migration-plan.md) demande explicitement de ne plus reproduire ces particularités par compatibilité.

Dans [store.rs](../../crates/atelier-app/src/infrastructure/store.rs), `document_input` traduit encore booléens, nombres, tableaux et objets en conventions de liaison SQLite héritées de Node. Ce n'est pas du code mort : [time_tracking.rs](../../crates/atelier-app/src/infrastructure/time_tracking.rs) et [task_templates.rs](../../crates/atelier-app/src/infrastructure/task_templates.rs) l'appellent dans leurs implémentations de repository. Les traits métier correspondants reçoivent `Option<&RawValue>` pour un identifiant de carte.

**Cas concret :** un `cardId` booléen ou un tableau atteint un contrat qui interprète sa forme JSON au lieu de demander immédiatement un identifiant texte. Cette complexité est visible même si l'identifiant final ne correspond à aucune carte. Aucune modification non autorisée de carte n'est démontrée par ce constat.

Autre exemple : [task_creation.rs](../../crates/atelier-core/src/application/task_creation.rs), `default_agent`, conserve des comparaisons de valeurs JSON et un message `replaceAll is not a function`. [http.rs](../../crates/atelier-app/src/http.rs), `object`, justifie encore sa limite supplémentaire par le comptage Node en unités UTF-16.

**Correction minimale proposée :** commencer par les identifiants des parcours Temps et Modèles. Valider un `String` non vide à l'entrée, faire recevoir `&str` aux traits et réutiliser `Store::document`. Adapter les appels et les tests aux nouveaux refus explicites. Étendre ensuite les modèles typés aux champs métier concernés. Conserver du JSON ouvert uniquement lorsqu'un besoin d'extension le justifie.

**Sans correction :** le compilateur ne peut pas protéger autant de règles métier et des erreurs de contrat restent détectées tard. Il faut entretenir des mécanismes de compatibilité que le projet a décidé d'abandonner. Le remplacement ne doit pas être une suppression globale de `Document` sans examen de ses usages.

### 3 La politique de concurrence HTTP dépend des noms de routes

**Priorité : moyenne pour la maintenance. Statut : couplage confirmé, course précise non démontrée.**

Dans [http.rs](../../crates/atelier-app/src/http.rs), `authorize` calcule le besoin de verrou exclusif avec une longue liste de `starts_with`, `ends_with` et comparaisons de chemins. Cette fonction porte à la fois la sécurité HTTP, la politique de concurrence et plusieurs gardes d'opérations métier. Les routes sont ensuite déclarées séparément dans le routeur.

**Cas concret :** l'ajout ou le renommage d'une route à effet externe doit être reporté dans cette liste pour conserver la même politique. Le compilateur ne signale pas l'oubli. Les annulations ont déjà une classification distincte dans [mutations.rs](../../crates/atelier-app/src/http/mutations.rs).

**Correction minimale proposée :** rattacher la classification des mutations aux groupes de routes concernés, ou à une petite définition partagée avec leur enregistrement. Réutiliser le verrou et les gardes existants. Ajouter une vérification qui échoue si une route à effet n'a pas de politique explicite. Ne pas remplacer cela par un framework générique de commandes.

**Sans correction :** chaque nouvelle route demande une vérification manuelle de plusieurs endroits. Ce constat porte sur la facilité d'introduire une régression. Il ne prouve pas qu'une route actuelle contourne les contrôles.

### 4 Le README contient une autorisation de CI périmée

**Priorité : moyenne pour les consignes de travail. Statut : confirmé par comparaison des textes.**

La section Développement et qualification du [README](../../README.md) indique que des relances correctives de CI ont été autorisées. Le même README contient plus haut une interdiction de lancement ou de relance. [AGENTS.md](../../AGENTS.md) précise que l'autorisation de CI a été retirée le 27 octobre et que cette décision remplace l'autorisation précédente.

**Cas concret :** une personne qui suit le paragraphe de qualification du README reçoit une consigne contradictoire sur la possibilité de relancer un workflow.

**Correction minimale proposée :** retirer la phrase périmée et renvoyer vers la règle actuelle. La configuration [rust.yml](../../.github/workflows/rust.yml) reste manuelle. Son existence n'autorise pas son lancement.

**Sans correction :** le dépôt conserve des instructions incompatibles pour une action explicitement soumise à restriction. Aucun workflow n'a été lancé pendant l'audit.

## À simplifier

### 6 Une dépendance HTTP directe sans appelant retrouvé

**Priorité : faible. Statut : absence de référence directe confirmée par recherche, suppression non compilée.**

Le [manifeste serveur](../../crates/atelier-app/Cargo.toml) déclare `tower-http` avec la fonctionnalité `fs`. La recherche de `tower-http` et `tower_http` dans les sources, tests, scripts et configurations du projet ne retrouve que cette déclaration, en dehors du verrou des dépendances. Le serveur d'assets lu utilise son propre routeur Axum et ses fichiers contrôlés.

**Cas concret :** le manifeste fait apparaître un composant de service de fichiers qui n'a aucun appelant direct retrouvé, alors que les assets passent par `asset_router`. Cela ajoute un choix de dépendance à maintenir sans besoin visible.

**Correction minimale proposée :** retirer cette déclaration directe, puis vérifier la compilation et le verrou dans un incrément séparé. Ne pas supprimer des dépendances transitives encore requises par d'autres composants.

**Sans correction :** un composant sans usage direct identifié reste dans le périmètre de maintenance. Aucun gain de taille binaire ou de temps de compilation n'est mesuré. Une seule dépendance directe est candidate au retrait, pas nécessairement tout son graphe transitif.

## Bonnes pratiques Rust observées

| Critère | Évaluation justifiée |
| --- | --- |
| Séparation des dépendances | `atelier-core` ne déclare que Serde et serde_json. Les recherches des accès disque, réseau et processus usuels n'y ont pas révélé de dépendance native. Voir [son manifeste](../../crates/atelier-core/Cargo.toml) |
| Modèles explicites | Les contrats Tempo utilisent des structs et `deny_unknown_fields` pour plusieurs commandes sensibles. Les états et erreurs métier comportent des enums. Voir [contracts.rs](../../crates/atelier-core/src/contracts.rs) |
| Typage des données métier | Partiel. Des `Document`, clés texte et `RawValue` restent au centre de plusieurs cas d'usage. Voir constat 2 |
| Gestion des erreurs | `Result` est utilisé aux frontières. Plusieurs erreurs métier sont typées. Le type général reste `Box<dyn Error + Send + Sync>`, avec classification HTTP par `downcast_ref` dans [responses.rs](../../crates/atelier-app/src/http/responses.rs). Ce choix fonctionne mais rend les nouveaux cas moins visibles à la compilation |
| Mémoire et ressources | Les handles et descripteurs possédés ont des gardes Rust. `Database::Drop` ferme le canal puis attend son worker. Les adaptateurs de processus possèdent leur enfant et leurs mécanismes d'arrêt |
| Code unsafe | Présent dans les adaptateurs natifs. Plusieurs blocs lus ont une explication de propriété, de taille ou d'initialisation. Exemple : liste de processus Windows alignée sur `usize` et compte vérifié avant lecture. La sûreté de tous les blocs n'a pas été démontrée |
| Asynchrone et SQLite | SQLite est déporté vers un worker dédié plutôt qu'exécuté directement dans chaque handler asynchrone. Des opérations fichiers lourdes utilisent `spawn_blocking`, par exemple l'empreinte de dépôt dans [files.rs](../../crates/atelier-app/src/infrastructure/files.rs) |
| Concurrence d'effets distants | Les réservations et revalidations sont concrètes dans l'export Tempo. Les mutations acceptées sont détachées du cycle de vie de la connexion navigateur dans `mutations.rs` |
| Outils reproductibles | Toolchain, dépendances directes et outils UI sont épinglés. Les constructions prévues utilisent le verrou Cargo |
| Tests | Des tests existent pour chemins, stockage, refus HTTP, effets incertains et nettoyage. Leur présence est une preuve d'intention et de scénarios écrits. Leur réussite sur l'état audité n'a pas été vérifiée |

Les `expect` portant sur des en-têtes constants ou sur l'écriture dans une `String` ne sont pas traités comme des défauts automatiques. Il faut examiner si l'invariant peut réellement être violé par une entrée externe. De même, une fonction longue n'est pas à elle seule une violation de clean code.

## Principes SOLID

SOLID désigne cinq principes de conception. Ils se lisent ici à travers les modules, les fonctions et les traits Rust. L'absence de classes ne constitue pas un écart.

| Principe | Signification | Observation dans ce projet | Conclusion |
| --- | --- | --- | --- |
| S | Une responsabilité principale par unité | Domaine, transport HTTP et I/O sont séparés. `authorize` mélange cependant contrôle d'accès et arbitrage métier de concurrence | Bien engagé à l'échelle des crates, à améliorer localement selon le constat 3 |
| O | Étendre sans modifier tous les comportements existants | Le protocole `atelier-connector/1` expose un manifeste et des capacités. Le catalogue d'intégrations lit ces capacités. Les fournisseurs intégrés et certaines règles HTTP demandent encore des modifications centrales | Partiel. Les extensions ont un point d'entrée concret, sans rendre toute la composition extensible |
| L | Remplacer une implémentation par une autre en conservant son contrat | `TimeRepository`, `TimeClock` et `TemplateRepository` expriment des contrats. Leurs paramètres dynamiques laissent toutefois des règles implicites de type | Non établi pour toutes les implémentations. Des tests de contrat communs seraient la preuve pertinente, pas un score théorique |
| I | Des interfaces limitées aux besoins de leurs appelants | Les traits Temps et Modèles sont dédiés à un cas d'usage. `CreationContext` expose les services nécessaires à la création | Respecté dans les exemples lus. Aucun bénéfice démontré à ajouter une interface à chaque fonction |
| D | Le métier dépend de contrats plutôt que d'adaptateurs concrets | Les traits vivent dans `atelier-core` et sont implémentés par `Store` dans `atelier-app`. Le noyau ne dépend pas d'Axum ni de rusqlite | Bon respect à la frontière principale. Le JSON générique affaiblit l'explicitation du contrat métier |

Références : [task_creation.rs](../../crates/atelier-core/src/application/task_creation.rs), [time_tracking.rs](../../crates/atelier-core/src/application/time_tracking.rs), [task_templates.rs](../../crates/atelier-core/src/application/task_templates.rs), [integrations.rs](../../crates/atelier-core/src/application/integrations.rs), [extensions.rs](../../crates/atelier-core/src/application/extensions.rs).

## Performance et lisibilité à qualifier

`Store::documents` charge toutes les lignes d'une table en mémoire. Le handler `/api/state` rassemble cartes, brainstorms, projets, exécutions et temps. [live.rs](../../crates/atelier-ui/src/live.rs) recharge cet état après les notifications et regroupe les demandes concurrentes. Cette combinaison reste simple pour une petite base locale, mais son coût croît avec l'historique total.

Aucune base volumineuse n'a été exécutée ni profilée. Ce point est donc un risque de croissance, pas une lenteur mesurée ni une obligation immédiate d'ajouter une pagination. Mesurer d'abord la taille de réponse, la mémoire et le temps de lecture avec un historique synthétique représentatif.

Des fichiers réunissent beaucoup de code, notamment `xtask/src/ui.rs`, le point d'entrée UI et certains adaptateurs. Les tests intégrés contribuent à leur taille. Aucun découpage n'est demandé sur la seule base du nombre de lignes. Séparer uniquement les responsabilités qui changent indépendamment et dont les dépendances sont établies.

Les limites commentées dans `i18n.rs`, `kanban.rs` ou les protocoles agents sont des décisions connues. Leur seule présence ne constitue pas un nouveau défaut. Aucun nombre de lignes ou de dépendances supprimables n'est annoncé sans preuve de non-utilisation.
