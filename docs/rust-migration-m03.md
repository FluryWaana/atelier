# M03 — Règles et API métier

## Validation des saisies de temps — base `de8083a`

`domain/time.rs` porte `lib/domain/time.mjs` sans dépendance supplémentaire. Les durées gardent les conversions JSON observables de `Number` (chaînes décimales/exponentielles/radix, booléens et tableaux), la borne 1–86 400 et l'ordre des erreurs. Les dates grégoriennes incluent l'année zéro et les siècles bissextiles ; une date hors plage conserve même l'erreur historique `Invalid time value`. Les heures restent au format strict `HH:mm:ss`.

Les descriptions passent directement par `Document`/`JsonString` : limite de 4 000 unités UTF-16 avant trim ECMAScript, conservation d'un substitut isolé prouvée par test et sérialisation. Aucun réglage, chronomètre ou envoi Tempo n'est modifié par ces fonctions pures.

Le corpus historique compte désormais 305 cas : les 246 précédents ont été comparés à HEAD et sont inchangés ; 59 nouvelles entrées proviennent de la fonction Node réelle. Bornes, conversions, priorités, dates impossibles, textes vides/longs et heures invalides sont comparés. Les commandes de chronomètre, persistance, concurrence et routes restent à porter.

Vérification : 44 tests Rust réussis, comparaison `test-domain-parity` réussie, formatage et Clippy natif/WASM propres ; suite Node complète avec paquets : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 82,01 s. Inventaire régénéré et contrôlé avant commit.

## Duplication de cartes — base `aa9bf62`

`application::duplicate_card` reprend uniquement les champs éditables prévus par Node, remet les critères à non validés et le statut à `todo`, sans ticket, PR, dépendances, worktrees, archivage, automatisation ni champ arbitraire. Le titre `Copie — …` est limité à 300 unités UTF-16, y compris lorsqu'une paire est coupée. Les textes et critères atypiques restent sans perte.

`POST /api/cards/{id}/duplicate` applique les protections locales existantes et renvoie 404 pour une carte absente. Lecture et insertion occupent une seule opération du worker SQLite ; aucune I/O distante ni lancement d'agent. Deux requêtes concurrentes créent deux identifiants distincts et laissent l'original inchangé.

42 tests Rust réussis, Clippy natif/WASM propre, comparaison historique complète réussie. Le corpus UTF-16 remplace la copie Node par la copie réellement calculée en Rust avant comparaison intégrale. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 83,69 s. Les autres opérations de tâches et leur exposition UI restent à porter.

État : en cours, règles pures et duplication HTTP portées ; autres API et cas d'usage à porter. M01 est requalifié après correction du test de réouverture ; M02 reste en cours.

## Règles de tâches et profils — base `fec382f`

`atelier-core/domain/{tasks,checklist,agent_options}` reprend critères, format de prompt, exigences de review, blocages/dépendances, fournisseurs par défaut et personnalisés, choix de modèle/effort et validation des profils. Les limites restent en unités UTF-16 et le trim conserve la sémantique ECMAScript. Les champs inconnus des critères/profils normalisés sont écartés comme dans Node ; les profils chargés gardent leurs champs additionnels.

Le parcours de dépendances est itératif : une chaîne de 10 000 cartes ne consomme pas la pile d'appels. Les cycles déjà présents sans lien vers la carte éditée terminent ; une nouvelle boucle vers cette carte est refusée. Une tâche archivée inachevée reste bloquante, et une référence manquante garde son diagnostic.

La validation des profils conserve les chemins absolus, refus réseau/périphériques, contrôles des arguments, paquets npm et identifiants réservés. Elle n'exécute aucun programme. Aucune nouvelle dépendance, aucun accès système dans core.

## Saisies et chronomètres locaux — après `3b4302e`

`application/time_tracking` porte création, modification, suppression, arrêt, remise en brouillon confirmée et reprise des entrées interrompues/envois incertains. Deux petits ports couvrent le stockage et l'horloge ; core reste sans dépendance native. Les adaptateurs SQLite et UTC réutilisent `Store` et `time` (fonction parsing activée, aucune bibliothèque ajoutée). Les champs additionnels et descriptions UTF-16 sont conservés. La liaison d'identifiant reprend aussi les conversions réelles Node/SQLite, y compris REAL/INTEGER et les erreurs de paramètres nommés.

Les routes locales `/api/time`, `/api/time/{id}` et `/api/time/{id}/stop|reset-export` s'exécutent dans une seule transaction du thread SQLite. Deux démarrages concurrents ne créent qu'un chronomètre actif. Le réveil à cinq secondes calcule le temps écoulé réel, conserve les fractions, plafonne à 24 heures et notifie SSE après commit. L'arrêt attend les requêtes puis enregistre les derniers temps avant fermeture SQLite ; la reprise interrompt les anciens chronomètres et rend les envois `sending` incertains. `state.times` expose les entrées, comme Node.

Les routes export/reconcile répondent explicitement 503 jusqu'au portage M05. Le port de garde d'export est testé, mais son branchement aux réservations distantes attend ce lot. L'interface de saisie reste en M07 ; les autres cas d'usage M03 et les protections M02 ouvertes ne sont pas déclarés terminés.

Sources confrontées : `lib/application/time-tracking.mjs`, `lib/time-tracking.mjs`, `lib/http/time-routes.mjs`, `lib/store.mjs`, `test/time-tracking.test.mjs` et routes d'état. `cargo xtask test-time-parity` reproduit 46 transitions capturées depuis ces vraies fonctions sur SQLite synthétique : erreurs et priorité des gardes, démarrage unique, fractions/recul d'horloge/plafond, arrêt des autres chronomètres, fermeture, champs additionnels/UTF-16, restart et reset confirmé. Un trigger SQL injecté prouve le rollback de toutes les entrées après faute lors d'un tick. Le scénario HTTP vérifie concurrence, modification/suppression, tick périodique/SSE, arrêt et redémarrage.

