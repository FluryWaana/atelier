# Paquets Rust de préproduction

Le staging ci-dessous est implémenté mais **non compilé et non exécuté**. Aucun paquet réel n’a encore été produit ou qualifié. Il prépare le contenu applicatif ; le lanceur produit, le tray, les installateurs, la signature et l’update/rollback restent ouverts dans R06/R07. Aucune publication ni aucun tag ne sont autorisés par cette commande.

Après construction lors de la campagne finale, utiliser :

```text
cargo xtask stage-package TARGET BINARY ASSETS OUTPUT
cargo xtask verify-package OUTPUT
```

`TARGET` vaut `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` ou `aarch64-apple-darwin`. Le binaire est fourni explicitement, avec une vérification de son en-tête PE/ELF/Mach-O et de son architecture ; cela ne prouve pas qu’il démarre. `ASSETS` est le répertoire produit par `cargo xtask build-ui`. La commande ne construit rien et ne parcourt aucun dossier de données.

Contenu autorisé : binaire `atelier[.exe]`, dossier `ui` contenant exclusivement l’index, le chargeur externe, les bindings/WASM et CSS Leptos, licence, notice de préproduction et manifeste `atelier-package.json`. Aucun fichier Node, source de référence, binaire de faisabilité, base, jeton ou journal n’est copié par découverte récursive du dépôt. Un asset inattendu fait échouer la préparation. Limites : 128 assets, 64 Mio par asset à la copie, 128 Mio pour le binaire et 256 Mio au total ; liens/reparse points refusés, fichiers lus avec identité recontrôlée. Le dossier de sortie doit être nouveau ; la préparation et son manifeste sont vérifiés dans un temporaire voisin avant publication du dossier.

Le manifeste recense version, cible, tailles et SHA-256. La vérification refuse fichier ajouté, absent ou modifié et en-tête incohérent. **Ces empreintes ne constituent pas une signature ni une racine de confiance pour les mises à jour.** Un futur updater ne doit pas les accepter comme preuve d’authenticité. Les tests utilisent uniquement de petits faux en-têtes et assets synthétiques ; seuls les artefacts natifs réels et les parcours de lancement de la campagne finale pourront qualifier le paquet.

Fabrication prévue : toolchain et dépendances Cargo verrouillés, SQLite embarqué dans le binaire, Trunk 0.22.0 et wasm-bindgen CLI 0.2.129 pour les assets. Les outils de référence Node peuvent encore servir à l’inventaire de développement ; ils n’entrent pas dans le payload Rust. Les outils d’installation, de signature et d’archivage seront recensés à leur livraison.
