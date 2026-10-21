# Publier et mettre à jour Atelier Kanban

## Première publication

Le parcours actuel utilise un **dépôt GitHub public**, sans jeton GitHub sur les postes utilisateurs. Le dépôt peut être créé et rempli avec ces sources ; `data/`, les journaux, `.updates/`, le verrou, `node_modules/`, `dist/` et le binaire Node sont ignorés par Git. Vérifier les fichiers avant le premier commit. Choisir la licence du projet avant sa diffusion publique.

Le workflow `.github/workflows/release.yml` utilise automatiquement le dépôt qui l’exécute (`GITHUB_REPOSITORY`). Il n’y a donc aucune URL à saisir par les utilisateurs. Pour un dépôt source privé, il faudra un dépôt de distribution public séparé ou une stratégie d’authentification spécifique ; cette version ne distribue pas de jeton secret dans le package.

1. Activer GitHub Actions sur le dépôt.
2. Utiliser la version Node.js de `.node-version`, puis mettre à jour la version dans `package.json` et `package-lock.json`, par exemple avec `npm version patch --no-git-tag-version`. Exécuter `npm run check:release` pour vérifier leur cohérence.
3. Commiter les sources, puis créer et pousser le tag correspondant, par exemple `v0.1.1`.
4. GitHub Actions contrôle le tag, les versions et l’architecture native de chaque job, installe les outils de développement, compile les styles, exécute les tests, fabrique les cinq paquets et vérifie leur démarrage ainsi que le retour arrière après échec sur chaque système. La publication n’a lieu que si toute la matrice réussit.
5. Avant publication, le job final exige les cinq archives portables, les cinq installateurs et leurs dix empreintes SHA256 valides. Il crée une release brouillon, envoie tous les fichiers, puis la rend publique seulement si l’envoi réussit. La release contient les archives `windows-x64.zip`, `linux-x64.tar.gz`, `linux-arm64.tar.gz`, `macos-x64.tar.gz`, `macos-arm64.tar.gz`, l’installateur `windows-x64-setup.exe` et les paquets `linux-x64.deb`, `linux-x64.rpm`, `linux-arm64.deb`, `linux-arm64.rpm`, préfixés par `atelier-kanban-X.Y.Z-`, chacun avec son fichier `.sha256`. Les notes sont générées à partir de l’historique GitHub ; les enrichir pour les utilisateurs si nécessaire.

Les pull requests, les pushes sur `main` et le lancement manuel produisent des artefacts de test sans publier de release, même si le lancement manuel cible un tag. Seul le push d’un tag publie. Les exécutions sur une même référence sont sérialisées pour éviter deux publications concurrentes. Node.js est fixé dans `.node-version`, utilisé par tous les jobs ; la licence téléchargée correspond au runtime installé. Maintenir cette version et republier un package pour livrer ses correctifs aux utilisateurs. Les exécutions ARM64 utilisent les runners natifs GitHub. Windows ARM64 n’est pas proposé dans cette matrice.

Exemple pour une nouvelle version corrective, depuis une branche `main` propre et à jour :

```sh
npm version patch --no-git-tag-version
npm run check:release
git add package.json package-lock.json
git commit -m "Prepare release X.Y.Z"
git tag -a vX.Y.Z -m "Atelier Kanban X.Y.Z"
git push origin main
git push origin vX.Y.Z
```

Remplacer `X.Y.Z` par la version obtenue. Attendre la réussite du workflow du tag et vérifier les vingt fichiers sur la release. Ne pas déplacer un tag déjà publié ni remplacer ses archives : corriger dans une nouvelle version.

Si la construction ou les tests échouent, aucune release n’est créée. Si l’envoi échoue, la release reste en brouillon et n’est pas proposée par le mécanisme de mise à jour. Supprimer uniquement ce brouillon dans GitHub, en conservant son tag, puis utiliser **Re-run failed jobs** sur l’exécution d’origine. Une release déjà publique n’est jamais écrasée par ce workflow ; ne pas la supprimer pour rejouer une publication.

