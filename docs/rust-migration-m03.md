# M03 — Règles et API métier

## Sélection des sources de détection — candidat après `544b294`

Le cas d'usage pur `projects::detect` porte la sélection de dépôts de `lib/application/projects.mjs` : projet absent prioritaire, rôle Front/Back strict, formulaire POST non sauvegardé, valeurs GET ignorées, effacement des dépôts dédiés, filtrage ou réaffectation des rôles, une analyse par chaîne de dépôt et déduplication ordonnée des avertissements. Résolution native des chemins et lecture des manifestes sont des ports explicites ; aucune I/O ni nouvelle dépendance dans core. Candidats, avertissements et chemins conservent leurs unités UTF-16.

24 attentes synthétiques explicites sont partagées par le test Rust sans Node et le comparateur appelant le véritable cas d'usage historique. Elles incluent erreurs avant résolution, absence de mutation du projet Node, source partagée, rôles dédiés, données de brouillon invalides ignorées hors rôle, échec d'analyse et substituts isolés. Les 70 opérations de cycle de vie précédentes restent inchangées. Le comparateur existant `test-project-lifecycle-parity` inclut cette référence ; la suite Node inclut aussi le scénario. Résultats Windows attendus ; aucune exécution locale. La route de détection conserve son indisponibilité explicite tant que le scanner natif M04 n'est pas implémenté : cette tranche ne prétend pas détecter réellement un projet.

## Interface actuelle sur le serveur Rust — candidat après `46338b8`

