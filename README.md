# Atelier Kanban — reconstruction Rust de préproduction

Atelier Kanban est une application locale pour préparer des tâches, travailler avec des agents dans des worktrees Git, examiner les changements et suivre le temps. Son interface Rust utilise Leptos/WASM ; le serveur natif conserve les données en SQLite et les identifiants dans le stockage protégé du système.

**Cette branche contient des candidats d’implémentation en cours de qualification. La reconstruction n’est pas déclarée terminée. Aucun paquet Rust qualifié ni release Rust disponible n’est annoncé.** Les scénarios écrits, le formatage et les anciens résultats ne constituent pas une preuve de réussite des changements actuels. Le suivi exact figure dans le [plan actif](docs/rust-migration-plan.md) et la [checklist des fonctionnalités](docs/rust-rebuild-features.md).

## Sources et prérequis

Le style de l’application reprend directement la feuille CSS de `7f48996:public/style.css`, comprenant Bootstrap 5.3.8 sous licence MIT. Ce CSS est une source intégrée au le projet Rust ; les adaptations aux composants sont écrites dans cette même feuille. Aucun Sass, npm, JavaScript Bootstrap ou runtime Node n’est nécessaire à sa fabrication. La licence complète reste dans le CSS et dans les avis des paquets Rust.

Le workspace contient `atelier-core` (domaine), `atelier-app` (serveur et intégrations), `atelier-ui` (interface) et `xtask` (fabrication et qualification). Utiliser [rust-toolchain.toml](rust-toolchain.toml) et [Cargo.lock](Cargo.lock). La fabrication UI utilise Trunk 0.22.1 et wasm-bindgen CLI 0.2.130 ; les dépendances natives et outils d’installation sont décrits dans [les paquets Rust](docs/rust-packages.md).

Le projet actif est le workspace Rust à la racine. Les sources, lanceurs, outils de livraison, tests et archives de code Node ont été retirés ; leur référence complète reste accessible dans l’historique Git au snapshot `7f48996b9626ccff79ecf0e1be7f8fa43b7b90e6`. Le snapshot historique de migration est également conservé au commit `7326122`. Aucun runtime Node ne fait partie du produit ou des paquets Rust.

Les dix fixtures JSON encore utiles aux tests Rust sont conservées, sans changement de contenu, dans `crates/atelier-core/tests/fixtures/` et `crates/atelier-app/tests/fixtures/`. Les comparateurs exécutant Node et l’ancienne interface ne sont plus des commandes de `xtask`. Les fichiers locaux non suivis et les données utilisateur n’ont pas été nettoyés par ce retrait de sources.

## Commandes natives

Sous Windows, double-cliquer sur [Atelier.cmd](Atelier.cmd) prépare puis ouvre l’aperçu Rust avec rechargement automatique. Les lancements suivants réutilisent la dernière compilation réussie si les sources sont inchangées. La première compilation peut être longue ; Rust installé sur le PC, Trunk et wasm-bindgen doivent être disponibles dans le PATH. Le lanceur ne remplace pas les installations Cargo/Rustup de l'utilisateur par celles du dépôt. L’interface utilise `cargo xtask build-ui --dev`, avec compilation incrémentale, optimisation légère et sans informations de débogage WASM, dans `.artifacts/rust-ui-dev`. Les assets release restent séparés. Ce raccourci ne lance ni tests ni Clippy et ne constitue pas une qualification du produit.

Garder la console ouverte. Après sauvegarde d'une source Rust, UI, traduction, CSS ou configuration Cargo, le script attend la fin des modifications, arrête l'instance de développement, reconstruit puis relance le vrai serveur `serve --dev` sur `http://127.0.0.1:4328`. L'onglet s'actualise automatiquement ; après une saisie dans un formulaire, il demande confirmation pour éviter de perdre le brouillon. Annuler conserve l'onglet : actualiser manuellement après avoir sauvegardé ses saisies. `Ctrl+C` demande l'arrêt authentifié du serveur. Les données synthétiques sont conservées dans `.artifacts/local-dev/data` ; `data/` n'est jamais surveillé ni utilisé. Les anciennes données de Preview restent dans `.artifacts/local-preview/data` sans être modifiées.

Ce mode recharge la page entière et réinitialise l'état en mémoire de l'interface. Une modification limitée à l'interface ne recompile pas le serveur ; une modification limitée au serveur ne reconstruit pas l'interface. Les changements partagés reconstruisent les deux. Le serveur reste disponible pendant la compilation de l'interface, puis redémarre pour charger les nouveaux assets ; il est indisponible pendant la compilation du serveur. Une erreur de compilation est affichée dans la console ; une nouvelle sauvegarde relance la tentative. Les dépendances sont récupérées lorsque les manifestes, le verrou ou la configuration Cargo changent. Le mode `--dev`, réservé au binaire hors installation, conserve la session locale existante entre redémarrages ; le lancement normal renouvelle toujours sa session et n'injecte aucun script de rechargement. Le contrat Linux/macOS et les parcours Windows sont décrits dans [le suivi du lanceur](docs/development-launcher.md).

Les compilations locales utilisent les ressources normales du poste et le parallélisme Cargo disponible. Les tests locaux Windows du lanceur ont été autorisés pour la campagne des 28–29 octobre ; les autres vérifications restent soumises aux consignes du dépôt. Aucun workflow CI ne doit être lancé ni relancé.

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

Ce sont des commandes candidates, pas des téléchargements disponibles. La fabrication, les formats Windows/Linux/macOS, les mises à jour et leur récupération sont décrits dans [les paquets Rust](docs/rust-packages.md). Aucun tag, release ou déploiement n’est autorisé par cette reconstruction.

## Développement et qualification

Suivre [AGENTS.md](AGENTS.md). Depuis l'accord explicite du 27 octobre, les builds locaux peuvent utiliser les ressources normales du poste. Les tests locaux, Clippy et scénarios navigateur restent interdits sans autorisation distincte. Tout lancement ou relance de CI est interdit depuis le 27 octobre. Le workflow reste manuel et son existence ne vaut pas autorisation. Une compilation ne vaut pas preuve de parcours réussi. Utiliser exclusivement des données, dépôts, identifiants et agents synthétiques ; ne pas ouvrir le `data/` utilisateur, un compte réel ou un service payant.

L’[inventaire des sources](docs/rust-migration-inventory.md) relie les capacités de référence au travail restant. Les [connecteurs Rust](docs/rust-connectors.md) documentent leurs contrats et limites. Les preuves et lacunes de qualification restent suivies dans le plan actif, sans présenter un scénario non exécuté comme réussi.

Licence : [AGPL-3.0-or-later](LICENSE).