Pour construire localement depuis les sources avec le runtime Node.js natif et son fichier `LICENSE` (ou `NODE_LICENSE` pointant vers celui-ci) :

```powershell
npm ci
npm run check:release
npm run build:css
npm test
npm run package -- proprietaire/depot
npm run test:release
```

Le paquet contient uniquement une liste explicite de fichiers applicatifs. Il ne contient aucune base personnelle. Un paquet local existant de même version n’est pas écrasé. Linux/macOS utilisent `tar --format=ustar` lors de la construction ; l’installation valide et extrait ce format restreint avec Node, sans lancer un extracteur externe. La licence Node est incluse dans `runtime/`. Ne pas distribuer un paquet de test construit avec un dépôt fictif.

Après le paquet portable, `npm run package:installers` produit les installateurs natifs du système courant, avec leurs empreintes. Windows utilise Inno Setup 6 (`ISCC` permet d’indiquer le chemin de `ISCC.exe`) et le compilateur C# du .NET Framework Windows. Linux nécessite `dpkg-deb`, `rpm`/`rpmbuild` et `desktop-file-validate`. Aucun de ces outils n’est requis sur le poste utilisateur. Les stages de construction sont créés dans le répertoire temporaire système. Les installateurs existants ne sont pas écrasés.

`ATELIER_REQUIRE_INSTALLERS=1 npm run test:installers` teste les paquets réellement construits. Sous PowerShell : `$env:ATELIER_REQUIRE_INSTALLERS = '1'; npm run test:installers`. Le test Windows installe dans un dossier temporaire, vérifie la réinstallation et la désinstallation, et refuse de s’exécuter si le compte possède déjà une installation Atelier. Utiliser un compte de test. La CI Linux teste aussi installation, refus de remplacement pendant l’exécution, réinstallation et désinstallation dans des conteneurs Ubuntu et Fedora natifs. Les tests ordinaires n’installent aucun paquet système.

## Installation Windows et Linux

Sous Windows, lancer `atelier-kanban-X.Y.Z-windows-x64-setup.exe`, puis **Atelier Kanban** depuis le menu Démarrer. L’installation se fait dans `%LOCALAPPDATA%\Programs\AtelierKanban`, sans élévation. Un petit lanceur ouvre le navigateur sans fenêtre de terminal et affiche les erreurs dans une boîte de dialogue. Les raccourcis permettent aussi l’arrêt et l’import d’une copie portable. L’installation et la désinstallation demandent l’arrêt des opérations actives avant de remplacer les fichiers.

Sous Ubuntu/Debian, ouvrir le `.deb` avec l’installateur de logiciels, ou utiliser `sudo apt install ./atelier-kanban-X.Y.Z-linux-ARCH.deb`. Sous Fedora, utiliser le `.rpm` ou `sudo dnf install ./atelier-kanban-X.Y.Z-linux-ARCH.rpm`. `ARCH` vaut `x64` ou `arm64`. Les paquets placent l’application dans `/opt/atelier-kanban`, ajoutent la commande `atelier-kanban`, son icône et une entrée dans le menu des applications. Les dépendances `secret-tool`, `xdg-open`, `lsof` et Zenity sont installées par le gestionnaire de paquets. Un trousseau Secret Service déverrouillé reste nécessaire pour les jetons.

Les données, sauvegardes et journaux des installations natives sont conservés dans `%LOCALAPPDATA%\AtelierKanban\data` sous Windows et `$XDG_DATA_HOME/atelier-kanban` sous Linux (par défaut `~/.local/share/atelier-kanban`). La désinstallation ne les efface pas. Le fichier `installation.json`, propre à l’installation, distingue ce mode des archives portables ; ne pas le supprimer ou le copier dans une version portable.

La version Windows installée conserve le mécanisme de mise à jour intégrée, y compris le retour arrière sur la base externe. Les paquets Linux restent sous le contrôle d’APT/DNF : Atelier signale une nouvelle version et ouvre les nouveautés, mais n’écrit pas dans `/opt`. Télécharger et installer le nouveau `.deb`/`.rpm` après avoir arrêté Atelier dans toutes les sessions. Les scripts du paquet refusent le remplacement ou le retrait si son runtime est encore utilisé. Aucun dépôt APT/DNF n’est configuré automatiquement.

