# Paquets Rust de préproduction

Les composants décrits ici sont des **candidats en cours de qualification finale**. Aucun paquet n'est encore déclaré qualifié. Le workflow Rust reste uniquement manuel ; aucune exécution n'a été lancée pendant l'implémentation. La campagne finale sur `1ba8e3b` est suivie dans [le relevé des résultats](rust-final-qualification.md) ; les paragraphes « non exécuté » ci-dessous décrivent la préparation et ne préjugent pas de son résultat. Aucun tag, release ni déploiement n'est autorisé.

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

Les avis sont générés depuis `cargo metadata --locked --offline` et les textes locaux des crates, pour toutes les plateformes et outils de fabrication : source crates.io connue, fichier borné à 1 Mio et total 32 Mio, refus des textes manquants ou chemins extérieurs. Les archives qui omettent leurs textes utilisent uniquement les compléments amont explicites de `xtask/licenses/` : nom/version, déclaration, dépôt, révision Cargo, checksum verrouillé et SHA256 des textes sont vérifiés hors ligne. Chaque avis donne aussi le lien de son archive source exacte. Les inconnus restent refusés ; cet inventaire de déclarations ne constitue pas une certification juridique. Les avis entrent dans l’inventaire signé. La collecte réelle et l’examen des composants natifs embarqués restent à effectuer ; le générateur écrit ne constitue pas leur preuve.

Dépendances de fabrication : toolchain/Cargo.lock du dépôt, Trunk 0.22.1, wasm-bindgen CLI 0.2.130, compilateur natif et SQLite embarqué ; Inno Setup 6 sous Windows, dpkg-deb/rpmbuild/desktop-file-validate sous Linux. Le conteneur ZIP et Ed25519 réutilisent les crates verrouillées. Aucun de ces outils de fabrication n’est ajouté au runtime applicatif.

`cargo xtask build-ui` utilise le profil release de Trunk, avec les mêmes versions épinglées et dépendances verrouillées hors ligne. Le WASM debug n'est pas le payload de livraison. Le serveur conserve sa borne de 64 Mio par asset et de 256 Mio au total ; un refus de lecture identifie l'asset concerné.

`cargo xtask build-ui --dev` est réservé à l’aperçu local : Trunk ne reçoit pas `--release`, sa destination absolue est `.artifacts/rust-ui-dev` et son sous-processus reçoit uniquement pour cette commande `CARGO_PROFILE_DEV_DEBUG=0` et `CARGO_PROFILE_DEV_INCREMENTAL=true`. Le profil Cargo applique `opt-level=1` à `atelier-ui`. Les versions épinglées, le verrouillage hors ligne et l’externalisation du chargeur restent identiques. `Atelier.cmd` utilise cette destination ; la CI navigateur et les paquets conservent le profil release et `.artifacts/rust-ui`. Après production, les deux profils refusent explicitement un asset supérieur à 64 Mio ou un total supérieur à 256 Mio.

Mesure locale du premier profil dev, avant cet ajustement : compilation réussie en 59 s puis reconstruction inchangée en 7,82 s, mais WASM de **277 613 094 octets**, supérieur à la limite ; cet essai ne produit donc pas une interface utilisable. La sortie release a été restaurée, avec reconstruction en cache observée à 3,8 s. Après optimisation légère et noms encodés, le build dev séparé réussit : **20 412 294 octets**, 2 min 27 s pour la compilation UI puis **4,16 s** pour une relance complète sans changement. Ces observations ne constituent pas une comparaison à conditions égales et ne mesurent pas une reconstruction après édition. Aucun scénario visuel local n'a été exécuté ; la première compilation peut rester longue.

