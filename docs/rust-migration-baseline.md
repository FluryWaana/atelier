# Référence M00 — 23 octobre 2026

Référence applicative : commit `843d3ea`, version `0.1.9`, `dataVersion = 2`, runtime Windows x64 `24.22.1`. Les ajouts M00 portent sur documentation, fixtures et outils de référence ; aucun comportement du produit n'a été modifié. État initial : seul `docs/rust-migration-plan.md` était non suivi. Aucun commit, tag, appel d'agent payant, installation ou publication effectué.

## Commandes et preuves

| Vérification exécutée | Résultat | Portée / exclusions |
| --- | --- | --- |
| `runtime/node.exe --version` et `runtime/node.exe scripts/check-release.mjs` | `v24.22.1`, release `0.1.9` vérifiée | Versions app/lock/runtime ; ne valide pas les trente fichiers d'une publication |
| `runtime/node.exe scripts/rust-migration-inventory.mjs` puis `--check` | 274 fichiers inventoriés, dont 93 fichiers de tests Node ; concordance vérifiée | Modules, ressources, scripts natifs et supports ; imports/repères lexicaux, pas analyse sémantique exhaustive |
| `runtime/node.exe --test test/rust-migration-reference.test.mjs` | 4 tests réussis | Fixture JSON/SQLite/WAL, rollback après erreur d'écriture, inventaire, faux CLI UTF-8 |
| `test/UpdateArchive.Tests.ps1` | 8 scénarios réussis | Archive valide, traversées, données interdites, noms Windows ambigus, doublons et absence d'extraction partielle |
| `runtime/node.exe scripts/package.mjs FluryWaana/atelier` | Archive locale Windows construite | Nouveau fichier sous `dist/`, aucun remplacement de distribution en cours ; aucun installateur ni signature de publication |
| `$env:ATELIER_REQUIRE_RELEASE='1'; runtime/node.exe --test --test-reporter=tap test/*.test.mjs` | **425 tests : 422 réussis, 0 échec, 3 ignorés**, 81,60 s | Paquet réel exigé ; installateurs désactivés |
| `runtime/node.exe scripts/rust-migration-baseline.mjs --browser 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'` | Toutes les assertions serveur/API/logs/navigateur ont réussi | Mesure finale après la suite, sans exécution concurrente des tests ; environnement et données ci-dessous |
| `git diff --check` | Aucune erreur d'espacement | Git signale seulement la conversion habituelle LF → CRLF du fichier existant |

Les trois tests ignorés sont l'installateur Windows (opt-in interdit sur le compte de travail), le tray Linux et le tray macOS. Les quatre scénarios `release-smoke` ont réellement tourné : mise à jour et rollback après démarrage défaillant, en portable et avec données utilisateur hors dossier applicatif. DPAPI et tray Windows sont inclus dans la suite réussie. Les tests POSIX sur doubles ne prouvent pas un bureau/trousseau POSIX réel.

Lors de l'examen préalable, la suite sans paquet présent avait donné 414 réussites et 7 exclusions. Une première tentative dans le bac à sable avait échoué sur `git init` temporaire (`Permission denied`) et des fetch locaux, puis stagné ; elle a été interrompue. La relance autorisée hors bac à sable et la référence finale ci-dessus ont réussi. Aucun défaut applicatif n'est déduit de cette tentative restreinte.

Les journaux de travail sont ignorés (`.artifacts/rust-migration-tests.tap`, `.artifacts/rust-migration-baseline.json`). Le [relevé JSON conservé](rust-migration-measurements.json) contient uniquement des mesures synthétiques et l'identité du paquet ; pas de token, PID, profil personnel ni base SQLite.

## Configuration et protocole de mesure

Windows `10.0.26300`, x64, Ryzen 9 7950X3D (32 processeurs logiques), 33 452 154 880 octets de RAM, Node `24.22.1`, Edge `155.0.4283.62` headless. Viewport effectif 1416 × 908, DPR 1 (fenêtre demandée 1440 × 1000). Ne pas comparer directement ces chiffres à un runner CI ou à un autre navigateur.

Jeu déterministe : 10 projets, 500 cartes réparties sur cinq statuts, 100 runs terminés, 200 temps draft, 10 000 lignes de journal. Même base synthétique réouverte entre cinq processus serveur successifs ; création initiale hors mesure. Aucun projet réel, aucun connecteur actif. Les appels externes sont désactivés ou remplacés par des refus.

