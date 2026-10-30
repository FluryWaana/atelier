# Atelier Kanban

Atelier Kanban est une application locale pour préparer des tâches, travailler avec des agents dans des worktrees Git, examiner les changements et suivre le temps. Son interface Rust utilise Leptos/WASM ; le serveur natif conserve les données en SQLite et les identifiants dans le stockage protégé du système.

**L’application est en préproduction et sa qualification reste en cours. Aucun paquet qualifié ni release disponible n’est annoncé.** Les scénarios écrits, le formatage et les anciens résultats ne constituent pas une preuve de réussite des changements actuels. Le suivi figure dans la [checklist des fonctionnalités](docs/features.md).

## Sources et prérequis

Le style de l’application reprend directement la feuille CSS de `7f48996:public/style.css`, comprenant Bootstrap 5.3.8 sous licence MIT. Ce CSS est une source intégrée au projet Rust ; les adaptations aux composants sont écrites dans cette même feuille. Aucun Sass, npm, JavaScript Bootstrap ou runtime Node n’est nécessaire à sa fabrication. La licence complète reste dans le CSS et dans les avis des paquets Rust.

Le workspace contient `atelier-core` (domaine), `atelier-app` (serveur et intégrations), `atelier-ui` (interface) et `xtask` (fabrication et qualification). Utiliser [rust-toolchain.toml](rust-toolchain.toml) et [Cargo.lock](Cargo.lock). La fabrication UI utilise Trunk 0.22.1 et wasm-bindgen CLI 0.2.130 ; les dépendances natives et outils d’installation sont décrits dans [les paquets Rust](docs/rust-packages.md).

Le workspace Rust à la racine contient les sources de l’application et ses outils de fabrication. Les fixtures de régression sont conservées dans les dossiers `tests/fixtures/` des crates concernées.

## Commandes natives

Sous Windows, double-cliquer sur [Atelier.cmd](Atelier.cmd) prépare puis ouvre l’aperçu Rust avec rechargement automatique. Les lancements suivants réutilisent la dernière compilation réussie si les sources sont inchangées. La première compilation peut être longue ; Rust installé sur le PC, Trunk et wasm-bindgen doivent être disponibles dans le PATH. Le lanceur ne remplace pas les installations Cargo/Rustup de l'utilisateur par celles du dépôt. L’interface utilise `cargo xtask build-ui --dev`, avec compilation incrémentale, optimisation légère et sans informations de débogage WASM, dans `.artifacts/rust-ui-dev`. Les assets release restent séparés. Ce raccourci ne lance ni tests ni Clippy et ne constitue pas une qualification du produit.

Garder la console ouverte. Après sauvegarde d'une source Rust, UI, traduction, CSS ou configuration Cargo, le script attend la fin des modifications, arrête l'instance de développement, reconstruit puis relance le vrai serveur `serve --dev` sur `http://127.0.0.1:4328`. L'onglet s'actualise automatiquement ; après une saisie dans un formulaire, il demande confirmation pour éviter de perdre le brouillon. Annuler conserve l'onglet : actualiser manuellement après avoir sauvegardé ses saisies. `Ctrl+C` demande l'arrêt authentifié du serveur. Les données synthétiques sont conservées dans `.artifacts/local-dev/data` ; `data/` n'est jamais surveillé ni utilisé. Les anciennes données de Preview restent dans `.artifacts/local-preview/data` sans être modifiées.

Ce mode recharge la page entière et réinitialise l'état en mémoire de l'interface. Une modification limitée à l'interface ne recompile pas le serveur ; une modification limitée au serveur ne reconstruit pas l'interface. Les changements partagés reconstruisent les deux. Le serveur reste disponible pendant la compilation de l'interface, puis redémarre pour charger les nouveaux assets ; il est indisponible pendant la compilation du serveur. Une erreur de compilation est affichée dans la console ; une nouvelle sauvegarde relance la tentative. Les dépendances sont récupérées lorsque les manifestes, le verrou ou la configuration Cargo changent. Le mode `--dev`, réservé au binaire hors installation, conserve la session locale existante entre redémarrages ; le lancement normal renouvelle toujours sa session et n'injecte aucun script de rechargement. Le contrat Linux/macOS et les parcours Windows sont décrits dans [le suivi du lanceur](docs/development-launcher.md).

La compilation, les tests locaux, Clippy et les parcours navigateur synthétiques utilisent les ressources normales du poste. Aucun workflow CI ne doit être lancé ni relancé.

Après fabrication et qualification dans un environnement autorisé, le binaire `atelier` fournit notamment :

```text
atelier --help
atelier --version
atelier launch --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets BUILT_RUST_UI --port 4317 --no-browser
atelier stop --data-dir EMPTY_SYNTHETIC_DIRECTORY --port 4317
atelier serve --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets BUILT_RUST_UI --port 4317
```

Remplacer les chemins d’exemple par des dossiers synthétiques explicites ; `BUILT_RUST_UI` désigne les assets Leptos/WASM fabriqués. `launch` démarre ou retrouve une instance locale ; `serve` garde le serveur au premier plan. Utiliser le même dossier et le même port pour l’arrêter. Voir [le lanceur](docs/rust-launcher.md).

L’installation signée dans un **nouveau** dossier dispose de la commande suivante ; elle exige une archive, ses métadonnées signées et une clé publique de confiance choisie séparément :

```text
atelier install --archive ARCHIVE.zip --signature SIGNED_METADATA.json --trust SYNTHETIC_PUBLIC_KEY.json --destination NEW_INSTALLATION --mode portable
atelier verify-installation
```

Ce sont des commandes candidates, pas des téléchargements disponibles. La fabrication, les formats Windows/Linux/macOS, les mises à jour et leur récupération sont décrits dans [les paquets Rust](docs/rust-packages.md). Aucun tag, release ou déploiement n’est autorisé par les vérifications locales.

## Développement et qualification

Suivre [AGENTS.md](AGENTS.md). La compilation, les tests locaux, Clippy et les parcours navigateur synthétiques sont autorisés avec les ressources normales du poste. L'accord du 30 octobre lève les anciennes restrictions d'exécution locale. Tout lancement ou relance de CI reste interdit. Le workflow reste manuel et son existence ne vaut pas autorisation. Une compilation ne vaut pas preuve de parcours réussi. Utiliser exclusivement des données, dépôts, identifiants et agents synthétiques. Ne pas ouvrir le `data/` utilisateur, un compte réel ou un service payant.

Dans une console PowerShell à la racine du dépôt, un dossier Cargo séparé permet de vérifier l'application pendant que l'aperçu reste ouvert :

```powershell
$env:CARGO_TARGET_DIR = Join-Path $PWD 'target/local-checks'
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo xtask build-ui
```

Les parcours `cargo xtask test-ui --prepared-assets` utilisent des données et des agents synthétiques. Ils nécessitent un WebDriver compatible avec le navigateur choisi et un environnement de test sans accès aux profils ou identifiants personnels.

La [checklist des fonctionnalités](docs/features.md) suit les capacités de l’application et leur validation. Les [connecteurs](docs/rust-connectors.md) documentent leurs contrats et limites. Les contrôles non exécutés restent signalés comme tels.

Licence : [AGPL-3.0-or-later](LICENSE).
