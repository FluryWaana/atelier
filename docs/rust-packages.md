# Paquets Rust de préproduction

Les composants décrits ici sont des **candidats non compilés et non exécutés**. Aucun paquet réel ni résultat de qualification n’existe encore pour ces changements. Le workflow Rust reste uniquement manuel ; aucune exécution n’a été lancée pendant l’implémentation. Aucun tag, release ni déploiement n’est autorisé.

## Fabrication et contenu

```text
cargo xtask third-party-notices NOTICES.txt
cargo xtask stage-package TARGET BINARY ASSETS NOTICES.txt OUTPUT
cargo xtask verify-package OUTPUT
cargo xtask archive-package OUTPUT ARCHIVE.zip
cargo xtask verify-archive ARCHIVE.zip TARGET
```

Cibles : Windows x64, Linux x64/ARM64, macOS x64/ARM64. Le payload contient uniquement le binaire `atelier`, l’interface Leptos/WASM et ses assets autorisés, LICENSE, README, avis de dépendances et manifeste. Le runtime Node, les sources/outils historiques, bases, jetons, sauvegardes, logs et exécutables de faisabilité sont exclus. Le manifeste donne tailles/SHA256/architecture ; sa seule présence n’authentifie pas une archive.

Staging et publication utilisent un temporaire voisin et refusent une destination existante. Le lecteur ZIP partagé borne entrées/tailles, vérifie chemins/types/collisions/CRC/SHA256/cible et refuse liens, chemins absolus ou traversées. L’exécutable reçoit son mode natif. Ces contrôles sont réutilisés par l’installation et l’update.

Les avis sont générés depuis `cargo metadata --locked --offline` et les textes locaux des crates, pour toutes les plateformes et outils de fabrication : source crates.io connue, fichier borné à 1 Mio et total 32 Mio, refus des textes manquants ou chemins extérieurs. Les avis entrent dans l’inventaire signé. La collecte réelle et l’examen des composants natifs embarqués restent à effectuer ; le générateur écrit ne constitue pas leur preuve.

Dépendances de fabrication : toolchain/Cargo.lock du dépôt, Trunk 0.22.0, wasm-bindgen CLI 0.2.129, compilateur natif et SQLite embarqué ; Inno Setup 6 sous Windows, dpkg-deb/rpmbuild/desktop-file-validate sous Linux. Le conteneur ZIP et Ed25519 réutilisent les crates verrouillées. Aucun de ces outils de fabrication n’est ajouté au runtime applicatif.

## Signature synthétique

```text
cargo xtask synthetic-update-key NEW_KEYS
cargo xtask sign-package ARCHIVE.zip TARGET NEW_KEYS SIGNED_METADATA.json
cargo xtask verify-signed-package ARCHIVE.zip SIGNED_METADATA.json NEW_KEYS/synthetic-update-public.json TARGET CURRENT_VERSION
```

Les clés sont exclusivement synthétiques. Le générateur refuse réutilisation/remplacement ; la clé privée PKCS8 reste dans le temporaire privé et n’entre jamais dans une livraison. La clé publique doit être choisie séparément du paquet : un document ne peut pas s’autoriser lui-même. La signature lie application/schéma, cible, version, taille/SHA256 d’archive et empreinte du manifeste. La préparation vérifie signature et version strictement supérieure avant lecture/extraction. Aucune clé de release n’est annoncée.

## Installation et update Rust

```text
atelier install --archive ARCHIVE.zip --signature SIGNED_METADATA.json --trust SYNTHETIC_PUBLIC_KEY.json --destination NEW_INSTALLATION --mode portable
atelier update-preview --archive NEXT.zip --signature NEXT.json --data-dir SYNTHETIC_RUST_DATA --port 4317
atelier update-status
atelier update-apply|update-cancel|update-recover --operation ID --confirm
```

L’installation refuse une destination existante. Elle conserve lanceur stable, confiance externe, reçus signés et slots `versions/<empreinte-archive>` ; `current.json` désigne version active et précédente. Le lanceur relit leur intégrité et empêche une version inactive de servir les données.

L’aperçu réserve son journal avant I/O et lie données/coffre/port/ancien pointeur/contenu signé. La confirmation lance un worker détaché qui arrête l’instance authentifiée, attend son verrou, bascule atomiquement, démarre sans navigateur et compare la **version réellement exécutée**. Échec de démarrage/version : retour du code puis vérification du démarrage précédent. Arrêt/reprise, changements humains et effets incertains restent explicites ; une commande ambiguë n’est pas rejouée.

Le rollback ne restaure, ne remplace et ne supprime jamais la base, les dépôts ou les fichiers humains. Pointeur/données/code modifiés, service étranger ou état illisible sont conservés pour examen. Les effets Jira/forge/Tempo/connecteurs et mises à jour de CLI incompatibles bloquent l’activation. L’écran bilingue conserve les brouillons et exige une confirmation liée à l’opération relue.

## Formats et maintenance

```text
cargo xtask build-installers ARCHIVE.zip SIGNED_METADATA.json PUBLIC_KEY.json NEW_OUTPUT
atelier verify-installation
atelier recover-package-maintenance --root ABSOLUTE_INSTALLATION --confirm
```

