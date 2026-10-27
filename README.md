# Atelier Kanban — reconstruction Rust de préproduction

Atelier Kanban est une application locale pour préparer des tâches, travailler avec des agents dans des worktrees Git, examiner les changements et suivre le temps. Son interface Rust utilise Leptos/WASM ; le serveur natif conserve les données en SQLite et les identifiants dans le stockage protégé du système.

**Cette branche contient des candidats d’implémentation en cours de qualification. La reconstruction n’est pas déclarée terminée. Aucun paquet Rust qualifié ni release Rust disponible n’est annoncé.** Les scénarios écrits, le formatage et les anciens résultats ne constituent pas une preuve de réussite des changements actuels. Le suivi exact figure dans le [plan actif](docs/rust-migration-plan.md) et la [checklist des fonctionnalités](docs/rust-rebuild-features.md).

## Sources et prérequis

Le workspace contient `atelier-core` (domaine), `atelier-app` (serveur et intégrations), `atelier-ui` (interface) et `xtask` (fabrication et qualification). Utiliser [rust-toolchain.toml](rust-toolchain.toml) et [Cargo.lock](Cargo.lock). La fabrication UI utilise Trunk 0.22.0 et wasm-bindgen CLI 0.2.129 ; les dépendances natives et outils d’installation sont décrits dans [les paquets Rust](docs/rust-packages.md).

Node n’est pas un prérequis du runtime Rust et doit rester exclu de ses paquets finaux. Les sources et fixtures Node restent dans le dépôt comme références fonctionnelles et outils de comparaison, sans obligation de rétrocompatibilité. Le [README Node archivé](docs/archive/node-readme.md) est conservé intégralement ; ses liens relatifs se rapportent à la racine historique du dépôt.

Ces sources historiques ne constituent plus le produit ni le runtime Rust. Leur déplacement ou retrait physique R07 reste différé jusqu’aux preuves nécessaires pour conserver les fixtures et comparateurs utiles ; seul le README précédent est archivé ici. Le snapshot historique de migration est conservé au commit `7326122`.

## Commandes natives

Après fabrication et qualification dans un environnement autorisé, le binaire `atelier` fournit notamment :

```text
atelier --help
atelier --version
atelier launch --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets BUILT_RUST_UI --port 4317 --no-browser
atelier stop --data-dir EMPTY_SYNTHETIC_DIRECTORY --port 4317
atelier serve --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets BUILT_RUST_UI --port 4317
```

Remplacer les chemins d’exemple par des dossiers synthétiques explicites ; `BUILT_RUST_UI` désigne les assets Leptos/WASM fabriqués. `launch` démarre ou retrouve une instance locale ; `serve` garde le serveur au premier plan. Utiliser le même dossier et le même port pour l’arrêter. `serve-prototype` reste une entrée de développement distincte, pas une installation livrée. Voir [le lanceur](docs/rust-launcher.md).

L’installation signée dans un **nouveau** dossier dispose de la commande suivante ; elle exige une archive, ses métadonnées signées et une clé publique de confiance choisie séparément :

```text
atelier install --archive ARCHIVE.zip --signature SIGNED_METADATA.json --trust SYNTHETIC_PUBLIC_KEY.json --destination NEW_INSTALLATION --mode portable
atelier verify-installation
```

Ce sont des commandes candidates, pas des téléchargements disponibles. La fabrication, les formats Windows/Linux/macOS, les mises à jour et leur récupération sont décrits dans [les paquets Rust](docs/rust-packages.md). Aucun tag, release ou déploiement n’est autorisé par cette reconstruction.

## Développement et qualification

Suivre [AGENTS.md](AGENTS.md). Pendant l’implémentation : aucun build, test, Clippy ou scénario navigateur sur le poste de travail, et aucun lancement de workflow GitHub. Une campagne finale manuelle doit produire des résultats réellement exécutés et relus. Utiliser exclusivement des données, dépôts, identifiants et agents synthétiques ; ne pas ouvrir le `data/` utilisateur, un compte réel ou un service payant.

L’[inventaire des sources](docs/rust-migration-inventory.md) relie les capacités de référence au travail restant. Les [connecteurs Rust](docs/rust-connectors.md) documentent leurs contrats et limites. Les preuves et lacunes de qualification restent suivies dans le plan actif, sans présenter un scénario non exécuté comme réussi.

Licence : [AGPL-3.0-or-later](LICENSE).