Le temps serveur commence juste avant `fork` et finit après la première réponse authentifiée `/api/health`. Chaque processus réalise ensuite 20 lectures complètes de `/api/state` et une lecture de toutes les pages du journal (500 lignes par page + page vide finale), avec assertions des comptes et curseurs. RSS est relevé après 500 ms de repos suivant cette charge, sans forcer un GC ; il s'agit du serveur, pas de la mémoire totale navigateur + serveur.

« À froid » désigne ici le premier processus Node neuf / premier chargement dans un profil navigateur neuf ; les quatre passages suivants réchauffent les caches système. **Le cache disque OS n'est pas purgé et il n'y a pas de redémarrage du poste.** Le benchmark utilise `createApp` avec adaptateurs simulés : il ne chronomètre pas le lanceur, le tray ou le coffre réel. Le démarrage complet du paquet est vérifié fonctionnellement par `release-smoke`, mais son coût graphique de bout en bout reste à mesurer lors de M08/M11. Les durées serveur et navigateur sont des mesures séparées ; ne pas les présenter comme une mesure du clic Start jusqu'à l'interface.

Le tableau est déclaré prêt lorsque ses 500 cartes sont montées, le SSE connecté, puis deux frames rendues. Cinq navigations sur le même serveur, profil initialement neuf, sans cache HTTP (`no-store`). Le parcours live modifie une carte par l'API et attend le titre reçu via SSE dans le DOM. Enfin, l'ouverture de cette carte attend la dernière des 10 000 lignes et deux frames. Ce sont de vrais parcours navigateur, pas des doubles DOM ; cela ne remplace pas les parcours clavier/Safari/Firefox prévus en M07.

