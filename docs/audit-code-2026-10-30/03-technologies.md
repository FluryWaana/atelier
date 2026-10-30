# Technologies utilisées dans Atelier Kanban

Atelier Kanban associe un serveur natif Rust, une interface Rust compilée pour le navigateur et une base SQLite locale. Il n'utilise pas de runtime Node pour le produit actif. Les mêmes contrats Rust servent au domaine, au serveur et à l'interface.

Les versions ci-dessous sont celles déclarées dans les manifestes audités. Ce ne sont pas des affirmations sur les dernières versions disponibles. Les raisons indiquées décrivent le rôle observé et l'intérêt technique du choix. Sauf mention d'une décision du plan, elles ne prétendent pas restituer la motivation historique de leur auteur.

## Architecture et langages

| Technologie | Utilisation | Pourquoi ce choix est cohérent | Limite ou coût |
| --- | --- | --- | --- |
| Rust édition 2024 | Langage des quatre crates | Partage des modèles et contrôle des ressources par le compilateur | Les frontières FFI et les règles métier restent à vérifier |
| Rust 1.99.1 | Toolchain déclarée dans `rust-toolchain.toml` | Version de travail reproductible avec rustfmt, Clippy et cible WASM | Les paquets déclarent une version minimale 1.99. Le fichier de toolchain fixe la version précise |
| Cargo et Cargo.lock | Dépendances, compilation et workspace | Graphe verrouillé et outillage commun | Le verrou ne remplace pas le suivi des avis de sécurité |
| WebAssembly | Binaire de l'interface exécuté dans le navigateur | Réutilisation des contrats et du langage Rust | Nécessite les adaptateurs JavaScript du navigateur et des assets compilés |
| HTML et CSS | Document hôte et présentation | Technologies natives du navigateur | La qualité visuelle et l'accessibilité demandent des parcours réels |
| JavaScript limité | Chargeur WASM généré et rechargement de développement | Démarrage du module et usage des API du navigateur | L'absence de runtime Node ne signifie pas absence de JavaScript côté navigateur |
| PowerShell, shell POSIX et CMD | Aperçu Windows, qualification et packaging | Intégration avec les outils du poste et de CI | Les scripts natifs doivent être qualifiés sur leur système |
| JSON | HTTP, contrats d'agents, extensions, configuration et documents stockés | Format partagé et sérialisable | Les zones métier en JSON ouvert perdent des garanties de type |
| TOML et YAML | Manifestes, configuration Trunk et workflow | Formats des outils concernés | La cohérence entre configuration et documentation reste à maintenir |

Sources : [Cargo.toml](../../Cargo.toml), [toolchain](../../rust-toolchain.toml), [README](../../README.md), [plan actif](../rust-migration-plan.md).

| Crate interne | Responsabilité et raison du découpage |
| --- | --- |
| `atelier-core` | Domaine, validation, cas d'usage et contrats sans adaptateurs I/O. Permet de réutiliser le métier et de le vérifier séparément |
| `atelier-app` | Composition, HTTP, stockage, fichiers, Git, processus et intégrations. Réunit les effets natifs derrière les contrats |
| `atelier-ui` | Interface Leptos CSR. CSR signifie que l'interface est construite dans le navigateur |
| `xtask` | Fabrication des assets, qualification et paquets. Centralise les commandes de projet dans le même langage |

## Bibliothèques directes du serveur et du domaine

