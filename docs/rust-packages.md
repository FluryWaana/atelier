# Paquets Rust de préproduction

Le staging, les archives ZIP et le lanceur natif sont candidats **non compilés et non exécutés**. Aucun paquet réel n’a encore été produit ou qualifié. Le tray, les installateurs, la signature et l’update/rollback restent ouverts dans R06/R07. Aucune publication ni aucun tag ne sont autorisés par ces commandes.

Après construction lors de la campagne finale, utiliser :

```text
cargo xtask third-party-notices NOTICES.txt
cargo xtask stage-package TARGET BINARY ASSETS NOTICES.txt OUTPUT
cargo xtask verify-package OUTPUT
cargo xtask archive-package OUTPUT ARCHIVE.zip
cargo xtask verify-archive ARCHIVE.zip TARGET
```

`TARGET` vaut `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` ou `aarch64-apple-darwin`. Le binaire est fourni explicitement, avec une vérification de son en-tête PE/ELF/Mach-O et de son architecture ; cela ne prouve pas qu’il démarre. `ASSETS` est le répertoire produit par `cargo xtask build-ui`. La commande ne construit rien et ne parcourt aucun dossier de données.

Contenu autorisé : binaire `atelier[.exe]`, dossier `ui` contenant exclusivement l’index, le chargeur externe, les bindings/WASM et CSS Leptos, licence, notice de préproduction et manifeste `atelier-package.json`. Aucun fichier Node, source de référence, binaire de faisabilité, base, jeton ou journal n’est copié par découverte récursive du dépôt. Un asset inattendu fait échouer la préparation. Limites : 128 assets, 64 Mio par asset à la copie, 128 Mio pour le binaire et 256 Mio au total ; liens/reparse points refusés, fichiers lus avec identité recontrôlée. Le dossier de sortie doit être nouveau ; la préparation et son manifeste sont vérifiés dans un temporaire voisin avant publication du dossier.

Le manifeste recense version, cible, tailles et SHA-256. La vérification refuse fichier ajouté, absent ou modifié et en-tête incohérent. **Ces empreintes ne constituent pas une signature ni une racine de confiance pour les mises à jour.** Un futur updater ne doit pas les accepter comme preuve d’authenticité. Les tests utilisent uniquement de petits faux en-têtes et assets synthétiques ; seuls les artefacts natifs réels et les parcours de lancement de la campagne finale pourront qualifier le paquet.

Le validateur du répertoire est partagé avec le lecteur natif d’archives ; il n’est plus dupliqué dans xtask. Une version contient trois nombres sans zéro initial superflu ; sa comparaison future est numérique, pas lexicale. La vérification structurelle peut ainsi examiner un autre numéro de version que celui du vérificateur, sans promettre de compatibilité de données ni de droit à un downgrade.

Les cinq cibles utilisent le même conteneur ZIP Stored sans compression, chiffrement, volumes, commentaire global ou ZIP64. Le choix évite l’ajout de codecs ; la taille est bornée à 260 Mio, contenu à 257 Mio manifeste compris, 132 fichiers au plus. Avant le parseur, la fin du ZIP borne le répertoire central et son nombre d’entrées. Chemins strictement allowlistés, noms bruts ASCII inchangés, doublons/collisions de casse, liens/types spéciaux, droits setuid/setgid, chevauchements, octets non référencés et entrées hors contenu sont refusés. L’extraction se fait seulement dans un temporaire privé neuf ; aucun extracteur générique qui écrase un dossier existant n’est utilisé. Taille réelle et CRC sont lus, puis le manifeste complet, les SHA-256, le WASM et la cible sont revalidés. Sur POSIX seul le binaire reçoit les droits exécutables.

La création vérifie le payload source, écrit une archive temporaire, la relit/extrait/vérifie avec le lecteur partagé, recontrôle le source et son manifeste, puis publie sans remplacer un fichier existant. Ces opérations sont du code de fabrication, **pas des commandes exécutées pendant l’implémentation**. Les scénarios d’aller-retour des cinq cibles emploient de faux en-têtes, jamais des exécutables réels : ils ne prouvent ni lancement ni qualification d’OS. Parcours supplémentaires écrits pour traversée, chemins Node/données/absolus, liens, collisions, corruption, troncature, compteurs excessifs, versions et refus de remplacement ; **aucun exécuté**.