Contrôles Windows : 60 tests Rust réussis ; Clippy natif et WASM ; 305 contrats de domaine, 46 transitions de temps et comparateur stockage/import/restauration réussis. Parcours Edge avec WASM/CSP, redémarrage du processus, relecture et rotation de session réussi. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 427 tests, 424 réussis, zéro échec, trois exclusions, 81,145 s. Le test de réouverture historique a été adapté à l'ajout de `times: []`, sans supprimer ses vérifications de cartes. L'exécution native du nouvel incrément reste attendue en CI.

### Correction de comparaison Windows — après `07691c8`

Le [job Windows de `6529ea2`](https://github.com/FluryWaana/atelier/actions/runs/40051922174/job/120746252539) passe les tests Rust, puis échoue au comparateur Node : assertion textuelle CRLF contre LF après checkout Git. Le comparateur vérifie désormais les valeurs JSON des deux côtés, en conservant les chaînes et tableaux ; les attentes métier n'ont pas été régénérées. Contrôles locaux : les 46 transitions passent avec le fichier réellement converti temporairement en CRLF, puis avec ses octets originaux restaurés ; aucun diff du JSON de référence. Revalidation Windows en CI attendue.

## Archivage et suppression — après `e281b44`

`application/card_lifecycle` conserve l'ordre des gardes historiques : booléen d'archivage, activité/chronomètres/envoi, ticket incertain, file, dépendances, activité, historique de temps et worktrees. Les liens de cartes gardent l'égalité stricte de Node ; les anciens `blockedBy` chaînes gardent aussi leur comportement `includes`. Le nouveau scénario Node confirme ces cas et les erreurs avant de vérifier leurs équivalents Rust. Archivage/restauration préservent titre, champs inconnus et unités UTF-16.

`POST /api/cards/{id}/archive` et `DELETE /api/cards/{id}` lisent et écrivent dans une transaction unique du worker. Les erreurs métier gardent 400/404/409 ; une erreur de document/stockage ne divulgue pas son contenu. Le parcours HTTP vérifie refus pendant un chrono, archive puis refus de nouveau temps, restauration, conservation de l'historique après redémarrage, suppression seulement après retrait des temps et 404 après suppression. Notifications après commit par le middleware SSE.

Les gardes d'activité et de file sont testées dans core ; leur branchement aux producteurs M04/M06 reste explicitement requis lorsque le prototype permettra ces opérations. L'édition complète des cartes, modèles et autres cas d'usage M03 restent ouverts. Aucun agent n'est simulé comme actif ou terminé dans l'API.

Contrôles Windows : 63 tests Rust réussis, Clippy natif/WASM. Suite Node complète avec contrôle release : 428 tests, 425 réussis, aucun échec, trois exclusions, 83,291 s. Nouvelle CI de cet incrément attendue.

Preuves précédentes : journal macOS Intel `6529ea2` relu, transitions de temps et scénario HTTP réussis ; Windows attend la correction CRLF `e281b44`. Les journaux Linux ARM64/macOS ARM64 `07691c8` confirment le dossier privé, le contrôle de marqueur et la préparation après remplacement du chemin.

## Preuves reproductibles

`test/fixtures/rust-domain-reference.mjs` appelle les vraies fonctions historiques sur 175 entrées et capture leurs résultats ou messages d'erreur dans le JSON voisin. `cargo xtask test-domain-parity` vérifie que cette référence correspond toujours à Node puis exécute `atelier-core/tests/domain-contracts.rs`. Les tests Rust courants lisent le JSON et restent indépendants de Node ; le comparateur historique est aussi exécuté sur les cinq runners CI.

Les vecteurs couvrent notamment champs absents/null, Unicode, bornes des critères, cycles, profils disparus, héritage carte/fournisseur, changement d'agent, authentification CLI/API, chaînes hostiles et priorité des validations. Le test de chaîne longue complète les vecteurs. Clippy natif et WASM passent ; 37 tests Rust réussis. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 84,51 s.

## Limites et suite

### Réglages de projets — base `05dbfc8`

`domain/projects.rs` conserve les champs, valeurs par défaut, limites UTF-16, mappings Jira, profils et conditions d'automatisation de `lib/project-settings.mjs`. Les dossiers sont contrôlés selon les traversées Windows sur tous les OS, comme la référence ; sorties du worktree, racines et préfixes de lecteurs sont refusés. La comparaison de dépôts conserve casse/séparateurs Windows, sans normaliser les chemins POSIX ni inventer une équivalence de slash final. Un port explicite exige `{port}` dans la commande.

Le comparateur couvre maintenant 246 résultats/messages Node. L'héritage conserve les champs inconnus et priorités projet/global. Aucun service ni agent n'est lancé ; les adaptateurs devront revalider chemins réels et liens à l'accès au disque. Formatage, Clippy natif/WASM et 37 tests Rust passent. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 83,19 s.

Ces preuves concernent les règles et leurs messages, pas encore les codes HTTP, transactions, effets distants ou courses de mutations. Le serveur prototype conserve son périmètre M01. Porter ensuite les cas d'usage de tâches/projets/réglages/temps et leurs routes, avec réservations et revalidation après I/O, puis comparer les parcours HTTP. Les gardes d'archivage/suppression, worktrees, modèles et invalidation des reviews ne sont pas validés par ces seuls tests purs.
