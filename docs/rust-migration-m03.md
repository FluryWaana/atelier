# M03 — Règles et API métier

État : en cours, règles pures préparatoires ; API et cas d'usage complets à porter. Les prérequis M01/M02 non clos ne deviennent pas validés par cet incrément.

## Règles de tâches et profils — base `fec382f`

`atelier-core/domain/{tasks,checklist,agent_options}` reprend critères, format de prompt, exigences de review, blocages/dépendances, fournisseurs par défaut et personnalisés, choix de modèle/effort et validation des profils. Les limites restent en unités UTF-16 et le trim conserve la sémantique ECMAScript. Les champs inconnus des critères/profils normalisés sont écartés comme dans Node ; les profils chargés gardent leurs champs additionnels.

Le parcours de dépendances est itératif : une chaîne de 10 000 cartes ne consomme pas la pile d'appels. Les cycles déjà présents sans lien vers la carte éditée terminent ; une nouvelle boucle vers cette carte est refusée. Une tâche archivée inachevée reste bloquante, et une référence manquante garde son diagnostic.

La validation des profils conserve les chemins absolus, refus réseau/périphériques, contrôles des arguments, paquets npm et identifiants réservés. Elle n'exécute aucun programme. Aucune nouvelle dépendance, aucun accès système dans core.

## Preuves reproductibles

`test/fixtures/rust-domain-reference.mjs` appelle les vraies fonctions historiques sur 175 entrées et capture leurs résultats ou messages d'erreur dans le JSON voisin. `cargo xtask test-domain-parity` vérifie que cette référence correspond toujours à Node puis exécute `atelier-core/tests/domain-contracts.rs`. Les tests Rust courants lisent le JSON et restent indépendants de Node ; le comparateur historique est aussi exécuté sur les cinq runners CI.

Les vecteurs couvrent notamment champs absents/null, Unicode, bornes des critères, cycles, profils disparus, héritage carte/fournisseur, changement d'agent, authentification CLI/API, chaînes hostiles et priorité des validations. Le test de chaîne longue complète les vecteurs. Clippy natif et WASM passent ; 37 tests Rust réussis. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 84,51 s.

## Limites et suite

Ces preuves concernent les règles et leurs messages, pas encore les codes HTTP, transactions, effets distants ou courses de mutations. Le serveur prototype conserve son périmètre M01. Porter ensuite les cas d'usage de tâches/projets/réglages/temps et leurs routes, avec réservations et revalidation après I/O, puis comparer les parcours HTTP. Les gardes d'archivage/suppression, worktrees, modèles et invalidation des reviews ne sont pas validés par ces seuls tests purs.
