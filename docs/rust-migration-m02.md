# M02 — données et protections locales

Incrément préparatoire du 24 octobre 2026, base `371a1a9`. **En cours** : les preuves natives M01 encore ouvertes ne sont pas réputées acquises. Les essais utilisent uniquement des dossiers synthétiques ; le serveur prototype continue de refuser les dossiers Node existants.

## Stockage porté

`atelier-app/infrastructure/store.rs` reprend le schéma `lib/store.mjs` : projects, cards, runs, time_entries, settings, logs et index logs_run. Les documents restent des valeurs JSON extensibles ; aucune normalisation des champs inconnus/absents/null. Les lectures suivent rowid décroissant, les UPSERT conservent cet ordre et les paramètres par défaut valent `{}`. Un enum fermé détermine les tables SQL : aucun nom de table fourni par une requête n'est interpolé.

Les logs conservent leur séquence, flux et texte, par pages de 500. La limite de 65 536 unités UTF-16 reproduit `slice` JavaScript, y compris le remplacement d'une moitié de surrogate lors de l'écriture SQLite. Les transactions restent une seule opération du thread dédié et reviennent en arrière sur erreur. Ce thread utilise une file bornée et se termine avant la libération de la base, même après abandon du futur appelant.

La sauvegarde utilise l'API SQLite pour inclure le WAL, écrit un fichier temporaire puis le publie sans écraser une destination existante. La restauration prépare et vérifie une copie privée, refuse les opérations inachevées et la perte d'envois Tempo récents/incertains, sauvegarde l'état courant puis remplace les tables dans une transaction. Elle efface le coffre, invalide les reviews et interrompt les automatismes comme Node. Elle reste une fonction interne : catalogue, validation des chemins et exposition HTTP restent à porter.

`checked_file.rs` ouvre sans suivre le lien final, vérifie l'identité du descripteur et le nombre de liens, puis borne la lecture. Windows refuse les reparse points ; POSIX exige propriétaire courant et permissions privées pour la session. Les ancêtres et racines autorisées doivent encore être contrôlés par les adaptateurs de chemins.

## Preuves

- `cargo test --workspace --exclude atelier-ui --locked` : 20 tests locaux réussis, dont schéma, champs inconnus/null/précision, ordre UPSERT, pagination, transaction interrompue, corruption JSON/SQLite, saturation via `max_page_count`, snapshot WAL sans écrasement, fermeture après annulation, restauration avec faute SQL injectée, remplacement de fichier, liens physiques et jonction Windows.
- `cargo xtask test-storage-parity` : Node → Rust → Node → Rust réussi sur 810 documents, paramètres et 10 002 logs. Tous les documents sont réécrits par Rust, des champs sont ajoutés par chaque version, et Node relit aussi la sauvegarde Rust. Les valeurs JSON complètes sont comparées ; seul le prototype interne nul des lignes SQLite Node est converti en objet JSON ordinaire.
- Le même comparateur exécute maintenant la restauration Node et Rust depuis un état identique : documents, paramètres, logs, invalidation et contenu de la sauvegarde de secours identiques. Node vérifie aussi la suppression du coffre restauré et sa conservation dans le secours.
- Rustfmt et Clippy natif sans avertissements. Suite Node avec les vrais scénarios de paquet : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 81,80 s. Parcours Edge complet réussi avec le stockage étendu avant l'incrément restauration.

Le comparateur Node est `test/fixtures/rust-storage-reference.mjs`, piloté par `xtask/src/storage.rs`. Il nécessite le runtime historique embarqué, ou `ATELIER_NODE` pour le désigner sur une autre plateforme. Cette dépendance de comparaison est transitoire ; les tests Rust courants n'exigent pas Node. Aucun résultat ni base personnelle n'est écrit dans le dépôt.

## Suite

Compléter les chemins et sessions, le coffre, le catalogue de sauvegardes et les imports, les chemins d'installation et le verrou commun N1/Rust. Vérifier les trousseaux natifs et leurs refus avant de valider M02. La compatibilité du schéma et des documents ne suffit pas à autoriser une migration de données utilisateur.
