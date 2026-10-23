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

## Coffre — incrément après `f9b1598`

`token_vault.rs` conserve le format DPAPI UTF-16 Windows et l'enveloppe POSIX `keyring-aes-256-gcm-v1` (plateforme, IV 12 octets, tag 16 octets, données base64). Les commandes et identifiants `atelier-kanban` / `token-key` restent ceux de Node. La clé est transmise sur stdin, jamais dans les arguments ; les sorties sont bornées à 65 536 octets et le délai à 15 secondes. Les accès POSIX sont sérialisés pour éviter deux créations concurrentes dans le processus.

Le chiffrement précède l'écriture SQL ; les adaptateurs peuvent enregistrer ensemble paramètres et enveloppe dans la transaction existante. Un coffre vide ne nécessite pas de trousseau. Une erreur de chiffrement ou une transaction avortée conserve l'ancien contenu. Aucun repli en clair.

Réutilisation de ring 0.17.14 et base64 0.22.1 déjà présents dans le lockfile, désormais dépendances directes. [API AES-GCM ring](https://docs.rs/ring/0.17.14/ring/aead/struct.LessSafeKey.html) : nonce aléatoire neuf à chaque chiffrement, authentification avant restitution. Les primitives ne sont pas réimplémentées.

25 tests Rust locaux réussis ; comparaison des enveloppes Node ↔ Rust pour Linux/macOS avec clé synthétique, et du coffre SQLite/DPAPI réel sous Windows. Altération, mauvaise clé, clé absente, trousseau verrouillé, sortie excessive et rollback sont couverts. Deux vrais processus synthétiques vérifient stdin/EOF, saturation stderr et arrêt au délai de 15 secondes. Clippy natif sans avertissements. Suite Node : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 81,89 s. `cargo xtask test-keyring` et les étapes CI dédiées produiront la preuve native POSIX ; elle n'est pas acquise par ces doubles. Le compte Windows différent et les refus natifs des bureaux restent à qualifier.

## Suite du lot

### Imports, catalogue et installation — incrément après `33d594a`

Le catalogue filtre les noms historiques et fichiers réguliers, conserve dates UTC et tailles, crée sans écrasement et restaure avec secours nommé. L'import prépare un dossier privé adjacent, copie les pièces jointes sans liens, écarte les sessions périmées et snapshotte SQLite avec WAL. Il refuse les destinations remplies, identiques ou imbriquées, y compris un parent résolu par alias ; un échec nettoie le staging et ne publie pas de base partielle. Les chemins portable/user/system reprennent LOCALAPPDATA et XDG absolu, avec refus des installations macOS non portables.

29 tests Rust locaux réussis et Clippy propre. Le comparateur historique couvre maintenant aussi import, contenu complet, pièces jointes et sauvegarde, relus par Node. Suite Node : 425 tests, 422 réussis, zéro échec, trois exclusions inchangées, 82,18 s. L'import reste une fonction interne de test : son branchement au lanceur, le verrou commun, les courses sur ancêtres et la synchronisation de version dans le registre Windows restent ouverts.

La [CI du coffre `33d594a`](https://github.com/FluryWaana/atelier/actions/runs/40034806672) a réussi sur les cinq runners natifs, incluant le probe trousseau Linux x64/ARM64 et macOS x64/ARM64 : création, relecture, suppression, refus après suppression. Ces essais utilisent un service aléatoire synthétique, jamais la clé de production. Le verrouillage interactif et le changement de compte ne sont pas couverts par ce probe.

Compléter sessions/protocole HTTP du lanceur, chemins et verrou commun N1/Rust ; intégrer les adaptateurs aux cas d'usage et à l'intégration OS dans leurs lots. La compatibilité des données ne suffit pas à autoriser une migration de données utilisateur.