macOS conserve uniquement ses archives portables. Aucun bundle `.app`, Developer ID ou service de notarisation n’est ajouté.

### Importer une copie portable

1. Arrêter l’ancienne copie et conserver son dossier complet en sauvegarde.
2. Installer Atelier. Avant le premier lancement normal, choisir **Importer les données portables** dans le menu Démarrer Windows, ou exécuter `atelier-kanban --import-data /chemin/ancienne-copie/data` sous Linux. Sans chemin, le sélecteur de dossiers s’ouvre.
3. Sélectionner le dossier `data` de l’ancienne copie. L’import fait une copie des fichiers et un instantané SQLite cohérent, crée une sauvegarde dans le nouveau dossier, puis lance Atelier. L’ancienne copie n’est pas modifiée.

Un dossier de destination contenant déjà des données est refusé : l’import ne fusionne pas deux tableaux. Les liens symboliques et les bases invalides sont refusés. Les jetons restent chiffrés et nécessitent le même compte Windows ou le même trousseau Linux. Les chemins des projets ne sont pas déplacés. Les modules ajoutés manuellement dans `connectors/` ne font pas partie du dossier `data` et doivent être réinstallés séparément.

## Changement des noms de lanceurs

Les lanceurs portables sont désormais `Start.cmd` / `Stop.cmd` sous Windows et `Start.command` / `Stop.command` sous Linux/macOS. Les anciennes versions qui utilisent les noms français refusent ces nouvelles entrées dans leur validation d’archive : cette transition nécessite une installation manuelle. Arrêter l’ancienne application, sauvegarder ses données, puis extraire la nouvelle archive dans un dossier distinct et y transférer les données avant le premier lancement. Mettre à jour les raccourcis personnels.

## Parcours utilisateur

Pour une copie portable, extraire l’archive de son système et utiliser `Start.cmd` sous Windows ou `Start.command` sous Linux/macOS. La commande `sh Start.command` peut aussi être utilisée depuis le dossier extrait. Une ancienne version dépourvue du mécanisme de mise à jour doit recevoir cette première installation manuellement, application arrêtée et données sauvegardées.

L’interface recherche une version stable au chargement puis toutes les six heures tant que l’onglet est visible. Le bouton **Mises à jour** déclenche une recherche manuelle. Les recherches sont limitées à une par minute côté serveur. Une panne réseau n’empêche pas l’usage local.

La notification flotte en bas à droite, au-dessus du contenu, et donne accès aux nouveautés et à **Mettre à jour**. Elle peut être fermée puis retrouvée avec **Mises à jour** ; une nouvelle version la fait réapparaître. L’utilisateur confirme le redémarrage. Le serveur refuse la mise à jour pendant un agent, une file active, un serveur Front/Back, un chronomètre, un envoi incertain ou une requête applicative en cours. Durant l’installation, les nouvelles opérations sont bloquées et la notification reste visible. Recharger la page reprend le suivi de l’installation en cours.

Le paquet et son empreinte doivent appartenir au tag annoncé dans le dépôt configuré. Chaque étape du téléchargement utilise HTTPS ; les redirections sont contrôlées avant la requête, limitées à cinq et aux hôtes `github.com`, `release-assets.githubusercontent.com` et `objects.githubusercontent.com`. La durée totale et le volume téléchargé sont bornés. Son SHA256 est comparé à celui publié. L’archive est contrôlée avant extraction : pas de données personnelles, chemins sortants, noms Windows ambigus, liens symboliques ou volume excessif. Le dépôt, la version et le format des données sont vérifiés.

Pour les mises à jour intégrées, une sauvegarde SQLite complète est créée avant l’arrêt. Un processus indépendant, utilisant une copie de Node, remplace uniquement les entrées applicatives. Le dossier de données reste à sa place. Le processus relance le serveur et vérifie son identité, sa version et sa réponse HTTP. En cas d’échec, il restaure les fichiers précédents et la sauvegarde SQLite, puis relance la version précédente.

