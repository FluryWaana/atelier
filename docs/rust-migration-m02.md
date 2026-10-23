# M02 — données et protections locales

Incrément préparatoire du 24 octobre 2026, base `371a1a9`. **En cours** : les preuves natives M01 encore ouvertes ne sont pas réputées acquises. Les essais utilisent uniquement des dossiers synthétiques ; le serveur prototype continue de refuser les dossiers Node existants.

## Stockage porté

`atelier-app/infrastructure/store.rs` reprend le schéma `lib/store.mjs` : projects, cards, runs, time_entries, settings, logs et index logs_run. Les documents restent des valeurs JSON extensibles ; aucune normalisation des champs inconnus/absents/null. Les lectures suivent rowid décroissant, les UPSERT conservent cet ordre et les paramètres par défaut valent `{}`. Un enum fermé détermine les tables SQL : aucun nom de table fourni par une requête n'est interpolé.

Les logs conservent leur séquence, flux et texte, par pages de 500. La limite de 65 536 unités UTF-16 reproduit `slice` JavaScript, y compris le remplacement d'une moitié de surrogate lors de l'écriture SQLite. Les transactions restent une seule opération du thread dédié et reviennent en arrière sur erreur. Ce thread utilise une file bornée et se termine avant la libération de la base, même après abandon du futur appelant.

La sauvegarde utilise l'API SQLite pour inclure le WAL, écrit un fichier temporaire puis le publie sans écraser une destination existante. Elle ne remplace pas encore la restauration utilisateur : l'effacement du coffre et les règles de protection Tempo/reviews/automatisation restent à porter avant exposition de cette fonction.

## Preuves

- `cargo test --workspace --exclude atelier-ui --locked` : 16 tests locaux réussis, dont schéma, champs inconnus/null/précision, ordre UPSERT, pagination, transaction interrompue, corruption JSON/SQLite, saturation via `max_page_count`, snapshot WAL sans écrasement et fermeture après annulation d'un appel.
- `cargo xtask test-storage-parity` : Node → Rust → Node → Rust réussi sur 810 documents, paramètres et 10 002 logs. Tous les documents sont réécrits par Rust, des champs sont ajoutés par chaque version, et Node relit aussi la sauvegarde Rust. Les valeurs JSON complètes sont comparées ; seul le prototype interne nul des lignes SQLite Node est converti en objet JSON ordinaire.
- Rustfmt et Clippy natif sans avertissements. Suite Node avec les vrais scénarios de paquet : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 82,11 s. Parcours Edge complet réussi avec le stockage étendu.

Le comparateur Node est `test/fixtures/rust-storage-reference.mjs`, piloté par `xtask/src/storage.rs`. Il nécessite le runtime historique embarqué, ou `ATELIER_NODE` pour le désigner sur une autre plateforme. Cette dépendance de comparaison est transitoire ; les tests Rust courants n'exigent pas Node. Aucun résultat ni base personnelle n'est écrit dans le dépôt.

## Suite

Porter les protections de fichiers et sessions, le coffre complet, les restaurations et imports, les chemins d'installation et le verrou commun N1/Rust. Vérifier les trousseaux natifs et leurs refus avant de valider M02. La compatibilité du schéma et des documents ne suffit pas à autoriser une migration de données utilisateur.