| Bibliothèque et version déclarée | Rôle et raison d'utilisation |
| --- | --- |
| `serde 1.0.229` | Sérialisation et désérialisation des structs. Évite un encodage manuel des contrats |
| `serde_json 1.0.154` | JSON et `RawValue`. Permet les protocoles et les documents ouverts, avec le coût de typage décrit dans l'audit |
| `tokio 1.54.1` | Runtime asynchrone, réseau, processus, canaux, délais et synchronisation. Permet de coordonner plusieurs opérations I/O |
| `axum 0.8.9` | Routeur HTTP, extracteurs, middleware et réponses. Structure les endpoints et leurs gardes |
| `hyper 1.12.0` | Primitives HTTP utilisées par la pile et certains chemins de transport. Complète Axum au niveau protocole |
| `tower-http 0.7.1` | Dépendance déclarée avec la fonctionnalité fichiers activée. Aucun usage direct trouvé dans les sources, tests ou configurations parcourus. Son besoin actuel n'est pas établi, voir constat 6 |
| `futures-util 0.3.34` | Composition de flux asynchrones, notamment les notifications SSE |
| `reqwest 0.13.5` | Client HTTP avec JSON et backend Rustls. Évite une implémentation HTTP/TLS spécifique par intégration |
| `rusqlite 0.40.2` | Accès SQLite synchrone, SQLite embarqué et sauvegarde. Le worker dédié l'intègre au serveur asynchrone |
| `tempfile 3.27.0` | Dossiers et fichiers temporaires de staging et de test. Gère leur durée de vie avec des ressources possédées |
| `cap-std 4.0.3` | Accès aux fichiers à partir de dossiers possédés. Réduit les opérations dépendant d'un chemin global |
| `cap-fs-ext 4.0.3` | Opérations complémentaires sur ces dossiers, dont des ouvertures sans suivi de lien |
| `getrandom 0.4.3` | Aléatoire système pour les sessions et autres secrets aléatoires |
| `hmac 0.13.0` | Preuve d'authenticité de la réponse de découverte locale |
| `sha2 0.11.1` | Empreintes SHA256 des fichiers et composants du protocole de preuve |
| `subtle 2.6.1` | Comparaison en temps constant du secret de session |
| `ring 0.17.14` | Chiffrement authentifié AES-GCM et signatures Ed25519. Utilise des primitives de bibliothèque au lieu de les réimplémenter |
| `base64 0.22.1` | Encodage de données binaires, d'enveloppes et d'en-têtes. Base64 n'est pas du chiffrement |
| `uuid 1.29.0` | Identifiants aléatoires v4 pour opérations et ressources |
| `time 0.3.55` | Parsing et formatage des dates et heures, macros de formats |
| `regex 1.13.1` | Reconnaissance de motifs textuels dans les adaptateurs. Ne remplace pas les validations structurées |
| `html-escape 0.2.15` | Traitement des entités HTML dans les contenus traités par le serveur. Ne constitue pas une garantie globale contre les XSS |
| `icu_collator 2.4.0` et `icu_locale_core 2.4.0` | Comparaison de textes avec des règles de locale et représentation des locales |
| `zip 8.6.0` | Archives des paquets Rust. Les chemins, tailles et contenus doivent aussi passer les contrôles applicatifs |

Sources : [manifeste workspace](../../Cargo.toml), [manifeste serveur](../../crates/atelier-app/Cargo.toml), [manifeste métier](../../crates/atelier-core/Cargo.toml). Les dépendances partagées par `xtask` ne sont pas répétées dans ce tableau.

## Interface et présentation

| Technologie | Rôle et raison d'utilisation |
| --- | --- |
| `leptos 0.8.22`, fonctionnalité `csr` | Composants, signaux et état réactif en Rust. Cohérent avec une UI sans moteur serveur de rendu HTML |
| `gloo-net 0.7.0` | Requêtes HTTP de l'interface vers l'API locale |
| `wasm-bindgen 0.2.129` | Liaisons entre Rust/WASM et JavaScript. Le CLI correspondant doit être aligné |
| `wasm-bindgen-futures 0.4.79` | Exécution des futures Rust côté navigateur |
| `js-sys 0.3.106` | Accès aux objets JavaScript, par exemple les dates |
| `web-sys 0.3.106` | API du navigateur typées, dont stockage, historique, événements, dialogues et `EventSource` |
| Trunk 0.22.0 | Construction des assets HTML, CSS et WASM. Appelé par `cargo xtask build-ui` |
| CSS intégrant Bootstrap 5.3.8 | Base de styles conservée dans `style.css`, puis adaptée directement. Aucun Sass ou JavaScript Bootstrap n'est requis par ce pipeline |
| Catalogues `fr-FR.json` et `en-US.json` | Textes localisés avec clés et replis dans `i18n.rs`. Évite de limiter le produit au français ou à l'anglais seul |
| SSE sur HTTP local | Notifications du serveur vers les onglets sans interrogation périodique complète. L'UI regroupe les rechargements demandés |