Fabrication prévue : toolchain et dépendances Cargo verrouillés, SQLite embarqué dans le binaire, Trunk 0.22.0 et wasm-bindgen CLI 0.2.129 pour les assets. Le conteneur est produit/lu en Rust avec [zip 8.6.0](https://docs.rs/crate/zip/8.6.0) (MIT), options par défaut désactivées, et typed-path 0.12.4 (MIT OR Apache-2.0). Les métadonnées et checksums ont été lus sur crates.io pour éditer le verrouillage ; résolution/compilation restent à vérifier dans la campagne finale. Aucun outil d’archivage Python/Node ni système n’est ajouté au runtime. Les outils Node de référence peuvent encore servir à l’inventaire de développement ; ils n’entrent pas dans le payload. Inventaire complet des licences/avis distribués et outils d’installation/signature restent à finaliser.

Le lecteur d’archives seul ne prouve pas l’authenticité. L’installateur/updater doit passer par l’authentification décrite ci-dessous avant toute acceptation ou exécution ; ni les CRC ni les SHA du manifeste ne la remplacent. Aucun paquet réel n’est signé, installé ou publié à ce stade.

## Signature synthétique et préparation authentifiée candidates

Commandes supplémentaires écrites, réservées aux fixtures de la campagne finale :

```text
cargo xtask synthetic-update-key NEW_SYNTHETIC_KEY_DIRECTORY
cargo xtask sign-package ARCHIVE.zip TARGET NEW_SYNTHETIC_KEY_DIRECTORY SIGNED_METADATA.json
cargo xtask verify-signed-package ARCHIVE.zip SIGNED_METADATA.json NEW_SYNTHETIC_KEY_DIRECTORY/synthetic-update-public.json TARGET CURRENT_VERSION
```

Le générateur exige un nouveau dossier, crée une paire Ed25519 synthétique et un document public séparé marqué synthétique ; il refuse toute réutilisation/remplacement. La clé privée PKCS8 reste dans ce dossier privé (0700/0600 sur POSIX), jamais dans le payload, les arguments ou les messages. Les scénarios doivent utiliser un compte/temporary dédié sous Windows ; aucune clé réelle ni aucun trousseau personnel ne sont consultés. Ces fonctions ne sont pas appelées pendant l’implémentation : **aucune clé n’a été générée sur ce PC**.

Le signataire revalide l’archive pour sa cible avant de lire la clé synthétique, puis signe une structure sérialisée stable : schéma/application, cible, version, taille/SHA-256 exacts de l’archive, SHA-256 du manifeste interne et empreinte de clé. Ce dernier lien permet de revalider les fichiers installés après retrait du ZIP. Crypto réutilisée : ring 0.17.14, déjà verrouillé. Le document signé ne contient pas de clé publique qui pourrait s’autoriser elle-même ; la confiance doit venir d’un document public choisi et conservé indépendamment. Le code candidat accepte uniquement la confiance synthétique, sans annonce de clé de release. Champs inconnus/dupliqués, formes de signature non canoniques et clés incompatibles sont refusés.

La préparation authentifiée vérifie contrat, clé et signature, puis cible et version strictement supérieure **avant de lire l’archive**. Version identique et downgrade sont refusés. Ensuite lecture bornée à la taille signée, empreinte des octets capturés, extraction privée et revalidation du manifeste interne ; sa version/cible doivent correspondre au document signé. La préparation retourne un temporaire vérifié ; elle ne lance ni ne remplace aucun programme, n’ouvre aucune donnée applicative et ne constitue pas une installation ou une mise à jour réussie.

Scénarios écrits : paire synthétique/paquet à faux en-tête, préparation valide puis nettoyage du temporaire, refus de clé étrangère, métadonnées modifiées, archive corrompue, mauvaise architecture, version identique/antérieure et remplacement de fichiers existants ; les refus de signature/version/cible sont contrôlés avec un chemin d’archive inexistant pour vérifier l’ordre des accès. **Non compilés/non exécutés.** Le cycle raccordé ci-dessous reste candidat ; aucune vérification distante, publication ou qualification n’en découle.

## Installation versionnée et récupération candidates

```text
atelier install --archive ARCHIVE.zip --signature SIGNED_METADATA.json --trust SYNTHETIC_PUBLIC_KEY.json --destination NEW_INSTALLATION --mode portable
atelier update-preview --archive NEW_ARCHIVE.zip --signature NEW_SIGNED_METADATA.json --data-dir EXISTING_SYNTHETIC_RUST_DATA --port 4317
atelier update-status
atelier update-apply --operation OPERATION_ID --confirm
atelier update-cancel --operation OPERATION_ID --confirm
atelier update-recover --operation OPERATION_ID --confirm
```

L’installation CLI refuse une destination existante et publie un dossier neuf après authentification et relecture complète. Modes portable/user/system typés ; aucun transfert de données implicite ni modification d’une ancienne installation Node. Le dossier contient un lanceur stable, une clé publique de confiance séparée, des reçus signés et des versions dans `versions/<empreinte-archive>`. `current.json` désigne version active et précédente. Le lanceur sélectionne l’exécutable et les assets actifs vérifiés ; une ancienne version appelée directement ne peut pas servir les données par erreur.

L’aperçu réserve son journal avant lecture/extraction de l’archive et préparation d’une version immuable. Il lie identité canonique des données Rust, identité de coffre, port, ancienne sélection de version et contenu signé. Une copie interrompue ne devient pas active. La confirmation revalide données, pointeur et fichiers avant de lancer le worker de la version préparée. Le worker arrête l’instance authentifiée, attend son verrou de données, change le pointeur atomiquement, démarre sans navigateur et compare la **version réellement exécutée** à la version signée. Le journal distingue préparation, attente, activation, tentative, validation, rollback et incertitude ; aucun renvoi implicite d’une commande ambiguë.

Pendant attente/activation/tentative/récupération, les mutations HTTP sont refusées, avec les arrêts et la récupération exacte toujours accessibles. Le serveur vérifie l’identité des données avant ouverture/récupération SQLite. Une erreur de démarrage déclenche le retour du pointeur puis la relecture du démarrage précédent. **La base, les dépôts et les fichiers humains ne sont jamais remplacés/restaurés par ce rollback de code.** Les effets Jira/forge/Tempo persistés restent dans les données. Un pointeur modifié hors opération, un dossier de données remplacé, un service étranger ou une ancienne version altérée sont conservés et signalés pour examen, sans kill arbitraire ni écrasement.

Le lanceur reprend une récupération interrompue à sa prochaine ouverture. La réserve garde versions active/précédente ; les versions signées devenues inactives peuvent être déplacées vers un nom `.retired-*`, avec leurs octets conservés. Une modification humaine empêche ce classement automatique. Parcours et taille de la réserve bornés (1 024 entrées, profondeur limitée, 1 Gio), aucun nettoyage automatique des données. Le cycle ne promet pas de migration de format SQLite ni de compatibilité Node.

L’écran bilingue Mises à jour est le sixième onglet des paramètres : chemins absolus, état de préparation, empreintes, contenu du paquet, confirmation du redémarrage, annulation et récupération. Les autres brouillons bloquent le redémarrage ; les erreurs conservent les saisies. Après une réponse perdue, une nouvelle action exige une relecture ; après rotation de session, le lanceur permet de rouvrir la session pour lire le résultat. Le format système et la racine de confiance ne sont pas exposés comme commandes techniques dans le parcours utilisateur.

| Parcours | Implémentation | Scénario | Exécution / résultat |
| --- | --- | --- | --- |
| Dossier neuf, refus d’écrasement, contenu signé installé et édition humaine conservée | Candidate | Rust/FS écrit | Non exécuté / en attente |
| Reprise aux frontières attente/activation/tentative/rollback et données inchangées | Candidate | Rust/FS/loopback écrit, faux en-têtes explicitement utilisés | Non exécuté / en attente |
| Identité de données/pointeur changé, journal corrompu, mutations bloquées | Candidate | Régressions écrites | Non exécuté / en attente |
| Deux vrais binaires compilés de versions différentes, session renouvelée et conversation persistée | Candidate | `cargo xtask test-updates` écrit | Non exécuté / en attente |
| Paquet signé annonçant une version différente de son exécution, rollback réel et cause précise | Candidate | Parcours natif écrit | Non exécuté / en attente |
| Brouillons, confirmation, redémarrage et écran après rollback | Candidate | WebDriver écrit | Non exécuté / en attente |
| Windows Setup, DEB/RPM, intégration bureau/tray, désinstallation, avis complets de licences | Incomplet | À compléter | Aucune qualification |

`test-updates` exige CI et `ATELIER_RUST_FINAL_CAMPAIGN=1`. Il copie uniquement les sources Rust suivies par Git dans un temporaire, y incrémente les versions du workspace/verrouillage et construit un second binaire hors source de travail. Dossiers, clés, données et port restent synthétiques ; aucun agent ni compte distant. **Ce code de scénario n’a pas été exécuté : aucun second binaire, aucune clé et aucun paquet n’ont été produits sur le PC.** Le workflow manuel et la campagne unique n’ont pas été lancés ; R06/R07 restent non validés.

Point candidat — avis des dépendances : third-party-notices lit les métadonnées Cargo --locked --offline et les textes locaux des crates, sans compilation ni réseau. Inventaire déterministe de toutes les plateformes et outils de fabrication, expression de licence déclarée, fichiers de licence/notice bornés ; refus explicite des sources non recensées, textes manquants, chemins extérieurs et sorties existantes. Ce recensement ne remplace pas l’examen final des obligations de distribution et des composants natifs éventuellement embarqués par les crates.

stage-package exige maintenant le fichier d’avis en argument et embarque THIRD-PARTY-NOTICES.txt dans l’inventaire signé, les ZIP et les versions installées. La limite dédiée est de 32 Mio ; aucun dossier/runtime Node autorisé. Les fixtures d’archive des cinq cibles conservent un avis explicitement synthétique et vérifient sa conservation ; les parcours de mise à jour génèrent les vrais avis avant leur packaging. Générateur et scénarios écrits seulement ; aucun inventaire Cargo ni paquet généré/exécuté sur le PC. La collecte réelle peut révéler des licences manquantes à compléter ; cette ligne reste non qualifiée.