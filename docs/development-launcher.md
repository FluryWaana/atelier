# Lanceur de développement : contrat et qualification

Le 28 octobre 2026, le propriétaire autorise les tests locaux Windows du lanceur et du rechargement, avec données synthétiques uniquement. Les vérifications se poursuivent le 29 octobre. Cette autorisation ne lance aucune CI et ne qualifie pas toute l'application. Linux et macOS restent au stade de conception.

## Comportement attendu sur les trois systèmes

- Utiliser Rust installé par l'utilisateur, le PATH, `rust-toolchain.toml` et `Cargo.lock`. Ne pas imposer un Cargo/Rustup privé dans le dépôt. Trunk 0.22.1 et wasm-bindgen CLI 0.2.130 doivent être installés dans le PATH.
- Démarrer avec les dernières sorties valides si les sources sont inchangées. Recompiler si une sortie requise manque. Une tentative échouée ne valide jamais son cache.
- Surveiller les sources, les traductions, HTML/CSS et la configuration ; exclure `target`, `.artifacts`, `.git` et `data` à tous les niveaux, sans parcourir ces dossiers. Refuser les chemins sources liés. Regrouper les sauvegardes rapprochées. Détecter aussi les ajouts et suppressions.
- Reconstruire seulement l'UI pour ses changements, seulement le serveur pour les siens, les deux pour le code partagé. Récupérer les dépendances après changement des manifestes, du verrou ou de la configuration Cargo.
- Garder le serveur valide disponible pendant la compilation UI. Arrêter le serveur avant de remplacer son exécutable. Publier une nouvelle version uniquement après compilation réussie et démarrage confirmé.
- Recharger l'onglet après publication ; demander confirmation après une saisie. Une annulation conserve le brouillon ; le rechargement manuel reste disponible. Le mode actuel recharge toute la page, sans remplacement des composants à chaud.
- Conserver la session pendant les redémarrages de développement, sans assouplir Host/Origin, l'authentification locale, la CSP ou les limites des assets. N'injecter le rechargement qu'en mode `serve --dev`.
- Exclure les doubles superviseurs par verrou exclusif. Arrêter via la commande authentifiée `stop`, vérifier l'identité des données, attendre la libération du serveur et ne jamais tuer le propriétaire inconnu d'un port.
- Afficher les erreurs et réessayer à la prochaine sauvegarde. Conserver les données synthétiques après l'arrêt. Ne jamais utiliser le dossier utilisateur `data/`.

## Linux et macOS : conception à réaliser

La logique commune doit aller dans une commande de développement de `xtask`, avec les processus et accès disque à ce niveau, sans nouvelle dépendance tant que la bibliothèque standard suffit. Les lanceurs Windows et Unix ne feront que choisir le répertoire du dépôt et appeler cette commande. Il n'existe pas encore de lanceur Unix qualifié.

| Sujet | Linux | macOS |
| --- | --- | --- |
| Chemins | `Path`/`PathBuf`, noms sensibles à la casse, espaces et Unicode | Même contrat, à vérifier sur volumes sensibles et insensibles à la casse |
| Exécutables | Suffixe fourni par la plateforme, permissions d'exécution | Même contrat, architectures Intel et Apple Silicon |
| Surveillance | Inventaire avec temporisation pour commencer ; notifications natives seulement si un coût mesuré le justifie | Même contrat, sauvegardes atomiques d'éditeurs et renommages à vérifier |
| Arrêt | SIGINT/SIGTERM vers le superviseur déclenchent l'arrêt authentifié et l'attente de ses seuls enfants | Même contrat ; aucun arrêt global par nom de processus |
| Navigateur | Réutiliser l'ouverture native existante ; absence de session graphique explicite | Réutiliser l'ouverture native existante ; vérifier aussi les chemins contenant des espaces |
| Stockage protégé | Session D-Bus/XDG jetable pour les tests de coffre | Trousseau temporaire dédié uniquement, jamais le trousseau personnel |

Une qualification native devra couvrir : lancement initial et à chaud, HTML/CSS/Rust/traductions, créations/suppressions, sauvegardes en rafale, échec puis reprise, saisie refusée/acceptée, session persistante, port occupé par un tiers, double lancement, interruption et absence de processus orphelins. Ne pas déduire une réussite Unix d'une compilation croisée Windows.

## Windows : vérification reproductible

`scripts/atelier-watch.tests.ps1` vérifie les décisions de surveillance et de reconstruction sur des fichiers synthétiques : sorties imbriquées ignorées, assets ou binaire manquants, changement de cache Cargo, échec de récupération des dépendances, chemins hors dépôt et port tiers. Il ne compile pas et n'utilise pas de navigateur.

