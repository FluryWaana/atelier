# Mise à jour globale des dépendances

Le 30 octobre 2026, les versions stables des dépendances directes et outils sont vérifiées avec les API officielles de crates.io, Rust et GitHub. Les versions exactes restent épinglées dans les manifestes. Cargo résout aussi les mises à jour transitives compatibles et conserve le verrou complet.

| Dépendance ou outil | Avant | Après |
| --- | --- | --- |
| zip | 8.6.0 | 9.1.3 |
| rusqlite | 0.40.2 | 0.41.1 |
| base64 | 0.22.1 | 0.23.1 |
| html-escape | 0.2.15 | 0.3.0 |
| zbus direct | 5.19.0 | 6.2.0 |
| wasm-bindgen et CLI | 0.2.129 | 0.2.130 |
| wasm-bindgen-futures | 0.4.79 | 0.4.80 |
| js-sys et web-sys | 0.3.106 | 0.3.107 |
| Trunk | 0.22.0 | 0.22.1 |

Les autres dépendances directes sont déjà à leur dernière version stable disponible. Rust reste en 1.99.1. Leptos reste en 0.8.22 et libc en 0.2.191. Les préversions 0.9.0-rc0 et 1.0.0-alpha.5 ne sont pas sélectionnées.

## Adaptations

Le lecteur ZIP traite désormais explicitement les erreurs de décodage des noms. Il vérifie aussi la cohérence des en-têtes locaux et centraux avant toute extraction, avec `validate_header_consistency`. Une régression écrite modifie seulement le nom local d'une entrée dont le nom central reste autorisé et exige un refus avant toute écriture. Les protections existantes sur chemins, liens, compression, tailles, doublons et signatures restent appliquées.

Le [guide officiel zbus 6](https://z-galaxy.github.io/zbus/upgrading-to-6.html) annonce la suppression de l'API bloquante. Les sondes de tray et de coffre utilisent l'API asynchrone, `Variant` et `OwnedVariant`, avec les erreurs de builders vérifiées à `build`. Les appels asynchrones utilisent Tokio. Le point d'entrée synchrone du faux tray garde un unique `zbus::block_on`. Les gardes CI et session jetable des tests restent présentes. Aucun coffre n'a été ouvert.

SQLite embarqué avec libsqlite3-sys 0.39.1 présente des gardes C incohérentes : FILESTAT enregistre `walstatFunc`, mais sa définition exige WALSTAT. Le build Windows reproduit l'erreur C2065. `.cargo/config.toml` active donc aussi `SQLITE_ENABLE_WALSTAT`, avec un commentaire de retrait lorsque SQLite corrige ces gardes. FILESTAT reste activé pour le contrôle d'identité des fichiers ouverts. Ni l'identité ATKR, ni la version de format applicatif ne changent. Aucune base utilisateur n'est ouverte ou modifiée.

Les épingles sont alignées dans Trunk.toml, xtask, le workflow manuel et les documents de fabrication actifs. Les preuves historiques de l'audit et des anciennes campagnes conservent leurs versions d'origine. Les outils Windows officiels sont vérifiés contre les SHA256 des assets GitHub, puis installés dans le PATH utilisateur. Les anciens exécutables restent dans `.artifacts/dependency-tools-2026-10-30/previous`. Ils n'étaient pas gérés par Cargo.

## Vérifications

- `cargo fmt --all -- --check` réussit.
- `cargo check --workspace --exclude atelier-ui --all-targets --locked --offline` réussit sous Windows, cibles de tests comprises.
- `cargo check -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked --offline` réussit.
- `cargo xtask build-ui` réussit avec les nouveaux outils. Trunk fabrique les assets release et xtask vérifie le chargeur CSP et les limites de taille.
- Les trois sections D-Bus modifiées passent aussi un contrôle de types Windows dans un workspace synthétique isolé sous `.artifacts/zbus6-typecheck`, sans exécution. Les copies n'activent que le code Linux à vérifier et remplacent le chemin du binaire de test par une chaîne synthétique. Cela vérifie les API zbus 6, sans démontrer le fonctionnement sur Linux.

Les contrôles portent sur le dossier de travail, qui contient d'autres modifications locales préexistantes ou en cours. Les journaux `.artifacts/dependency-update-*.log` sont locaux et non publiés. Le premier contrôle natif a rencontré un refus d'accès du linker MSVC dans le bac à sable. La compilation autorisée hors du bac à sable a ensuite révélé le défaut de gardes SQLite décrit ci-dessus. Le contrôle final passe avec la configuration corrigée.

Aucun test, Clippy, parcours navigateur, scénario natif ou workflow CI n'est exécuté. Aucune qualification Linux ou macOS, installation de paquet ou mise à jour de données n'est revendiquée.

## Avis de sécurité restants

Les 422 paquets de registre du verrou sont interrogés par nom et version contre l'[API OSV](https://google.github.io/osv.dev/api/). Aucun code ni contenu utilisateur n'est transmis. Les cinq avis ZIP de Dependabot ne correspondent plus à la version retenue. Les deux alertes critiques initiales étaient deux occurrences du même [avis](https://github.com/zip-rs/zip2/security/advisories/GHSA-m665-v7pm-7grj), dans Cargo.lock et le manifeste.

La réponse OSV contient encore les avis suivants. Ils ne sont ni ignorés par configuration ni déclarés corrigés.

| Avis | Dépendance et portée | Suite nécessaire |
| --- | --- | --- |
| [GHSA-qwgh-2vcv-g2f7](https://github.com/advisories/GHSA-qwgh-2vcv-g2f7) | OSV associe block-buffer 0.10.4 à un avis modéré de corruption après panic rattrapée. Le chemin est Leptos, wasm_split_helpers, wasm_split_macros, sha2 0.10.9, digest 0.10.7. Il s'agit d'une dépendance de compilation. Le produit utilise directement sha2 0.11.1 et block-buffer 0.12.1. | Suivre la mise à jour amont de la chaîne de macros. Aucun scénario d'exploitation n'est exécuté et aucune absence de risque n'est certifiée. Ne pas forcer une dépendance transitive incompatible. |
| [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | paste 1.0.15 est déclaré non maintenu. Il reste utilisé par Leptos et ses composants stables. | Suivre leur migration amont. Ce signal de maintenance ne démontre pas une vulnérabilité exploitable. |
| [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html), [0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html), [0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html) | zvariant, zbus_names et zvariant_derive sont fusionnés dans zbus 6 et ne sont plus maintenus séparément. Ils restent transitifs via tray-icon 0.26.1, ksni 0.3.7 et zbus 5.19.0 sous Linux. Nos dépendances directes utilisent zbus 6.2.0. | Suivre la migration de ksni et tray-icon. Leur dernière version stable conserve ce graphe. Aucune substitution forcée ni fork local n'est ajouté. |

Ce relevé est une vérification des avis publiés au moment de l'interrogation, pas une certification de sécurité de toutes les dépendances.
