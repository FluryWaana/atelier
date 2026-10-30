# Méthode et vérifications de l'audit Atelier Kanban

Cette revue recherche des problèmes observables dans le code actuel, puis les distingue des hypothèses et des preuves d'exécution. Elle tient compte de la reconstruction de préproduction et de l'absence d'obligation de rétrocompatibilité.

## Corpus et état local

Le relevé porte sur la branche `main`, basée sur `7a5bf7d264c6f3f456539fb588e524b65c6a04f4`, avec les modifications de travail déjà présentes. Elles n'ont pas été intégrées à un commit de code par cet audit. Les modifications du produit n'ont pas été corrigées ou nettoyées.

L'inventaire utilise les fichiers suivis et non ignorés de `crates`, `xtask`, `scripts`, `docs`, `.github`, `.cargo` et les fichiers racine explicitement listés : `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `README.md`, `AGENTS.md`, `Atelier.cmd`, `LICENSE` et `.gitignore`. Le dossier de cet audit est exclu de son propre inventaire.

Les chemins `data/`, les sorties de compilation, les artefacts de développement, les répertoires de configuration personnelle et l'historique du code Node sont hors corpus. Aucun contenu utilisateur n'a été ouvert. Les PNG, icônes et avis de licence sont inventoriés mais ne sont pas assimilés à du code Rust revu.

Le relevé initial compte 467 fichiers, dont 341 fichiers Rust et 98 745 lignes Rust physiques. Ces valeurs incluent les tests et ne sont ni une mesure de complexité ni une preuve de couverture sémantique.

## Niveaux de couverture

L'[inventaire CSV](inventaire-fichiers.csv) attribue un niveau explicite à chaque fichier et enregistre son empreinte SHA256.

| Niveau | Ce qui a été fait | Ce que cela ne prouve pas |
| --- | --- | --- |
| Lecture ciblée | Lecture de fonctions ou d'extraits, recherche des usages et comparaison avec les règles du projet | Lecture intégrale du fichier ou validation de toutes ses branches |
| Recherche transversale | Recherches dans les sources et tests pour frontières I/O, unsafe, erreurs, HTML, TLS, processus, secrets ou appelants | Absence de défaut si aucun motif n'est trouvé |
| Contexte documentaire | Lecture du document ou de sections pertinentes pour l'architecture et les restrictions | Vérification indépendante des anciens résultats rapportés |
| Inventaire uniquement | Présence, taille et empreinte relevées | Lecture ou analyse du contenu |

Les commandes d'inventaire lisent les octets pour établir les empreintes. Ce traitement automatique ne doit pas être décrit comme une relecture du contenu de tous les fichiers. Aucun fichier ne reçoit un niveau de lecture intégrale par défaut.

## Parcours examinés

| Parcours | Points suivis | Profondeur et limite |
| --- | --- | --- |
| Démarrage et session | CLI, binding loopback, session aléatoire, contrôle Host et Origin, preuve HMAC, stockage navigateur | Fonctions clés lues, comportement natif non exécuté |
| Requête métier | Validation JSON, gardes, concurrence, worker SQLite et réponse HTTP | Middleware et fonctions de stockage lus par sections |
| Temps et modèles | Traits métier, `RawValue`, adaptateurs `Store`, lookup de carte | Appelants directs du mécanisme historique recherchés et confirmés |
| Export Tempo | Aperçu, réservation, appel externe, mémorisation, rapprochement et statut incertain | Sections de l'adaptateur et du handler lues, parcours réseau non exécuté |
| Fichiers et Git | Chemins, liens, identité, empreinte de dépôt et environnement Git | Sections sensibles lues, pas de tentative de course système |
| Processus et extensions | Sorties bornées, délais, ownership, job Windows, groupes Unix, empreinte et copie de binaire | Sections clés lues. Tous les programmes appelants et tous les blocs unsafe ne sont pas validés |
| Sauvegarde et mises à jour | Format SQLite, intégrité, secours, écriture atomique, journal et signature | Recherche des appels et des synchronisations. Panne électrique non simulée |
| Interface | Session, flux SSE, rechargement d'état, traduction et état d'un composant de transition | Lecture ciblée. Écrans, clavier, focus et brouillons non qualifiés au navigateur |
| Livraison | Manifestes, outils verrouillés, fabrication UI et workflow manuel | Configuration lue par sections. Aucun paquet fabriqué ou installé |