Sous PowerShell 7, `scripts/atelier-browser.tests.ps1 -Driver CHEMIN_MSEDGEDRIVER` exerce le vrai lanceur, Cargo/Trunk, le serveur `serve --dev` et Edge headless. Il vérifie les versions du navigateur/pilote, le démarrage, l'arrêt supervisé, la relance sans reconstruction, le verrou Windows PowerShell 5.1, HTML/CSS, le serveur seul, les confirmations du brouillon et l'échec/reprise UI. Il compare les jetons déchiffrés et vérifie l'accès authentifié avec le jeton initial ; les octets du chiffrement DPAPI peuvent changer pour un jeton identique. Il modifie temporairement les sources nécessaires, sauvegarde leurs octets initiaux dans `originals/`, refuse d'écraser une modification concurrente et les restaure en sortie. Ne pas éditer ces fichiers pendant ce parcours. Les rapports locaux restent dans `.artifacts/launcher-verification/` ; aucun jeton n'y est écrit.

## Résultats Windows des 28–29 octobre 2026

Rust système 1.99.1, Cargo provenant de `%USERPROFILE%\.cargo\bin`, Trunk 0.22.0, wasm-bindgen 0.2.129 ; Edge et WebDriver **156.0.4314.50**. Le pilote initial 155 a été remplacé pour la campagne retenue : Microsoft exige que les trois premières composantes [correspondent à celles du navigateur](https://learn.microsoft.com/en-us/microsoft-edge/webdriver-chromium/).

Les assertions du script de surveillance passent sous PowerShell 7 et Windows PowerShell 5.1. Le parcours Edge complet réussit, puis les sources initiales sont restaurées et reconstruites. Le jeton initial donne toujours accès à `/api/state`. Un lancement réel séparé sous Windows PowerShell 5.1 réutilise les sorties et réussit en **2,96 s**, suivi de l'arrêt authentifié.

Les quatre tests `http::assets::tests` passent avec le Rust système : noms d'assets, bornes et fichiers inattendus, injection limitée du rechargement et refus des liens physiques. Les connexions loopback et liens physiques étaient bloqués dans le bac à sable ; la campagne retenue a été exécutée hors de cette restriction sur les seuls fichiers et processus synthétiques.

`scripts/atelier-windows51.tests.ps1` vérifie séparément le vrai moteur utilisé par `Atelier.cmd`, sans navigateur : publication d'une modification HTML puis restauration et reconstruction des octets initiaux. Ce parcours réussit avec une reconstruction observée à **5,48 s**. Il a révélé l'indisponibilité de `Get-FileHash` dans cet environnement de lancement ; l'empreinte utilise désormais SHA-256 .NET en lecture par flux, vérifié de nouveau sous PowerShell 7 et 5.1. Les options `-NoBrowser` et `-Port` du lanceur permettent ces essais sans ouvrir d'onglet ni imposer le port habituel.

| Parcours Edge final | Résultat | Temps observé |
| --- | --- | ---: |
| Démarrage avec vérification des compilations déjà en cache | Réussi | 9,90 s |
| Arrêt supervisé | Réussi | 3,53 s |
| Relance sans reconstruction | Réussi | 2,26 s |
| Double lancement refusé sous Windows PowerShell 5.1 | Réussi | 0,27 s |
| Sauvegarde HTML → document actualisé | Réussi | 5,95 s |
| Sauvegarde CSS → style appliqué | Réussi | 14,17 s |
| Refus du rechargement et conservation du brouillon | Réussi | 14,11 s |
| Serveur seul et acceptation du rechargement | Réussi | 16,27 s |
| Erreur UI sans interruption, correction et reprise | Réussi | 13,97 s |
| Sources initiales restaurées et reconstruites | Réussi | 11,44 s |

Mesures locales dans l'arbre de travail en cours, sans isolation de la charge du poste : elles ne sont pas un benchmark comparatif. La première reconstruction après passage au Rust système a reconstruit les caches et pris plusieurs minutes. Le rechargement actuel remplace toute la page ; il ne fournit pas de mise à jour CSS instantanée. Les premières tentatives ont aussi révélé des erreurs du harnais (commentaire hors `html`, saisie avant disponibilité du WASM, comparaison du chiffrement DPAPI, mauvaise route de vérification), corrigées avant la campagne finale réussie. Les rapports partiels de ces tentatives ne constituent pas des réussites complètes.

L'arrêt supervisé est exercé par annulation du runspace et par la commande authentifiée. La frappe physique de Ctrl+C et la fermeture forcée de la console ne sont pas qualifiées par ces tests. Les autres versions de Windows, les autres navigateurs, Linux et macOS ne sont pas qualifiés ici. Aucun Clippy ni test global du workspace n'a été lancé pour cette campagne ciblée.

Les compilations et parcours doivent être relus avant d'ajouter un résultat. La couverture du lanceur ne remplace ni les parcours fonctionnels de toute l'application ni une garantie de « 100 % » de situations possibles.