Sources : [manifeste UI](../../crates/atelier-ui/Cargo.toml), [Trunk.toml](../../crates/atelier-ui/Trunk.toml), [index.html](../../crates/atelier-ui/index.html), [styles](../../crates/atelier-ui/style.css), [live.rs](../../crates/atelier-ui/src/live.rs), [i18n.rs](../../crates/atelier-ui/src/i18n.rs), [fabrication UI](../../xtask/src/main.rs).

## Stockage et sécurité native

SQLite stocke notamment cartes, projets, exécutions, temps, logs et réglages. Plusieurs tables contiennent un identifiant et un document JSON. WAL sépare l'écriture courante du fichier principal pour le fonctionnement SQLite. Les sauvegardes doivent tenir compte de cet état, ce que le code prévoit par ses opérations SQLite dédiées. Un fichier SQLite contient aussi le verrou d'exclusion du dossier de données.

L'identité `ATKR` et la version de format 1 permettent de refuser un fichier étranger au produit. Elles ne chiffrent pas la base. Le chiffrement des jetons relève d'un adaptateur séparé et la protection de l'ensemble des données dépend aussi des permissions du compte et du système.

| Technologie native | Utilisation et justification |
| --- | --- |
| `windows-sys 0.61.2` | API Windows pour DPAPI, handles, fichiers, processus, jobs, réseau et interfaces natives. Fournit les liaisons FFI sans runtime Node |
| DPAPI | Protection Windows liée au contexte du compte. Évite de stocker le jeton sensible en clair dans le fichier protégé par cette voie |
| `libc 0.2.191` | API POSIX pour descripteurs, groupes de processus et opérations système indisponibles directement dans les abstractions utilisées |
| Secret Service et `secret-tool` sous Linux | Accès au coffre de la session. Nécessite l'environnement D-Bus et le service de secrets appropriés |
| Keychain et outil `security` sous macOS | Accès au coffre macOS par adaptateur natif. Les probes doivent utiliser des keychains jetables |
| `tray-icon 0.26.1` | Icône et menu de bureau. Fonctionnalité `ksni` activée, avec dépendances natives selon la plateforme |
| `objc2 0.6.5` | Liaisons Objective-C pour les appels macOS |
| `objc2-app-kit 0.3.2` et `objc2-foundation 0.3.2` | Boucle d'application et objets natifs macOS pour l'intégration bureau |
| `block2 0.6.2` | Blocs Objective-C dans l'outillage macOS de qualification, pas une dépendance générale du noyau métier |

Sources : manifestes [app](../../crates/atelier-app/Cargo.toml) et [xtask](../../xtask/Cargo.toml), [token_vault.rs](../../crates/atelier-app/src/infrastructure/token_vault.rs), [session.rs](../../crates/atelier-app/src/infrastructure/session.rs), [paquets](../rust-packages.md).

## Outils de développement et de qualification