La lecture du WASM intermédiaire a identifié une section de noms de 55 120 756 octets pour 17 990 012 octets de code, avant l’expansion observée après wasm-bindgen. Le lien Rust d’`index.html` porte désormais `data-no-demangle`, option documentée par [Trunk 0.22.0](https://github.com/trunk-rs/trunk/blob/a3026f888c4beeca8cb3c378ac87f2c9f19b1521/guide/src/assets/index.md) et transmise à `wasm-bindgen --no-demangle` par son [pipeline de cette version](https://github.com/trunk-rs/trunk/blob/a3026f888c4beeca8cb3c378ac87f2c9f19b1521/src/pipelines/rust/wasm_bindgen.rs). Elle conserve les noms Rust encodés sans demander leur expansion lisible. Les mesures du profil ajusté ci-dessus incluent également `opt-level=1` ; elles n'isolent pas l'effet de ce seul attribut. Les limites restent inchangées.

Les builds locaux sont autorisés avec les ressources normales de la machine, en profil dev comme release. Preview n’impose ni affinité CPU, ni priorité réduite, ni limite à un job Cargo. Cette autorisation ne transforme pas une compilation en preuve de tests, de Clippy ou de scénario réussi ; ces vérifications restent dans la campagne CI autorisée.

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

Les recettes DEB/RPM déclarent `xdg-utils` pour l’ouverture par le bureau, ainsi que le stockage natif des secrets, Zenity et les outils de verrouillage nécessaires. Un navigateur et un hôte de tray appartiennent au bureau installé ; XFCE/exo et Epiphany sont les outils réels de la fixture CI, pas des composants ajoutés au payload Rust. Le job de qualification installe aussi `xdg-utils` explicitement, y compris sur le runner ARM64 minimal.

Le serveur Windows conserve un mutex global entre sessions ; Setup/désinstallation réservent la maintenance et refusent toute instance active, sans arrêt forcé. Sous Linux, chaque serveur système conserve un verrou partagé ; le gestionnaire exige l’exclusivité et pose un marqueur qui empêche les nouveaux lancements. Un verrou système absent/modifié ou lié ne devient pas un accès portable implicite.

`verify-installation` contrôle bootstrap, slots et reçus sans ouvrir de données. Le retrait refuse code modifié, reçu étranger et historique nécessitant un examen. Les fichiers humains non suivis et données sont conservés. La récupération Linux exige verrou exclusif, marqueur exact et installation signée complète ; elle conserve le marqueur dans un reçu et ne répare pas silencieusement une installation partiellement effacée.

Les recettes DEB/RPM conservent leur outil et l’archive signée sous `/usr/lib/atelier-kanban-rust` ; le code actif sous `/opt/atelier-kanban-rust` appartient au cycle Rust. Avant remplacement, le gestionnaire réserve la maintenance sous verrou exclusif. Après extraction, `system-package apply` vérifie confiance, signature, cible et version réellement compilée, prépare un slot puis bascule le pointeur atomiquement. L’écran d’update système guide vers le gestionnaire ; il ne tente pas une écriture utilisateur dans `/opt`.

`system-package recover --root /opt/atelier-kanban-rust --confirm` reprend hors ligne depuis le journal de code et la confiance installée. Le journal persistant bloque le démarrage même si le marqueur manque ; une récupération du marqueur historique ne peut pas lever ce blocage. Un échec garde le code précédent vérifié ou laisse l’opération explicitement incertaine. Aucun parcours ne lit, restaure ou rétrograde la base. Le retrait déplace seulement les fichiers suivis, inchangés, dans un dossier conservé ; les fichiers humains restent en place. Les préparations et retraits conservés sont bornés à deux chacun, les slots à seize et 1 Gio : la limite refuse l’opération sans effacement automatique. Un contenu humain modifié, une identité de dossier différente ou un effet ambigu impose une intervention explicite.

Sous Windows, Inno retire les fichiers et inscriptions qu’il suit. Le scénario Setup applique d’abord l’archive successeur existante, vérifie version réellement servie, conversation et historique précédent, arrête puis désinstalle. Les slots/reçus créés ensuite par l’updater Rust restent inactifs après désinstallation ; aucune suppression récursive de ces fichiers ni du contenu humain n’est promise. Ces mécanismes et scénarios restent des candidats non exécutés.

## Lanceur, tray et transfert

`launch/stop/serve` vérifient l’identité loopback/session/données ; l’ouverture ne relance pas une instance disparue et le tray est lié au PID de son serveur. Le jeton utilisé pour vérifier l’instance est conservé pour l’action, sans relecture vers une session remplacée. L’arrêt doit aussi attendre la libération du verrou de données avant de confirmer un redémarrage sûr.

Le serveur natif possède le helper tray du même exécutable : thread principal AppKit, boucle Windows, backend StatusNotifier Linux. Menus Ouvrir/Quitter bilingues, actions sérialisées, refus visible et retrait sur EOF du propriétaire. Aucun Node/Python/.NET ni jeton en arguments/environnement. Linux sans bureau/D-Bus n’ouvre pas de tray ; une création refusée laisse l’application accessible dans le navigateur. Windows/macOS exigent un rectangle réel avant READY ; le scénario Linux observe l’Id/pixels/état de l’item produit dans l’hôte D-Bus puis son retrait. Clics et focus restent à qualifier.

Un second scénario Linux démarre un vrai serveur synthétique puis utilise les interfaces publiques StatusNotifier/DBusMenu : sélection des actions bilingues par leur label, événement externe `clicked` pour Ouvrir, fichier de session navigateur préparé et retour à Prêt, puis Quitter avec serveur HTTP arrêté, verrou libéré et item retiré. Il n’appelle aucune fonction interne d’action. Raccordé à `native-tray --ignored`, il échouait à Ouvrir sur `40470039650` ; les corrections suivantes ont réellement réussi ce parcours sur Linux x64 et ARM64, notamment dans `40481745917` sur `654f066`. Les résultats et commits suivants restent détaillés dans le relevé de qualification.

Le helper tray conserve les variables non secrètes de session bureau et les répertoires XDG configurés par son propriétaire. Le scénario Linux utilise un véritable hôte XFCE et `exo-open`, avec association HTML/http/https vérifiée dans des répertoires XDG jetables ; aucune substitution par `BROWSER` ni faux navigateur. La CI réutilise Chrome lorsque son entrée desktop système est présente et installe Epiphany sinon, pour éviter les téléchargements inutiles du runner x64. Ce choix de fixture attend sa propre requalification. Le [source officiel xdg-open 1.1.3](https://gitlab.freedesktop.org/xdg/xdg-utils/-/blob/v1.1.3/scripts/xdg-open.in) montre que son mode générique attend directement le navigateur, tandis que le mode XFCE délègue à l’activation native. Atelier exige toujours un retour réussi de l’ouvreur sous 15 s ; une expiration reste une erreur, sans tuer le navigateur externe. Le rendu WASM relève du parcours WebDriver distinct ; aucun succès n’est déduit d’un simple `spawn`.

**Preuve bureau macOS encore manquante :** dans une session jetable, lancer le paquet de la cible avec un dossier de données synthétique explicite, cliquer l’icône puis Ouvrir, constater le fichier de session navigateur et le retour du menu à Prêt, puis cliquer Quitter. Consigner cible/commit/version, fermeture du serveur HTTP, acquisition du verrou de données et disparition de l’icône, sans publier de jeton. Le scénario AppKit existant ne prouve que présence/retrait sur EOF ; il ne remplace pas ces interactions ni leur résultat. Le rendu WASM relève des parcours WebDriver distincts.

Le transfert Rust portable explicite réutilise `import-data` et son choix de source confirmé : source verrouillée, format ATKR contrôlé, copie privée bornée puis publication, aucun ancien format Node. Session/verrous/autorisation locale de dépôts ne sont pas transférés ; chiffrement intercompte non promis. Les sauvegardes sont conservées.

## Campagne finale préparée, non lancée

```text
cargo xtask qualify-packages .artifacts/rust-delivery
cargo xtask test-updates --prepared-delivery .artifacts/rust-delivery
cargo test -p atelier-app --test native-tray --locked -- --ignored
cargo test -p atelier-app --test native-import --locked -- --ignored
cargo test -p atelier-app --test synthetic-tls --locked -- --ignored
cargo test -p xtask --test forge-review-tls --locked -- --ignored
```

Le workflow reste exclusivement manuel. Par défaut, `scope=browser`, `browser=chrome` et `journey=jira` ciblent un seul parcours ; `native` et `delivery` ciblent la plateforme choisie (`windows` par défaut). `scope=full, platform=all` sélectionne explicitement les cinq cibles et les quatre navigateurs. Le raccourci `full_matrix` vaut désormais `false` par défaut. Une sélection partielle ne satisfait jamais la checklist globale.

Les assets WASM candidats sont construits une seule fois et contrôlés avant utilisation. Livraison et navigateurs les réutilisent indépendamment : un échec navigateur ne masque plus les résultats des installeurs. Les étapes DEB, RPM et mise à jour restent indépendantes après fabrication réussie ; leurs échecs ne sont pas neutralisés. Les caches de compilation sont sauvegardés même après un échec de scénario. Les artefacts restent des candidats de préproduction, sans publication. Voir [les résultats effectifs](rust-final-qualification.md) : les vrais installeurs restent sans preuve.

`qualify-packages` construit le vrai binaire release et un successeur, génère avis/ZIP/signatures synthétiques, installe puis exécute le lanceur portable : version observée, destination existante refusée, conversation conservée au redémarrage, session renouvelée et arrêt. Il mesure les durées effectives de démarrage, redémarrage, arrêts et relecture d’état ainsi que la taille réelle de l’archive. Ce sont des observations du runner sans baseline ni gain de vitesse revendiqué. Il produit les formats natifs et un rapport des seules étapes achevées ; installation native et update/rollback restent explicitement en attente de leurs étapes distinctes.

Les scénarios natifs distincts prévoient Setup/réinstallation/refus d’un serveur actif ou code humain sous Windows ; DEB/RPM via vrais gestionnaires dans conteneurs Ubuntu/Fedora jetables de l’architecture native, utilisateur synthétique distinct, retrait et upgrade actifs refusés, confiance étrangère et version compilée incorrecte refusées, récupération hors ligne, upgrade réel puis conversation relue, code modifié et données/fichiers humains conservés. Les logs/JSON effectifs et archives restent des artifacts CI de préproduction, jamais une release. La clé privée et les données de fixture n’y sont pas incluses.

Les trois étapes natives explicites `test-updates --prepared-delivery` vérifient et réutilisent les deux archives signées déjà fabriquées. Elles conservent les assertions de changement de version réellement exécutée, session et conversation, puis de rollback après version compilée incorrecte. Leur rapport n’est écrit qu’après réussite effective ; la commande sans option conserve le parcours autonome avec compilation. Les tests de desktop explicitement ignorés ne seront des preuves qu’après invocation réelle et lecture de leurs résultats. `native-import` concerne Windows ; les interactions POSIX du sélecteur restent à qualifier.

Les parcours TLS natifs utilisent des CA et serveurs HTTPS temporaires synthétiques. Sur macOS, l’outillage de test compile un Git épinglé et vérifie son lien avec le curl Homebrew OpenSSL ; son chemin précède le Git système et reste transmis aux sous-processus isolés. Cet outil de qualification reste hors payload. Aucun repli de confiance vers le trousseau personnel n’est prévu.

Linux utilise des sessions D-Bus/XDG et keyrings jetables. macOS crée un trousseau temporaire dédié/marqué et transmet son chemin sous garde CI/finale/jetable : lectures, écritures et suppressions de secrets lui sont explicitement liées, sans recherche dans le trousseau par défaut. Les probes de comptes/trousseaux déjà explicitement ciblés conservent leur scope. Les mots de passe/clés synthétiques restent hors arguments et journaux. Le backend refuse en CI un accès implicite au trousseau macOS par défaut.

État exact : implémentation et scénarios écrits, formatage/lecture seulement ; **aucune compilation, exécution, réussite ou livraison de paquets qualifiée**. Les vrais résultats, artefacts, erreurs/reprises et limites devront être relus avant de cocher R06/R07.

## Recherche et installation depuis GitHub

Le panneau des mises à jour interroge la dernière release publique stable de `FluryWaana/atelier` via `GET /repos/FluryWaana/atelier/releases/latest`. Les tags acceptés sont `VERSION` et `vVERSION`, avec trois nombres. Les versions identiques ou inférieures ne sont pas proposées. Contrat GitHub : https://docs.github.com/en/rest/releases/releases#get-the-latest-release.

Chaque release doit contenir `atelier-VERSION-TARGET.zip` et `atelier-VERSION-TARGET.zip.signed.json`. Le second fichier est le manifeste signé produit par `sign-package`, avec la clé déjà approuvée par l'installation. Les deux assets doivent appartenir à cette release et à ce dépôt. Aucun jeton GitHub ni nouvelle clé n'est demandé dans l'interface. Les redirections de téléchargement sont limitées aux hôtes HTTPS GitHub autorisés, sans identifiants dans l'URL. Réponses, tailles et durée sont bornées. Une release sans ces assets pour la plateforme est affichée comme incompatible. Les clés disponibles restent synthétiques. Ce changement ne constitue pas une publication ni une qualification d'une clé de release.

Un clic lance téléchargement, authentification, préparation et activation. Le serveur relit la release et refuse une version différente de celle proposée. La signature est vérifiée avant le téléchargement de l'archive. La taille et les empreintes sont ensuite vérifiées par le préparateur existant. La progression expose les octets réellement reçus et le total signé, puis la vérification reste distincte. Une seule installation peut télécharger à la fois. Les brouillons bloquent le redémarrage. Les états et la récupération existants restent disponibles sans cases de confirmation supplémentaires. Le serveur lancé depuis les sources peut consulter les releases, mais seul un paquet Rust installé avec sa clé de confiance peut se mettre à jour. Les installations système Linux restent gérées par le gestionnaire de paquets.

Avant l'activation, le worker arrête proprement le serveur et verrouille le dossier de données. La base existante doit passer `PRAGMA quick_check`, les effets externes doivent être résolus et une sauvegarde SQLite atomique doit réussir dans le catalogue local habituel. Son nom est conservé dans le journal de mise à jour. Un échec interdit le changement de version. Une base absente n'est pas créée par cette étape. Le retour au binaire précédent conserve la base et les fichiers humains, sans restauration automatique susceptible d'effacer un travail récent. Le format SQLite actuel refuse les schémas non pris en charge et n'effectue aucune migration implicite. Toute future migration de schéma devra qualifier séparément sa réversibilité. La sauvegarde conservée ne rend pas cette qualification facultative.

La validation locale utilise uniquement des bases, clés, paquets et serveurs HTTP synthétiques. Les tests couvrent version numérique, release absente ou invalide, limites HTTP, transfert tronqué, provenance des assets, concurrence, progression et nettoyage, signature et clé étrangères, archive altérée, conservation des tables et fichiers voisins, échec de sauvegarde et récupération aux frontières du remplacement. Les scénarios WebDriver ont été adaptés aux nouveaux contrôles mais ne sont pas exécutés localement. Aucune CI, publication ou donnée utilisateur n'est utilisée.

Vérifications du 30 octobre 2026, après autorisation explicite des tests locaux : `cargo test -p atelier-app --lib infrastructure::update --locked` réussit avec 16 tests. `cargo test -p atelier-app --lib backup --locked` réussit avec 10 tests, dont les deux tests de sauvegarde avant mise à jour également présents dans le premier lot. `cargo test -p atelier-app --lib http::updates --locked` réussit avec 2 tests. Cela représente 26 tests distincts. Les premiers essais sous bac à sable avaient échoué sur les sockets loopback et les publications de temporaires Windows. Les relances avec ces accès autorisés passent. `cargo check -p atelier-ui --target wasm32-unknown-unknown --locked` et `cargo check -p xtask --locked` réussissent. Le cache incrémental signale un repli de liens physiques vers des copies, sans erreur de compilation. Clippy, parcours navigateur et qualification d'une release réelle restent non exécutés. Aucun résultat R06/R07 global n'est déclaré acquis.