`build-installers` exige `CI=true` et `ATELIER_RUST_FINAL_CAMPAIGN=1`. Il prépare et vérifie une installation signée neuve : Setup x64 utilisateur Windows (`ATELIER_ISCC` absolu), DEB/RPM système Linux x64/ARM64 ; macOS utilise le portable. Les recettes n’embarquent aucun Node. Les scripts Linux sont conservés en LF et les modes des fichiers permettent lecture/exécution par les autres comptes.

Le serveur Windows conserve un mutex global entre sessions ; Setup/désinstallation réservent la maintenance et refusent toute instance active, sans arrêt forcé. Sous Linux, chaque serveur système conserve un verrou partagé ; le gestionnaire exige l’exclusivité et pose un marqueur qui empêche les nouveaux lancements. Un verrou système absent/modifié ou lié ne devient pas un accès portable implicite.

`verify-installation` contrôle bootstrap, slots et reçus sans ouvrir de données. Le retrait refuse code modifié, reçu étranger et historique nécessitant un examen. Les fichiers humains non suivis et données sont conservés. La récupération Linux exige verrou exclusif, marqueur exact et installation signée complète ; elle conserve le marqueur dans un reçu et ne répare pas silencieusement une installation partiellement effacée.

**Limite encore ouverte :** les formats natifs acceptent une installation neuve ; les mises à jour du paquet système et la récupération d’une extraction partielle restent à compléter/qualifier. L’updater Rust utilisateur ne donne pas au serveur les droits d’écrire dans `/opt`. Aucun succès de mise à jour via dpkg/rpm n’est revendiqué par les scénarios ci-dessous.

## Lanceur, tray et transfert

`launch/stop/serve` vérifient l’identité loopback/session/données ; l’ouverture ne relance pas une instance disparue et le tray est lié au PID de son serveur. Le jeton utilisé pour vérifier l’instance est conservé pour l’action, sans relecture vers une session remplacée. L’arrêt doit aussi attendre la libération du verrou de données avant de confirmer un redémarrage sûr.

Le serveur natif possède le helper tray du même exécutable : thread principal AppKit, boucle Windows, backend StatusNotifier Linux. Menus Ouvrir/Quitter bilingues, actions sérialisées, refus visible et retrait sur EOF du propriétaire. Aucun Node/Python/.NET ni jeton en arguments/environnement. Linux sans bureau/D-Bus n’ouvre pas de tray ; une création refusée laisse l’application accessible dans le navigateur. Windows/macOS exigent un rectangle réel avant READY ; le scénario Linux observe l’Id/pixels/état de l’item produit dans l’hôte D-Bus puis son retrait. Clics et focus restent à qualifier.

Le transfert Rust portable explicite réutilise `import-data` et son choix de source confirmé : source verrouillée, format ATKR contrôlé, copie privée bornée puis publication, aucun ancien format Node. Session/verrous/autorisation locale de dépôts ne sont pas transférés ; chiffrement intercompte non promis. Les sauvegardes sont conservées.

## Campagne finale préparée, non lancée

```text
cargo xtask qualify-packages .artifacts/rust-delivery
cargo xtask test-updates
cargo test -p atelier-app --test native-tray --locked -- --ignored
cargo test -p atelier-app --test native-import --locked -- --ignored
```

Le workflow manuel `rust.yml` prévoit la matrice native des cinq cibles et Edge/Chrome/Firefox/Safari. Le défaut `full_matrix=true` prépare la campagne complète ; une exécution partielle ne satisferait pas la checklist. Les paquets sont fabriqués seulement après les jobs natifs et navigateur réussis, avec assets WASM partagés. Aucun job ni commande ci-dessus n’a été exécuté ici.

`qualify-packages` construit le vrai binaire release, génère avis/ZIP/signature synthétique, installe puis exécute le lanceur portable : version observée, destination existante refusée, conversation conservée au redémarrage, session renouvelée et arrêt. Il produit les formats natifs et un rapport des seules étapes achevées ; il ne les marque pas installés.

Les scénarios natifs distincts prévoient Setup/réinstallation/refus d’un serveur actif ou code humain sous Windows ; DEB/RPM via vrais gestionnaires dans conteneurs Ubuntu/Fedora jetables de l’architecture native, utilisateur synthétique distinct, retrait actif refusé, reprise du marqueur, code modifié et données/fichiers humains conservés. Les logs/JSON effectifs et archives restent des artifacts CI de préproduction, jamais une release. La clé privée et les données de fixture n’y sont pas incluses.

`test-updates` compile une deuxième version depuis les sources Rust suivies, signe les deux paquets et vérifie update puis rollback après version réellement exécutée incorrecte, sans perdre la conversation. Les tests de desktop explicitement ignorés ne seront des preuves qu’après invocation réelle et lecture de leurs résultats. `native-import` concerne Windows ; les interactions POSIX du sélecteur restent à qualifier.

Linux utilise des sessions D-Bus/XDG et keyrings jetables. macOS crée un trousseau temporaire dédié/marqué et transmet son chemin sous garde CI/finale/jetable : lectures, écritures et suppressions de secrets lui sont explicitement liées, sans recherche dans le trousseau par défaut. Les probes de comptes/trousseaux déjà explicitement ciblés conservent leur scope. Les mots de passe/clés synthétiques restent hors arguments et journaux. Le backend refuse en CI un accès implicite au trousseau macOS par défaut.

État exact : implémentation et scénarios écrits, formatage/lecture seulement ; **aucune compilation, exécution, réussite ou livraison de paquets qualifiée**. Les vrais résultats, artefacts, erreurs/reprises et limites devront être relus avant de cocher R06/R07.