La [CI `0e7a252`](https://github.com/FluryWaana/atelier/actions/runs/40113045258) réussit dans les trois jobs Windows, journaux relus : routes inconnues 404, assets figés et privés refusés, Clippy natif/WASM, comparateurs historiques ; Node 440 tests, 436 réussis, aucun échec, quatre exclusions documentées. Les deux parcours Edge réussissent. L'artefact `rust-ui-Windows-X64` (ID 12567232088) contient cette fois les captures : création et restauration de l'interface historique ont été inspectées visuellement, texte HTML affiché littéralement, titre restauré et formulaire lisible. Le toast de fonction indisponible et la détection d'agents inachevée restent visibles : cette passerelle ne qualifie pas les fonctions M04–M06.

Le [journal Edge de `8028632`](https://github.com/FluryWaana/atelier/actions/runs/40112432242/job/120926772226) confirme les deux parcours complets : WASM M01 et interface historique sur Rust, avec création/édition, sauvegarde/restauration et redémarrage. La suite Node réussit (440/436/0/4). Le job Rust échoue sur `/api/missing` : le fallback 503 était trop large. Il est restreint aux familles de fonctions non portées ; une route inconnue conserve 404, et l'assertion existante est maintenue. La collecte des captures doit inclure le dossier caché `.artifacts`, avec un motif limité aux PNG synthétiques ; aucune capture de cette première exécution n'est disponible pour revue. Nouvel essai complet attendu.

`cargo xtask build-legacy-ui` copie les seuls assets publics exposés par Node dans `.artifacts/rust-legacy-ui`. Le même `serve-prototype --data-dir <dossier synthétique vide> --assets .artifacts/rust-legacy-ui --port 4318` peut alors afficher l'interface actuelle. Aucun Node lancé par ce serveur, aucune préférence produit ajoutée, aucun changement des paquets. `state.runs` lit désormais les vrais documents stockés, nécessaires à ces vues. Les fonctions non portées renvoient un JSON 503 après authentification, au lieu d'une page 404 sans explication.

Les assets natifs et historiques sont lus une fois au démarrage à travers `CheckedFile` dans le répertoire ouvert, avec une limite de 64 Mio par fichier ; leurs octets sont partagés entre réponses sans réouverture par chemin. Une liste explicite inclut les seuls modules de domaine utilisés par le navigateur ; aucune route générique vers `lib/` ou `data/`. Un test HTTP vérifie MIME, liste autorisée, refus des fichiers privés et conservation du contenu si le chemin est remplacé ensuite.

Le nouveau parcours WebDriver `test-legacy-ui` manipule l'interface réelle : session retirée du fragment, SSE, titre hostile échappé, création/édition, sauvegarde/restauration avec confirmation et redémarrage. Des captures synthétiques sont conservées par la CI Windows. Ce scénario reste candidat tant que le journal et les captures ne sont pas recueillis. Aucun navigateur lancé sur le poste ; étapes Linux/macOS inchangées, extension du nouveau parcours réservée à la qualification finale.

Cette tranche ne qualifie pas encore toute l'interface métier : catalogue des connecteurs, diagnostics d'agents, files actives, suggestion de branche et autres effets M04–M06 restent ouverts. L'interface WASM minimale et son parcours M01 restent vérifiés séparément ; M07 n'est pas réalisé par cette passerelle.

## API de sauvegarde — candidat après `fbfac16`

La [CI Windows du correctif `46338b8`](https://github.com/FluryWaana/atelier/actions/runs/40111589208) réussit intégralement, journaux relus : 87 tests Rust, dont garde des sauvegardes et restauration HTTP avec panne finale/rollback, Clippy natif/WASM et comparateurs historiques ; suite Node 440 tests, 436 réussis, aucun échec, quatre exclusions documentées ; parcours Edge M01 réussi. Le parcours de l'interface historique ci-dessus n'appartient pas encore à cette preuve.

Le premier [essai CI `589bdba`](https://github.com/FluryWaana/atelier/actions/runs/40111051509) échoue dans les deux nouveaux scénarios avant leurs assertions de restauration : leur chrono synthétique fournit `seconds: 0`, refusé par la validation historique même avec `start: true`. La fixture utilise désormais 60 secondes ; les validations produit restent inchangées. Nouvelle exécution complète attendue.

GET/POST `/api/backups` et POST `/api/backups/restore` réutilisent le répertoire ancré M02. Gardes métier, confirmation strictement booléenne et messages sont confrontés à `server.mjs`, `lib/backups.mjs` et `lib/task-queue.mjs`. Le contrôle des chronomètres/envois et l'opération restent dans un seul travail SQLite : une mutation concurrente ne peut pas s'intercaler entre contrôle et snapshot/restauration. Les réservations de fournisseurs, processus, connecteurs et file active restent à raccorder avec leurs producteurs M04–M06 ; ces opérations ne sont pas activées dans le prototype.

La restauration conserve la sauvegarde de secours et inclut maintenant la remise en pause de la file dans sa transaction, avec l'invalidation des jetons, reviews et automatisations. La référence Node remet sa file en pause après le commit : Rust renforce l'atomicité en annulant aussi le remplacement des cartes et l'effacement du coffre si cette dernière écriture échoue. Il n'existe pas encore de cache de jetons à purger en mémoire. Le helper de spread JSON déjà présent dans la restauration est déplacé dans `Document` et réutilisé pour préserver les champs inconnus et UTF-16 de la file.

Nouveaux scénarios à vérifier en CI Windows : parcours HTTP Node/Rust avec liste, confirmation, noms sortants, chrono actif, sauvegarde de secours et état restauré ; côté Rust, panne SQL injectée à la dernière écriture de la file, conservation des secrets courants et réouverture. Les erreurs métier gardent 400/409 ; une faute d'I/O/SQL interne reçoit un 500 générique, sans détail privé, au lieu du diagnostic brut 400 de Node. JSON de sauvegarde invalide reçoit un 400 générique. Aucun contrôle exécuté sur le poste.

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

## Modèles de tâches — après `ec85b26`

`application/task_templates` et les routes POST/DELETE `/api/templates` reprennent la validation du nom, la copie des seuls champs éditables, la remise à zéro des critères et la limite de 50 modèles. Chaque mutation tient dans une transaction SQLite et notifie après commit. Les réglages additionnels, la file et les chaînes UTF-16 restent intacts ; le retrait d'un identifiant absent est sans effet sur les autres modèles. La conversion des identifiants SQLite est partagée avec les temps sans changement de contrat.

Sources confrontées : `lib/application/tasks.mjs`, `lib/http/task-routes.mjs`, `lib/store.mjs` et `test/board-use-cases.test.mjs`. Le scénario Node équivalent et le test SQLite Rust vérifient les bornes et la conservation des données. Le test HTTP lance 55 créations concurrentes (50 succès, cinq refus), redémarre le serveur puis vérifie retrait, remplacement, limite et carte absente. L'application des modèles à la création complète, les réglages publics et leur interface restent à porter.

Contrôles locaux : 65 tests Rust réussis, Clippy natif/WASM réussi (avertissements de copie du cache de compilation uniquement). Suite Node complète avec contrôle release : 429 tests, 426 réussis, aucun échec, trois exclusions, 85,001 s. Les nouvelles preuves natives de cet incrément restent attendues.

La [CI `ec85b26`](https://github.com/FluryWaana/atelier/actions/runs/40053199326) est entièrement réussie. Journaux des cinq jobs natifs relus : les 46 transitions de temps, le parcours HTTP et les deux gardes de cycle de vie passent ; les quatre cibles POSIX passent la préparation privée après remplacement du chemin. Le journal Windows confirme la correction du comparateur CRLF. Les cinq journaux confirment aussi le refus entre comptes des coffres, avec relecture finale du second secret sur macOS. Ces preuves ne qualifient pas les fonctionnalités encore absentes.

## Création de tâches et brainstorms dans core — après `1b9a9f7`

`application/task_creation` porte les champs complets, l'héritage et les remplacements explicites des modèles, le choix de fournisseur/modèle/effort, les colonnes initiales et les particularités des brainstorms. Le port explicite fournit configuration, agent de projet, résolution du dépôt, identifiant et horloge ; core ne lit aucun fichier et ne démarre aucun agent. La priorité des validations et même l'ordre des appels identifiant/dépôt/horloge sont comparés à Node. Le branchement HTTP et les adaptateurs réels sont l'incrément suivant ; la route de création du prototype reste pour l'instant limitée au titre.

Les critères et leur rendu de prompt utilisent désormais `JsonString` plutôt que `String`, pour conserver les substituts UTF-16 isolés acceptés par Node. Les validations existantes restent partagées, notamment les options d'agent. Les 305 contrats historiques restent inchangés et passent. Le nouveau corpus `rust-task-creation-reference` capture 77 créations depuis `createTasks` : succès, erreurs, champs ignorés, modèles absents, limites, priorité des erreurs, textes/critères UTF-16 et échec synthétique de résolution du dépôt. Les réponses et le prompt sont comparés sans conversion UTF-8 avec perte. `cargo xtask test-task-creation-parity` est ajouté aux cinq runners natifs ; les tests core ordinaires n'utilisent pas Node.

Contrôles Windows : 66 tests Rust réussis, Clippy natif/WASM et comparateurs historiques réussis. Suite Node complète avec contrôle release : 429 tests, 426 réussis, aucun échec, trois exclusions, 85,141 s. Le résolveur de dépôt de ce corpus est synthétique ; il ne prouve ni les accès natifs ni la sélection réelle du projet.

La [CI des modèles `1b9a9f7`](https://github.com/FluryWaana/atelier/actions/runs/40097251427) est entièrement réussie. Les journaux Windows, Linux x64/ARM64 et macOS Intel/ARM64 confirment le scénario HTTP concurrent avec redémarrage et la conservation SQLite des réglages/UTF-16. La CI du nouveau portage de création reste attendue.

## Création HTTP et dépôts natifs — après `e027e70`

POST `/api/cards` et `/api/brainstorms` utilisent désormais le cas d'usage complet dans une transaction du worker SQLite. La sélection de l'agent de projet précède la résolution du dépôt, comme Node ; les réglages ne sont pas convertis globalement en UTF-8 et leurs champs inconnus restent intacts. Les chemins explicites sont trimés, résolus et contrôlés comme dossiers ; Windows retire le préfixe technique de chemin étendu avant stockage. La comparaison de dépôts garde casse/séparateurs Windows et UTF-16 sans perte. Aucun agent n'est démarré par une création en colonne `doing`.

L'ancien constructeur M01 limité au titre et son test de refus des autres champs sont retirés : leurs validations de titre figurent dans le comparateur complet et le scénario HTTP existant. Les 77 attentes précédentes sont inchangées ; sept scénarios issus de `settingsForProject` portent le corpus à 84 créations. Le test HTTP Node équivalent et le test Rust vérifient héritage, overrides, dossiers Unicode, refus d'un fichier/dossier absent/NUL, modèles absents et options invalides. Rust vérifie en plus deux créations concurrentes, une panne SQL injectée sans écriture partielle ni détail privé en réponse, et la relecture des critères UTF-16 après redémarrage.

Contrôles Windows : 66 tests Rust réussis ; 305 contrats de domaine et 84 créations historiques vérifiés ; Clippy natif/WASM réussi. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 430 tests, 427 réussis, aucun échec, trois exclusions, 84,745 s. Parcours Edge réel réussi après rebuild WASM : CSP, création, redémarrage du processus, relecture et rotation de session.

Limites : les messages des erreurs système de chemins proviennent de Rust/OS, sans qualification à l'octet des diagnostics Node `lstat` ; les statuts de refus et l'absence d'écriture sont testés. Les pannes de stockage répondent 500 avec message public générique. L'édition, les réglages/projets HTTP et l'interface de création complète restent ouverts. Les protections SQLite M02 déjà signalées restent ouvertes ; le prototype n'est pas qualifié pour des données réelles.

Journaux `e027e70` Windows, Linux x64/ARM64 et macOS ARM64 relus : comparateur de 77 créations et test Rust correspondants réussis. macOS Intel et la nouvelle CI du branchement HTTP restent attendus.

## Préparation des éditions dans core — après `567a529`

`application/task_editing` prépare une carte éditée et distingue sauvegarde locale, lancement et synchronisation. Les gardes archive/activité/worktree conservent leur priorité ; le ticket garde autorité sur titre/description, le changement d'agent remet les options à leur valeur historique et marque le choix explicite. Les dépendances utilisent le parcours de graphe existant. Les champs inconnus et textes UTF-16 restent préservés. Une modification des coches seules ne périme pas une review ; titre, description, instructions ou texte des critères le font.

Les validations de texte/fournisseur/options sont partagées avec la création. Le corpus `rust-task-editing-reference` capture 63 modifications par la vraie fonction Node, puis compare en Rust valeurs, état sauvegardé et ordre des effets : résolution, horodatage, écriture/lancement, invalidation et synchronisation. Les erreurs de dépôt et de lancement synthétiques ne publient pas de carte modifiée. Les gardes manquante/archivée/occupée, les changements de branches sous worktree, Jira, les cycles et les valeurs null/absentes sont couverts. L'aide de comparaison JSON UTF-16 est partagée par les deux suites core.

Ce corpus ne lance aucun agent et ne synchronise aucun ticket distant. Le branchement HTTP/SQLite et l'invalidation réelle des reviews restent l'incrément suivant ; M04/M05 devront réserver les opérations, revalider après I/O et comparer les exigences après le lancement réel. `cargo xtask test-task-editing-parity` est ajouté aux cinq cibles CI ; les tests core ordinaires lisent seulement le JSON commis.

Contrôles locaux : 67 tests Rust réussis, Clippy natif/WASM et comparateur historique d'édition réussis. Suite Node complète avec contrôle release : 430 tests, 427 réussis, aucun échec, trois exclusions, 86,634 s. Les 305 contrats de domaine et 84 créations précédents restent couverts par les tests Rust. La CI `e027e70` est entièrement réussie, journal macOS Intel relu pour les 77 créations. Les journaux Windows, Linux x64/ARM64 et macOS ARM64 de `567a529` confirment 84 créations et le scénario HTTP avec chemins natifs/panne SQL/reprise ; macOS Intel encore attendu.

## Édition HTTP locale et invalidation transactionnelle — après `0ac6993`

PATCH `/api/cards/{id}` applique les règles d'édition dans une transaction unique du worker SQLite. La modification de la carte, les motifs d'obsolescence des seules reviews PR associées et l'interruption des automatisations `ready`/`blocked` sont atomiques. Les champs additionnels des cartes, runs et automatisations restent intacts. Les coches seules ne périment pas une review, conformément au scénario exécuté dans le serveur Node réel.

Une transition nécessitant un lancement d'agent ou une synchronisation de ticket répond 503 avant toute écriture jusqu'à M04/M05. Le branchement à l'activité réelle reste explicitement requis lors de M04/M06 ; le prototype ne peut pas lancer ces opérations. Les éditions locales de notes restent possibles sur les cartes Jira, sans modifier leur titre/description. Une erreur SQL/document répond 500 avec message public générique.

Le parcours HTTP synthétique vérifie deux modifications concurrentes sans perte, guards archive/worktree/dépendances, 404, refus 503 sans écriture, critères UTF-16 et reprise. Un trigger rejetant l'invalidation d'une review annule la modification de carte déjà exécutée dans la transaction, conserve son horodatage/automatisation et ne divulgue pas le détail de la faute. Le scénario Node ajouté confirme les reviews concernées, la conservation UTF-16 et les transitions d'automatisation réussies ; la garantie transactionnelle renforcée est testée côté Rust.

Contrôles locaux : 68 tests Rust réussis, Clippy natif/WASM réussi ; 305 contrats de domaine, 84 créations, 63 modifications et 46 transitions de temps couverts par les tests Rust. Suite Node complète avec contrôle release : 431 tests, 428 réussis, aucun échec, trois exclusions, 85,370 s. La CI `567a529` est entièrement réussie ; le journal macOS Intel confirme les 84 créations et le scénario HTTP natif. Les journaux Linux ARM64/macOS ARM64 `0ac6993` confirment les 63 modifications historiques. Les autres résultats de cet incrément et la nouvelle CI de l'édition HTTP restent attendus.

## Séparation des brainstorms dans l'état — après `d86c57c`

La confrontation de `/api/state` à `server.mjs` a révélé que les brainstorms créés par la nouvelle route restaient dans `cards` côté Rust. Le serveur les range désormais dans `brainstorms` selon la même condition de vérité que Node, tout en préservant l'ordre de stockage et les documents. Le contrat partagé ajoute cette liste avec une valeur par défaut à la désérialisation. Le parcours de création Node vérifie deux cartes ordinaires et un brainstorm ; le scénario Rust vérifie aussi cette séparation après redémarrage. Le test d'état M01 est adapté sans retirer sa vérification de persistance.

Contrôles complets : 68 tests Rust et Clippy natif/WASM réussis ; parcours réel Edge (WASM/CSP, création, redémarrage et relecture) réussi. Suite Node avec contrôle release : 431 tests, 428 réussis, aucun échec, trois exclusions, 85,844 s. Le frontend complet des brainstorms reste en M07 ; cet incrément corrige le contrat de données. Les journaux Windows/Linux x64 `0ac6993` sont relus et confirment les 63 éditions ; macOS Intel reste attendu, ainsi que la CI de l'édition HTTP `d86c57c`.

## Création, archivage et retrait des projets — après `df8b42c`

`application/projects` et ses adaptateurs portent la création de projet, l'archivage des cartes d'un dépôt et le retrait avec option d'archivage. Nom, repo, doublons et ordre des gardes sont conservés ; cartes archivées déjà présentes, champs inconnus, unités UTF-16 et historique de temps restent préservés. L'archivage d'un dépôt orphelin compare le chemin enregistré sans exiger que ce dossier existe encore. `state.projects` expose les documents comme Node.

Chaque commande HTTP s'exécute dans une seule transaction du worker. Deux créations concurrentes du même dossier donnent un succès et un conflit. Un chrono actif ou un envoi `sending` empêche l'archivage ; retirer le projet sans archivage conserve les cartes. La suppression du projet et l'archivage demandé sont atomiques : un trigger de suppression en échec annule les cartes déjà archivées, sans exposer le détail de la faute SQL. Cela renforce la référence Node, dont la suppression suit la transaction d'archivage.

`cargo xtask test-project-lifecycle-parity` compare 39 opérations capturées depuis `createProjects` : validation, casse par plateforme, portée d'archivage, activité/serveurs simulés, chronomètres/envois, champs additionnels/UTF-16 et faute d'écriture. Le parcours HTTP Node équivalent et le parcours Rust vérifient doublons, timer, conservation des cartes/temps, dépôt devenu absent ; Rust vérifie aussi rollback de suppression et reprises successives. Le comparateur est ajouté aux cinq cibles CI.

La comparaison native révèle aussi que Node résout un chemin vide après nettoyage vers le dossier courant, alors que Rust le refusait. Le résolveur partagé conserve désormais ce comportement. Les tests HTTP Node et Rust vérifient création de projet, création et édition de carte dans un processus enfant dont le dossier courant est temporaire et synthétique, sans changer celui des autres tests.

Les réglages/déplacements de projets et la détection de lancement restent à porter ; leurs routes répondent explicitement 503 pour un projet existant, 404 sinon. Les gardes de processus/serveurs sont éprouvées dans core mais restent à relier aux producteurs M04/M06 avant activation. Aucun dossier utilisateur ni fichier de dépôt n'est déplacé par ces commandes.

Contrôles locaux : 71 tests Rust réussis, Clippy natif/WASM et comparateur des 39 opérations réussis. Parcours Edge réel réussi après rebuild WASM : CSP, création, redémarrage et relecture. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 433 tests, 430 réussis, aucun échec, trois exclusions, 138,020 s. Les vérifications sur ce poste suivent désormais le budget demandé : un job Cargo, un thread de test, priorité BelowNormal et deux processeurs logiques ; aucune commande lourde en parallèle. La suite Node utilise `--test-concurrency=1`. La CI distante garde sa concurrence.

La [CI `1c49fb4`](https://github.com/FluryWaana/atelier/actions/runs/40100250745) est entièrement réussie. Les cinq journaux natifs confirment les scénarios HTTP d'édition atomique et de création avec séparation des brainstorms, ainsi que les 63 modifications historiques. Les CI `0ac6993` et `d86c57c` sont également terminées avec succès. La nouvelle preuve native du cycle de vie des projets reste attendue.

## Report des contrôles Linux/macOS — après `5442e2c`

À la demande de l'utilisateur, les nouveaux essais Linux/macOS sont réservés à la qualification finale. La CI `40102490207` a été annulée : les jobs POSIX avaient terminé avant la prise en compte de l'annulation, le job Windows a été interrompu. Cette interruption n'est pas un échec fonctionnel ni une preuve de réussite. Les journaux déjà lus pour Linux x64/ARM64 et macOS ARM64 confirment les 39 opérations de projets, le parcours HTTP transactionnel et les chemins vides résolus dans un dossier synthétique. Les résultats non recueillis et les futurs changements devront être revérifiés en fin de migration.

La CI Rust des prochains pushes ne lance que Windows. Les cinq cibles natives et les navigateurs Linux/macOS restent disponibles via `workflow_dispatch` avec `full_matrix: true`, à utiliser lors de la qualification finale. Aucun scénario ni critère final n'est supprimé.

## Réglages de projet sans perte UTF-16 — après `d2deabe`

La préparation de l'édition des projets révèle que `domain/projects` utilisait encore `serde_json::Value` et `String` pour les textes. La validation prend désormais des `Document` et produit des réglages typés sérialisables dont les textes sont des `JsonString`. Règles, branches, commandes, chemins relatifs, mappings Jira et identifiants restent sans perte ; champs inconnus dans l'entrée écartés comme Node. La recherche d'un fournisseur est partagée avec création/édition de tâches, sans déplacement d'adaptateur dans core ni nouvelle dépendance.

Le corpus `rust-project-settings-reference` ajoute 51 cas capturés par la vraie fonction `projectSettings` : substituts isolés, bornes UTF-16, trim, traversées, ports/placeholder, fournisseur et priorités entre erreurs. Les 305 vecteurs historiques sont inchangés ; les 84 créations et 63 éditions de tâches gardent leurs résultats. `cargo xtask test-domain-parity` vérifie aussi ce nouveau corpus. Les tests ordinaires lisent les JSON commis sans Node.

Cet incrément porte la validation pure ; PATCH projet et déplacement avec sauvegarde restent à brancher. Contrôles Windows : 72 tests Rust réussis, Clippy natif/WASM et comparateur de 305 + 51 résultats réussis. Suite Node avec `ATELIER_REQUIRE_RELEASE=1`, en série : 433 tests, 430 réussis, aucun échec, trois exclusions, 139,306 s. Inventaire régénéré et vérifié. La CI est désormais limitée à Windows ; la vérification des changements sur Linux/macOS reste reportée à la fin, conformément à la demande de l'utilisateur.

## Édition et déplacement des projets — après `8e73f7c`

PATCH `/api/projects/{id}` applique désormais les réglages partiels, les validations de fournisseurs/chemins et les gardes de projets/worktrees avant toute écriture. Un changement de dépôt produit une sauvegarde SQLite avant la transaction ; celle-ci regroupe projet, cartes associées (archives comprises), motifs d'obsolescence des reviews et interruption des automatisations prêtes/bloquées. Les historiques de runs gardent leur dépôt précédent. Aucun fichier du dépôt n'est déplacé.

Le répertoire de sauvegarde est ouvert à partir du handle du répertoire du prototype, puis conservé pendant le serveur. Une panne de sauvegarde interrompt la commande avant toute mutation. Les erreurs de sauvegarde/SQLite répondent 500 avec message public générique. La fusion de documents conserve les clés inconnues UTF-16 ; le comparateur commun a aussi été corrigé pour comparer ces clés sans passage forcé en UTF-8. La fonction d'invalidation existante est partagée avec l'édition des cartes.

Les 31 nouvelles opérations portent le corpus à 70 ; les 39 précédentes sont inchangées. Elles vérifient notamment héritage, champs ignorés, dépôts de services, projet lié, priorité des gardes, casse, sauvegarde et ordre d'écriture/invalidation, y compris des erreurs injectées. Le test HTTP Node équivalent relit le snapshot précédant le déplacement. Le parcours Windows Rust vérifie deux éditions concurrentes conservées, les gardes publiques, la sauvegarde, la panne pendant l'invalidation annulant toutes les écritures, le refus de sauvegarde via permissions temporaires du seul dossier synthétique, puis le redémarrage. Ces permissions sont restaurées après la requête.

Détection de lancement toujours indisponible ; activité, serveurs et connecteurs devront être reliés aux producteurs M04–M06. Le prototype refuse un connecteur explicite non disponible. Les nouveaux scénarios Linux/macOS restent reportés à la qualification finale. Contrôles Windows : 73 tests Rust, comparateur des 70 opérations, Clippy natif/WASM et parcours Edge réel réussis. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1`, en série : 434 tests, 431 réussis, aucun échec, trois exclusions, 137,671 s. Inventaire régénéré et vérifié ; mêmes limites locales de ressources.

La [CI Windows `8e73f7c`](https://github.com/FluryWaana/atelier/actions/runs/40103677444) est réussie. Son journal confirme les 51 réglages UTF-16, les 39 opérations de cycle de vie précédentes et les parcours HTTP de projets/chemins vides. La CI de report `d2deabe` est également réussie, avec Windows seul et le job de navigateurs ignoré. Les preuves du présent incrément restent à recueillir après son push.

La CI Windows `38ceb6c` a ensuite détecté une fixture qui enregistrait les chemins temporaires bruts du runner au lieu des chemins résolus normalement enregistrés par l'application : le doublon attendu n'était pas reconnu. La fixture utilise désormais le résolveur réel pour préparer ces dépôts et conserve cette valeur pour vérifier les snapshots. La nouvelle CI Node `dd57bfa` a aussi révélé que `--check` comparait littéralement les CRLF du checkout à la sortie LF du générateur ; seuls les séparateurs de lignes sont maintenant normalisés, sans tolérer un inventaire périmé. Corrections candidates, preuves distantes attendues. Aucun test local n'est relancé.

## Preuves reproductibles

### Profils d'agents — candidat après `c59d929`

Qualification Windows de `bcb158d` : les [trois jobs CI](https://github.com/FluryWaana/atelier/actions/runs/40109486980) sont réussis, journaux relus. Les 83 tests Rust incluent le CRUD HTTP des profils, les références empêchant leur retrait, le rollback SQLite/coffre et les 18 contrats UTF-16 historiques supplémentaires. Clippy natif/WASM, comparateurs historiques et DPAPI entre comptes passent. Node avec `ATELIER_REQUIRE_RELEASE=1` : 439 tests, 435 réussis, aucun échec, quatre exclusions déjà documentées. Edge vérifie création, redémarrage, persistance et rotation de session. Les réservations et diagnostics CLI M04 restent ouverts.

Le validateur de profils reçoit maintenant un document sans perte : nom, chemins, libellé et arguments conservent les unités UTF-16 isolées. La classification des chemins ne transforme pas les valeurs sauvegardées. Les options modèle/effort partagent leur validation ASCII avec la création/édition de cartes et les réglages globaux. Les 305 anciens contrats restent inchangés ; 18 cas supplémentaires comparent les textes, arguments et priorités de refus avec Node.

`application/agent_providers` prépare ajout, remplacement et suppression, avec type immuable, limite de 50, validation des clés, références de cartes/projets (archives comprises) et gardes d'activité. Les routes GET/POST `/api/agent-providers` et DELETE `/api/agent-providers/{id}` réutilisent le même commit atomique réglages/coffre. Les descriptions n'exposent que la présence de la clé. Les tests de cas d'usage Node et Rust reproduisent les mêmes gardes ; le scénario HTTP Windows couvre chiffrement, héritage, refus de suppression, réouverture et rollback injecté. Résultats de cet incrément attendus sur GitHub.

Les diagnostics, informations de mise à jour, lancement CLI et réservations d'activité restent à brancher en M04 ; l'API ne prétend pas les avoir exécutés. Le coffre du prototype POSIX conserve son refus explicite jusqu'au service isolé requis en M02. Aucun programme d'agent ni appel payant lancé.

Preuve recueillie pour `76ed18c`/`c59d929` : [suite Node Windows](https://github.com/FluryWaana/atelier/actions/runs/40108724090/job/120915769191) réussie, journal relu : 437 tests, 433 réussis, aucun échec, quatre exclusions inchangées. Les trois remplacements natifs et les trois variantes d'identifiants au-delà de la précision des `Number` sont réussis. Les tests asynchrones de lecture de fichiers passent aussi.

La CI `c59d929` est maintenant entièrement réussie. Journaux natif et Edge relus : 79 tests Rust, 42 réglages globaux et 23 URL comparés à Node, rollback du coffre/réglages, concurrence HTTP avec modèles, conservation UTF-16, réouverture, Clippy natif/WASM et parcours Edge réussis. Cela qualifie cet incrément Windows ; les nouveaux profils ci-dessus, l'intégration POSIX et les producteurs M04/M05 ne sont pas couverts par cette exécution.

### Réglages globaux HTTP et coffre atomique — candidat après `c613dc2`

PUT `/api/settings`, DELETE `/api/settings/tokens/{service}` et `state.settings` utilisent le préparateur historique. Lecture, préparation, chiffrement et transaction occupent une seule opération du worker SQLite : aucune écriture de modèle/réglage ne s'intercale, et aucun verrou SQLite de transaction n'est détenu pendant le chiffrement. Les jetons sont relus depuis le coffre, sans cache mutable distinct susceptible de survivre à un rollback. Une faute de chiffrement refuse la commande ; une faute d'écriture annule coffre et réglages ensemble. La réponse expose uniquement les indicateurs de présence des jetons, avec les champs de configuration historiques.

L'adaptateur d'URL réutilise `reqwest::Url` déjà présent. Les contrats [URL Node](https://nodejs.org/api/url.html#the-whatwg-url-api) et [URL Rust](https://docs.rs/url/2.5.8/url/struct.Url.html) sont confrontés à 23 cas : HTTPS, identifiants, paramètres, fragments vides/non vides, chemins, IPv6 et substitution USV des unités UTF-16 isolées. Aucun appel Jira réel. Le test HTTP Windows utilise uniquement DPAPI sur des secrets synthétiques et vérifie héritage du dépôt, concurrence avec les modèles, réouverture, oubli et rollback après trigger SQL. Le test d'état historique conserve toutes ses assertions en ajoutant le champ public `settings`.

Limite explicite : le prototype POSIX refuse encore les écritures nécessitant un coffre, avant mutation, pour ne jamais appeler le service de production du trousseau. Un service isolé doit être branché avant qualification POSIX. L'overlay de configuration des connecteurs reste lié à M05. Tous les nouveaux résultats sont attendus en CI, aucun test local exécuté.

La [CI `9a6721d`](https://github.com/FluryWaana/atelier/actions/runs/40106480434) est entièrement réussie ; le journal Edge confirme WASM/CSP, création, redémarrage, relecture et rotation. La CI suivante `c613dc2` a passé les tests Rust et Clippy, puis signalé trois défauts des lectures de fichiers Node ; correction exacte des identifiants décrite en M02, sans retirer ces scénarios. Les comparaisons historiques du nouveau préparateur restent attendues.

### Réglages globaux — candidat après `9a6721d`

`application/settings.rs` prépare les paramètres et les jetons sans accès système ni écriture : défauts, héritage des profils, limites UTF-16, priorité des erreurs, mapping Jira, changement d'identité et oubli explicite. Les modèles, la file et les champs inconnus déjà stockés sont préservés ; leur remplacement depuis ce formulaire reste ignoré comme dans Node. Les profils et noms de champs historiques peuvent conserver des unités UTF-16 isolées. La recherche de profil réutilise maintenant la même construction des défauts complets.

Un corpus de 42 scénarios à attentes explicites est confronté à `lib/application/settings.mjs` par `rust-settings-reference.mjs`, puis au préparateur Rust. Il ne qualifie ni les chemins réels ni les URL : les deux résolveurs de ce corpus sont synthétiques. Le test Rust courant n'a pas besoin de Node. Un second test couvre les indicateurs publics et le retrait d'un jeton sans copier sa valeur dans la réponse. Ces nouveaux tests restent à exécuter sur GitHub ; aucun résultat local revendiqué. Branchement HTTP, coffre transactionnel, concurrence et véritable normalisation d'URL à réaliser dans l'incrément suivant.

Le [job natif `9a6721d`](https://github.com/FluryWaana/atelier/actions/runs/40106480434/job/120909135524) est réussi et son journal relu : test HTTP de déplacement/rollback des projets, 70 opérations de projet, 84 créations, 63 éditions, 46 transitions de temps et 305 + 51 contrats de domaine. La correction des chemins temporaires est donc vérifiée sur le runner Windows. La suite Node distante réussit également (434/430/0/4 ; exclusions détaillées dans M02). Edge reste en cours à cette collecte.

`test/fixtures/rust-domain-reference.mjs` appelle les vraies fonctions historiques sur 175 entrées et capture leurs résultats ou messages d'erreur dans le JSON voisin. `cargo xtask test-domain-parity` vérifie que cette référence correspond toujours à Node puis exécute `atelier-core/tests/domain-contracts.rs`. Les tests Rust courants lisent le JSON et restent indépendants de Node ; le comparateur historique est aussi exécuté sur les cinq runners CI.

Les vecteurs couvrent notamment champs absents/null, Unicode, bornes des critères, cycles, profils disparus, héritage carte/fournisseur, changement d'agent, authentification CLI/API, chaînes hostiles et priorité des validations. Le test de chaîne longue complète les vecteurs. Clippy natif et WASM passent ; 37 tests Rust réussis. Suite Node complète avec `ATELIER_REQUIRE_RELEASE=1` : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 84,51 s.

## Limites et suite

### Réglages de projets — base `05dbfc8`

`domain/projects.rs` conserve les champs, valeurs par défaut, limites UTF-16, mappings Jira, profils et conditions d'automatisation de `lib/project-settings.mjs`. Les dossiers sont contrôlés selon les traversées Windows sur tous les OS, comme la référence ; sorties du worktree, racines et préfixes de lecteurs sont refusés. La comparaison de dépôts conserve casse/séparateurs Windows, sans normaliser les chemins POSIX ni inventer une équivalence de slash final. Un port explicite exige `{port}` dans la commande.

Le comparateur couvre maintenant 246 résultats/messages Node. L'héritage conserve les champs inconnus et priorités projet/global. Aucun service ni agent n'est lancé ; les adaptateurs devront revalider chemins réels et liens à l'accès au disque. Formatage, Clippy natif/WASM et 37 tests Rust passent. Suite Node : 426 tests, 423 réussis, zéro échec, trois exclusions historiques, 83,19 s.

Ces preuves concernent les règles et leurs messages, pas encore les codes HTTP, transactions, effets distants ou courses de mutations. Le serveur prototype conserve son périmètre M01. Porter ensuite les cas d'usage de tâches/projets/réglages/temps et leurs routes, avec réservations et revalidation après I/O, puis comparer les parcours HTTP. Les gardes d'archivage/suppression, worktrees, modèles et invalidation des reviews ne sont pas validés par ces seuls tests purs.
