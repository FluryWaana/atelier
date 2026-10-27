# Livraison Node — archive historique

Ces 22 fichiers suivis ont été déplacés depuis la racine en conservant leur arborescence interne : `launcher.mjs`, `Start.cmd`, `Stop.cmd`, `Start.command`, `Stop.command`, onze outils sous `scripts/` et les six fichiers de `scripts/installers/`. Leur contenu est conservé sans modification. Ils ne sont plus des lanceurs, installateurs, outils de tray ou commandes de livraison actifs du produit Rust.

Leurs chemins/imports supposent la racine historique du dépôt : cette archive n’est pas un paquet Node exécutable autonome. Pour consulter ou reproduire la référence complète, utiliser le snapshot Git **`7f48996b9626ccff79ecf0e1be7f8fa43b7b90e6`** dans une copie isolée avec des données synthétiques. Les tests Node de livraison, installation et tray restés sous `test/`, ainsi que les entrées de livraison historiques de `package.json`, se rapportent à ce snapshot ; ils ne sont pas des commandes de qualification Rust actives et ne sont pas attendus comme exécutables depuis l’arborescence actuelle.

`lib/`, `public/`, `test/`, les fixtures JSON intégrées aux tests Rust et les scripts de comparateurs restent à leur emplacement. Les deux scripts CI Rust keyring restent sous `scripts/`. Aucun runtime non suivi, dossier utilisateur ou `data/` n’a été déplacé ou supprimé. Le générateur d’inventaire recense explicitement cette archive comme historique.

Voir le [README Rust actuel](../../../README.md) et les [paquets Rust](../../rust-packages.md). Les déplacements de sources ne constituent aucune preuve de qualification des fonctionnalités ou paquets.