Le pilotage utilise uniquement les commandes du [protocole DevTools Page](https://chromedevtools.github.io/devtools-protocol/tot/Page/) et [Runtime](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/), sans dépendance npm. Cette instrumentation JavaScript de référence est transitoire ; le runner et les assertions UI courants seront portés en Rust avant M11.

## Résultats mesurés

| Mesure | Échantillons | Médiane | Min–max |
| --- | --- | --- | --- |
| Processus serveur → santé authentifiée | 5 | 133,70 ms | 131,74–155,95 ms |
| Lecture complète `/api/state` | 100 | 8,69 ms | 7,62–28,99 ms ; p95 10,99 ms |
| Lecture API des 10 000 logs | 5 | 24,61 ms | 23,84–27,57 ms |
| RSS serveur au repos après charge | 5 | 86,64 Mio | 86,09–91,15 Mio |
| Navigation → tableau utilisable + SSE | 5 | 308,00 ms | 280,10–453,30 ms |
| PATCH → titre actualisé dans le tableau via SSE | 1 | 297,80 ms | Observation ponctuelle |
| Ouverture carte → 10 000 logs rendus | 1 | 112,10 ms | Observation ponctuelle |

Premier démarrage serveur : 155,95 ms ; médiane des quatre suivants : 133,40 ms. Premier tableau : 453,30 ms ; médiane des quatre suivants : 306,45 ms. Les deux observations ponctuelles doivent être répétées avant une conclusion statistique.

### Taille du paquet Windows de référence

Archive locale : `atelier-kanban-0.1.9-windows-x64.zip`, **36 828 474 octets**. Somme des tailles décompressées de 175 fichiers : **97 251 878 octets**, dont runtime/licence Node **94 073 073** et documentation **1 677 596**. Ce n'est pas une mesure des blocs disque ni d'une installation Setup.

SHA256 : `624bb2d6f7413848a349ba85a425072eef6c462626dd1db7da82cd942e4311a0`.

Le paquet contient le code applicatif de `843d3ea` et la documentation/outillage M00 présents à sa fabrication ; les ajouts documentaires ultérieurs ne sont pas dans cet artefact. Les mesures détaillent la documentation pour distinguer cette variation de la taille du produit. Il est **non signé et réservé aux essais locaux** ; ne pas le publier comme release. Son empreinte, extraction, démarrage, conservation des données et rollback sont couverts ; la signature avec la clé réelle et la matrice des dix artefacts ne le sont pas.

## Budgets fixés avant comparaison Rust

Comparer sur le même poste, même jeu, mêmes parcours, même navigateur et après la suite, en conservant les valeurs brutes. Au-delà des seuils ci-dessous, investiguer et documenter une décision avant validation ; ne pas rendre la CI intermittente en y comparant directement ces durées locales.

- Temps serveur, API, logs et tableau : médiane au plus `max(référence × 1,20, référence + 5 ms)`. Répéter trois séries si dépassement ; conserver les séries, ne pas choisir la plus rapide. Suivre également le p95 de l'API et les maxima pour détecter des pauses cachées par la médiane.
- RSS serveur : médiane au plus +20 % (103,97 Mio), charge et repos identiques. Mesurer séparément l'empreinte totale navigateur + serveur en M07/M11 ; aucune économie WASM n'est présumée.
- Taille installée et archive : au plus +10 % à périmètre comparable, en isolant documentation/outillage et en conservant les licences requises. Les cinq cibles ont besoin de leurs propres références avant comparaison native.
- Parcours ponctuels SSE/logs : seuil exploratoire 1 s ; collecter au moins cinq observations par parcours lors de M07 avant validation des performances. Un résultat fonctionnel faux bloque indépendamment de la vitesse.
- Aucune tolérance de régression sur données, secrets, sécurité, erreurs, accessibilité ou effets distants. Aucun gain annoncé tant que Rust n'a pas été mesuré.

## Estimation de travail après inventaire

Ordres de grandeur de planification pour une personne connaissant le dépôt, en jours de travail effectif ; **non mesurés, non engagements**. Ils incluent portage, tests et revue. Disponibilité de machines natives, trousseaux interactifs, pilotes navigateur et incidents de packaging peuvent étendre le calendrier ; recalibrer après les prototypes M01. Les 274 fichiers incluent ressources et tests : ce nombre n'est pas utilisé comme une vitesse de traduction.

| Lot | Fourchette | Incertitude dominante |
| --- | --- | --- |
| M01 | 3–6 j | Toolchains, WASM/CSP, backends tray et cinq cibles |
| M02 | 5–10 j | Sérialisation, coffre natif, verrou et courses fichiers |
| M03 | 4–8 j | Contrats HTTP/defaults et réservations concurrentes |
| M04 | 8–15 j | Agents, protocoles interactifs, Git et descendants OS |
| M05 | 7–14 j | Parité connecteurs et callbacks JSON-RPC |
| M06 | 5–10 j | Effets incertains et reprises de workflows |
| M07 | 10–20 j | UI complète, brouillons, clavier, langues et navigateurs |
| M08 | 5–10 j | Trays natifs, launcher, import et absence de bureau |
| M09 | 7–15 j | Trois formats/versions, worker Windows, fautes durables |
| M10 | 5–10 j | Cinq cibles, installateurs, signatures et contrôles sécurité |
| M11 | 5–10 j | Qualification prolongée, écarts restants et retrait Node |

Total indicatif M01–M11 : **64–128 jours effectifs**, sans promesse de délai calendaire ni gain de performance. Les explorations indépendantes de M08 peuvent chevaucher d'autres lots ; elles ne retirent pas leurs dépendances de validation.

## Revue M00 et limites restantes

Réemploi de `openStore`, `createBackups` et `createApp` ; fixtures et faux CLI sous `test/fixtures`, outillage dans `scripts`. Aucun nouvel adaptateur produit, abstraction métier ou package npm. Les scripts possèdent des assertions de scénario ; les fixtures et l'inventaire ont quatre tests dédiés. Le script de mesure ne lit ni ne copie `data/`, n'utilise pas les profils navigateur personnels et n'enregistre pas les secrets de session transmis par IPC. Les profils temporaires sont supprimés ; contrôle final : aucun processus Edge portant le profil de benchmark ne restait actif.

Le runner, la fixture et l'inventaire sont propres à la référence Node et seront remplacés/retirés en M11 après preuve équivalente ; ne pas les transformer en infrastructure générale. Les sélecteurs du runner doivent évoluer explicitement si le contrat UI change.

M00 est validé comme **référence locale Windows avec exclusions documentées**. Restent à qualifier dans leurs lots : coffres/bureaux Linux et macOS, installateurs sur comptes dédiés, matrice native de release, Safari/Firefox, démarrage graphique complet et endurance. Aucune de ces preuves n'est considérée acquise. Prochain travail : **M01**, workspace et prototype vertical sur données temporaires ; aucune source Rust n'existe encore.