## Contrôle des constats

Chaque constat possède un cas concret, un emplacement, une correction proportionnée et une conséquence. Les constats 1, 2 et 4 reposent sur un mécanisme ou un écart vérifié dans les sources. Le constat 3 décrit un couplage de maintenance, pas une course démontrée. Le constat 5 décrit une exposition conditionnelle du jeton, pas une fuite avérée. Le constat 6 repose sur une recherche d'usages directs, sans compilation de suppression.

L'absence de synchronisation du dossier a été confrontée à la [documentation système Linux](https://man7.org/linux/man-pages/man2/fsync.2.html). Les versions de bibliothèques proviennent des manifestes. Les raisons de choix technologiques sont présentées comme des intérêts techniques observables, sans inventer une décision historique.

Les capacités et anciens résultats de qualification cités par les documents de projet ne sont pas promus en résultats actuels. La présence d'un test n'autorise pas à écrire qu'il passe. Les limites déjà déclarées dans des commentaires `shortcut:` ne sont pas requalifiées automatiquement en nouvelles anomalies.

## Ordre proposé des suites

| Ordre | Action | Critère de sortie proposé |
| --- | --- | --- |
| 1 | Corriger la durabilité du journal Linux, constat 1 | Publication acquittée après synchronisation requise et erreurs propagées. Qualification de reprise séparée |
| 2 | Corriger la phrase du README, constat 4 | Une seule consigne cohérente avec l'interdiction actuelle de CI |
| 3 | Typer les identifiants Temps et Modèles, constat 2 | Refus explicite des types non texte. Plus d'émulation SQLite Node dans ces appelants. Parcours utilisateur conservés |
| 4 | Rapprocher routes et politique de concurrence, constat 3 | Chaque mutation à effet dispose d'une classification vérifiable. Arrêts et annulations toujours accessibles |
| 5 | Encadrer la collecte des URL SSE, constat 5 | Aucun jeton de session dans les journaux ou diagnostics produits. Autre transport à étudier seulement si nécessaire |
| 6 | Qualifier les risques restants | Parcours synthétiques, croissance des données, plateformes natives et dépendances contrôlés dans un cadre autorisé |
| 7 | Retirer la dépendance directe sans appelant, constat 6 | Compilation vérifiée et verrou cohérent après retrait. Aucun gain chiffré supposé |

Aucun de ces correctifs n'est appliqué par la demande d'audit. Aucun gain de lignes ou de dépendances n'est chiffré sans travail de suppression et vérification des usages.

## Vérifications non exécutées

Les instructions du dépôt interdisent actuellement les tests locaux, Clippy, les scénarios navigateur et tout lancement ou relance de CI. Une demande de rapport d'audit n'a pas été interprétée comme une levée de ces restrictions. Aucune compilation n'a été lancée non plus, car ce livrable ne change que la documentation.

La campagne finale prévue par le dépôt inclut le formatage Rust, Clippy et tests du workspace hors UI, les vérifications WASM séparées, puis les vrais parcours WebDriver sur données synthétiques. Elle nécessite l'autorisation applicable au moment de son exécution.

Restent également non vérifiés : base d'avis de sécurité des dépendances, tests de pénétration actifs, preuve de toutes les interfaces SOLID, analyse exhaustive de chaque bloc unsafe, limites CPU et mémoire en exécution, arrêts multi-OS, permissions effectives des coffres et récupération sur panne électrique. Cet audit n'est donc pas une autorisation de publication ni une déclaration d'achèvement R07.
