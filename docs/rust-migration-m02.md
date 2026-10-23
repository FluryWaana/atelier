# M02 — données et protections locales

Incrément initial du 24 octobre 2026, base `371a1a9`. **En cours** ; M01 est désormais validé pour la faisabilité. Les essais utilisent uniquement des dossiers synthétiques ; le serveur prototype continue de refuser les dossiers Node existants.

## Stockage porté

### Conservation UTF-16 — incrément après `038e794`

`atelier-core/document.rs` réutilise `serde_json::RawValue` pour les valeurs inconnues et son désérialiseur d'octets WTF-8 pour les noms/chaînes UTF-16. Aucun parseur JSON ni remplacement silencieux de substitut ajouté. Les modifications ciblées conservent le JSON des champs non modifiés, y compris les clés atypiques, tableaux et objets imbriqués. Les lectures SQLite deviennent génériques ; les opérations qui doivent conserver ces documents utilisent `Document`. La restauration et ses mutations de paramètres/reviews/automatisation emploient ce chemin.

40 tests Rust réussis, formatage et Clippy natif/WASM propres. Les tests couvrent toutes les 65 536 unités UTF-16, les échappements, clés/valeurs imbriquées, champs null, SQLite et restauration. `cargo xtask test-storage-parity` exécute maintenant la vraie duplication Node qui coupe l'emoji, réécrit les quatre tables et les réglages en Rust, crée aussi des substituts depuis Rust, puis fait comparer documents, restauration et secours par Node. La comparaison historique complète antérieure passe aussi. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,32 s.

Limite restante : les contrats `Card` du prototype HTTP/UI et plusieurs règles métier utilisent encore `String`/`Value`. Leur adaptation est la prochaine étape ; ce stockage sans perte ne suffit pas à déclarer la chaîne entière compatible. Les preuves natives de chemins/coffres restent ouvertes.

**Écart de compatibilité identifié après `05dbfc8` :** un titre constitué de 291 `a` puis un emoji peut produire un substitut UTF-16 isolé lorsque Node duplique la carte et limite `Copie — …` à 300 unités. `JSON.stringify` conserve ce substitut sous la forme `\ud83d`, alors que `serde_json::Value` refuse sa lecture. Le probe local reproduit les deux comportements sur une chaîne synthétique ; aucune donnée utilisateur ouverte. Les aller-retour précédents ne couvrent pas ce cas. M02 reste non qualifié tant que les chaînes et clés JSON correspondantes ne sont pas conservées et comparées à Node. La [documentation du désérialiseur](https://docs.rs/serde_json/latest/serde_json/de/struct.Deserializer.html) distingue les chaînes UTF-8 des chaînes d'octets WTF-8 ; [RawValue](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html) permet la conservation de JSON brut. Évaluer ces mécanismes existants avant d'introduire un parseur ou une conversion avec perte.

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

### Canal local — incrément après `f9f53cf`

`LocalHttp` utilise exclusivement HTTP vers 127.0.0.1, sans proxy ni redirection, sans connexion persistante, avec délai et réponse bornée à 65 536 octets. La santé challenge/HMAC charge le fichier de session après la réponse et n'envoie Bearer qu'après validation de la preuve. Refus de connexion/socket fermé ne demandent pas de secret ; timeout, faux serveur et redirection restent des erreurs. Réutilisation de reqwest et du type d'erreur Hyper déjà verrouillés, sans comparer des chaînes de diagnostic.

Le bootstrap navigateur POSIX écrit un HTML privé atomique et ne place que son URL fichier dans les arguments ; Windows conserve l'URL à fragment. Les adresses et jetons sont validés. Le test UI utilise désormais ce même client pour santé et arrêt. 33 tests Rust réussis, Clippy propre et parcours Edge réel réussi ; suite Node 425 tests, 422 réussis, zéro échec, trois exclusions, 82,70 s.

Compléter chemins et verrou commun N1/Rust ; intégrer les adaptateurs aux cas d'usage et à l'intégration OS dans leurs lots. Le serveur de production, SSE, reprise navigateur et arrêt occupé ne sont pas remplacés par le prototype. La compatibilité des données ne suffit pas à autoriser une migration de données utilisateur.

### Verrou commun — incrément après `4afa807`

Le prototype détient désormais une transaction exclusive sur `.atelier-data-lock.sqlite`, distincte de la base applicative, avant d'ouvrir celle-ci. Le fichier reste en place ; aucun PID périmé ni expiration à interpréter. Node dispose du même adaptateur synchrone pour N1 ; son branchement au serveur de transition reste en M09. La production N0 ne respecte toujours pas ce verrou et doit être arrêtée avant migration.

Ce choix réutilise les [transactions SQLite](https://www.sqlite.org/lang_transaction.html) et leurs [verrous système](https://www.sqlite.org/lockingv3.html), disponibles dans les deux runtimes. Le chemin du dossier est canonique. Aucun descripteur ordinaire ne doit ouvrir puis fermer le fichier de verrou déjà détenu : sous POSIX cela peut libérer les verrous du processus. Les tests vérifient précisément qu'une acquisition refusée dans le même processus ne libère pas le premier verrou. Les fichiers de coordination sont exclus des imports des deux versions.

34 tests Rust locaux réussis ; comparaison Node/Rust avec exclusions réciproques, aliases, ports indépendants et libération après arrêt brutal Node puis du vrai serveur Rust. Clippy propre. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,39 s. La comparaison historique passe maintenant dans les cinq jobs natifs, sans attendre les outils de navigateur.

La CI de `f9f53cf` a détecté une permission trop large du dossier temporaire d'import sur les quatre runners POSIX : `tempfile` crée les dossiers avec 0777 filtré par umask. Le builder fixe désormais 0700 dès la création, avant toute copie ; le test de confidentialité est conservé. Cette correction et le verrou attendent leur validation native POSIX. Les courses hostiles sur les ancêtres et l'intégration N1 complète restent ouvertes.

### Registre Windows — incrément après `44dd895`

La synchronisation de DisplayVersion utilise les API Windows, sans PowerShell : clé HKCU 64 bits existante uniquement, InstallLocation résolu et comparé avec la casse ordinale Windows avant écriture. La clé absente ou une autre installation ne sont pas modifiées. Le test natif utilise exclusivement un sous-arbre temporaire `Software/AtelierKanbanTests/<uuid>` et le supprime : hausse/baisse de version, casse, séparateur final, chaîne littérale, valeur absente et REG_EXPAND_SZ. Le branchement au démarrage installé reste en M08.

35 tests Rust locaux réussis, Clippy propre ; suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions, 83,39 s. Les [cinq jobs natifs de `44dd895`](https://github.com/FluryWaana/atelier/actions/runs/40036942056) sont désormais réussis, comparaison Node/Rust comprise : le mode privé du staging et le verrou commun sont vérifiés sur Linux/macOS x64/ARM64 et Windows x64.