| Outil ou bibliothèque | Rôle et raison |
| --- | --- |
| rustfmt | Formatage Rust commun. Réduit les divergences de présentation |
| Clippy | Analyse statique de code Rust. Complète la lecture sans prouver à lui seul la correction |
| Tests Rust et faux agents compilés | Scénarios métier et natifs sans compte réel ni appel payant |
| `fantoccini 0.22.1` | Client WebDriver de `xtask` pour les parcours navigateur |
| EdgeDriver, ChromeDriver, GeckoDriver et SafariDriver | Pilotage des navigateurs prévus par les scénarios. La présence d'une sélection ne prouve pas leur qualification |
| `tokio-rustls 0.26.6` | Serveurs TLS synthétiques dans les dépendances de test |
| `hyper-util 0.1.21` | Adaptateurs HTTP/Tokio nécessaires aux doubles de test |
| `zbus 5.19.0` | Accès D-Bus dans les dépendances de test Linux, notamment l'environnement de coffre |
| Git et worktrees | Isolation des branches de travail sans clone complet de chaque dépôt. Git reste un exécutable natif externe |
| GitHub Actions | Workflow Rust manuel, matrices de systèmes et conservation de preuves. Son lancement est actuellement interdit par les consignes du dépôt |
| Inno Setup 6 | Fabrication de l'installeur Windows candidat |
| `dpkg-deb` et `rpmbuild` | Fabrication des paquets Linux DEB et RPM candidats |
| Docker | Qualification des gestionnaires de paquets Linux dans des conteneurs jetables prévue par le workflow |
| Xvfb, D-Bus et environnement de bureau jetable | Préparation des tests Linux du tray et des coffres sans agir sur une session personnelle |

Sources : [xtask/Cargo.toml](../../xtask/Cargo.toml), [workflow Rust](../../.github/workflows/rust.yml), [scripts](../../scripts), [packaging](../../xtask/packaging). Les outils listés sont utilisés ou prévus par le code et les recettes. Leur installation et leur fonctionnement n'ont pas été contrôlés pendant l'audit.

## Intégrations externes

| Intégration | Pourquoi elle existe dans le produit | Point de vigilance |
| --- | --- | --- |
| Jira Cloud et Data Center | Rechercher, importer et synchroniser des tickets et leurs transitions | Authentification, origine configurée, pagination et conflits avec les éditions locales |
| Tempo | Exporter et rapprocher le temps associé aux tickets | Un effet peut avoir eu lieu malgré la perte de réponse, d'où un état incertain sans rejeu aveugle |
| GitHub, GitLab et Azure | Dépôts, PR ou MR et publication selon les adaptateurs | Vérifier l'association dépôt, les droits et le statut des opérations distantes |
| Azure Boards | Adaptateur de tickets disponible comme capacité optionnelle | Ne pas confondre présence du code et qualification fonctionnelle |
| Agents Codex, Claude et autres fournisseurs du catalogue | Exécuter des tâches et échanger via les protocoles des CLI ou ACP selon le fournisseur | Catalogue, installation, connexion locale et accès réel à un modèle sont des informations différentes |
| Extensions `atelier-connector/1` | Ajouter des capacités tickets, temps ou dépôts via des processus natifs | Protocole et empreinte ne remplacent pas la confiance dans le programme exécuté |
| Releases GitHub | Recherche et téléchargement de mises à jour dans le candidat présent | Domaine et taille contrôlés, puis vérification du paquet. Aucune release réelle n'a été consultée ici |

Sources : [jira.rs](../../crates/atelier-app/src/infrastructure/jira.rs), [tempo](../../crates/atelier-app/src/infrastructure/tempo.rs), [forge.rs](../../crates/atelier-app/src/infrastructure/forge.rs), [catalogue agents](../../crates/atelier-core/src/domain/agent_catalog.rs), [extensions](../../crates/atelier-core/src/application/extensions.rs), [mises à jour GitHub](../../crates/atelier-app/src/infrastructure/updates/github.rs).

## Ce que couvre cet inventaire technologique

Les bibliothèques externes déclarées directement dans les quatre manifestes sont regroupées ci-dessus, y compris les dépendances spécifiques aux plateformes et aux tests. Les bibliothèques transitives, comme les composants internes de Rustls, sont consignées dans [Cargo.lock](../../Cargo.lock) et ne sont pas chacune un choix applicatif indépendant. Ce document n'est pas un inventaire complet de licences ou une attestation de sécurité de toute la chaîne de dépendances. Le projet possède aussi un générateur d'avis tiers dans `xtask`.
