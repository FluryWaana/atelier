# Architecture Rust

Le dépôt est un workspace Cargo à la racine, avec exactement quatre membres :

- `crates/atelier-core` : modèles, validation, règles métier et contrats publics, sans accès aux fichiers, au réseau ou aux processus.
- `crates/atelier-app` : binaire natif `atelier`, composition des cas d’usage, HTTP loopback, SQLite, coffre système, fichiers, Git, agents et intégrations.
- `crates/atelier-ui` : interface Leptos CSR compilée en WebAssembly, servie avec ses assets par le binaire natif.
- `xtask` : fabrication de l’interface, inventaire, qualification synthétique, signatures de préproduction et paquets.

Les dépendances et la toolchain sont fixées par [Cargo.lock](../Cargo.lock) et [rust-toolchain.toml](../rust-toolchain.toml). Le produit n’embarque aucun runtime Node. Les fixtures JSON de régression sont conservées dans les dossiers `tests/fixtures/` de leurs crates. Leurs attentes suivent les contrats actuels. Les dossiers de services sont validés à l’enregistrement et au lancement avec les mêmes règles, notamment le refus des flux de fichiers Windows et des noms réservés.

Le serveur écoute sur loopback et vérifie session locale, Host et Origin. Les identifiants restent dans le stockage protégé natif ; ils ne sont pas exposés au navigateur. Les accès aux dépôts demandent une autorisation explicite et restent bornés. Les processus d’agents et de connecteurs sont supervisés, avec entrées privées, sorties bornées et arrêt des descendants.

Le clonage de projet utilise Git installé, sans nouvelle dépendance. Son contrat typé et la validation du nom de dossier appartiennent au core. Le transport et la réservation de destination appartiennent à app. HTTPS accepte un dépôt public ou une connexion de forge choisie, limitée à son origine exacte. SSH accepte les URL `ssh://` et `utilisateur@hôte:chemin`, avec les clés de l’agent SSH et le fichier `~/.ssh/known_hosts`. Il ne charge ni clé privée, ni configuration SSH personnelle et refuse les hôtes inconnus. Les alias SSH et les gestionnaires personnels d’identifiants Git ne sont pas utilisés.

Une mutation exclusive réserve le clonage avant le réseau. Le dossier parent est ouvert sans suivre de lien. Un dossier temporaire unique reçoit les objets, puis les fichiers de la branche par défaut. Le téléchargement et le contenu déplié sont chacun limités à 512 Mio, avec au plus 10 000 fichiers et des processus bornés. Aucun hook, filtre personnel ou sous-module distant n’est exécuté. La publication refuse atomiquement toute destination existante. L’autorisation locale vérifie l’identité du dossier publié. L’annulation arrête les descendants avant de retirer le dossier temporaire vérifié. Un arrêt brutal peut laisser ce dossier temporaire, qui n’est jamais repris ou effacé automatiquement. Le dépôt terminé est conservé si son autorisation ou la création du projet échoue. L’interface permet alors de l’ajouter comme dossier existant sans recloner.

SQLite possède une identité et une version Rust explicites ; une base inconnue est refusée sans effacement. Les mutations revalident leurs instantanés, les transactions restent courtes et les opérations externes sont réservées avant l’I/O. Journaux persistants, confirmations et rapprochements traitent les effets distants incertains sans répétition aveugle. Les restaurations et mises à jour doivent conserver les modifications humaines ; les opérations incompatibles restent bloquées pendant les runs, services et autres opérations réservées.

L’application est en préproduction. Une implémentation ou un scénario écrit ne vaut pas qualification : consulter la [checklist fonctionnelle](features.md) et les [résultats réels de la campagne](rust-final-qualification.md). Les contrats des [connecteurs](rust-connectors.md), du [lanceur](rust-launcher.md), des [agents et mises à jour CLI](rust-agent-cli.md) et des [paquets](rust-packages.md) détaillent leurs protections et limites.