## Exploitation et limites

- L’installation automatique nécessite le runtime embarqué du système et de l’architecture courants, ainsi que le dossier de données prévu par le mode portable ou Windows installé. Le manifeste refuse un paquet d’un autre système ou d’une autre architecture. Les installations Linux utilisent APT/DNF. Une exécution depuis les sources peut consulter les nouveautés, mais doit utiliser le package pour l’installation automatique.
- Le dossier applicatif doit être accessible en écriture. Prévoir de l’espace pour le téléchargement, l’extraction, la version précédente et la sauvegarde.
- Les fichiers de travail, anciens fichiers et logs restent dans `.updates/` pour diagnostic. Après une mise à jour confirmée, application arrêtée, les anciens sous-dossiers peuvent être supprimés manuellement. Ne pas supprimer le dossier référencé par un verrou actif.
- Le résultat est enregistré dans `data/update-result.json`. Le journal du programme d’installation se trouve dans `.updates/update-…/update.log`.
- Après une interruption, le lanceur tente de récupérer une transaction dont les processus ne tournent plus. S’il reste un serveur issu de la tentative, l’arrêter avant de reprendre. Une panne disque ou un verrouillage antivirus peut nécessiter une restauration manuelle depuis `.updates/…/previous` et `data/backups`.
- Le SHA256 détecte les téléchargements corrompus ; la confiance dans le code repose sur le dépôt GitHub et ses droits de publication. Protéger les comptes et les tags de release. Le package n’est pas signé Authenticode.
- Les sauvegardes SQLite contiennent le coffre chiffré : DPAPI sous Windows, AES-256-GCM avec une clé dans le trousseau système sous Linux/macOS. Le compte et son trousseau restent nécessaires pour déchiffrer les jetons ; une copie sur un autre système impose de les ressaisir. Ne pas publier ces sauvegardes. Elles ne remplacent pas une sauvegarde hors du disque.
- Sous Linux, installer `secret-tool` et déverrouiller un trousseau compatible Secret Service pour enregistrer les jetons. Sans trousseau, les usages locaux sans jeton restent disponibles. `lsof`, `xdg-open` et Zenity servent respectivement à l’inspection des ports, à l’ouverture du navigateur et au sélecteur de dossiers. Les intégrations CLI demandent leur propre installation et authentification sur chaque système.
- Les paquets ne sont pas signés par le projet avec Authenticode ou Developer ID. macOS peut demander une autorisation d’ouverture ; ne pas désactiver globalement Gatekeeper.
- La compatibilité des nouvelles versions est couverte par le contrôle `dataVersion` et les tests de release. Avant de changer ce format, ajouter un mécanisme de migration et tester les sauvegardes historiques.

## Vérification avant diffusion

Les tests couvrent la comparaison des versions, les réponses GitHub, les empreintes, les états occupés, la préparation, le verrouillage API, les archives malveillantes et la restauration des fichiers. Les tests de pipeline refusent les versions désynchronisées, un mauvais tag/runtime, un paquet absent, supplémentaire, vide ou corrompu. `test:release` extrait le vrai paquet dans un dossier temporaire, contrôle son manifeste (version, système, architecture, Node et format des données), effectue un remplacement et vérifie la conservation d’une carte. Un second scénario force un échec au démarrage et vérifie le retour à la version précédente ; Windows et Linux répètent ces scénarios avec des données utilisateur hors du dossier applicatif. La CI impose `ATELIER_REQUIRE_RELEASE=1` pour empêcher qu’un paquet absent transforme ces contrôles en tests ignorés. `npm run check:release -- dist` permet de vérifier localement les vingt fichiers réunis depuis les artefacts GitHub, dans un dossier sans autre contenu.

Les cinq jobs de release doivent réellement passer dans GitHub Actions avant d’annoncer le bouton comme validé sur chaque plateforme. Vérifier aussi l’installation sur des postes avec les outils de développement absents et le fonctionnement réel de leur trousseau. Le coffre POSIX est testé ici avec des doubles des commandes système ; cela ne valide pas un trousseau interactif réel.
