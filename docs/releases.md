# Publier et mettre à jour Atelier Kanban

## Première publication

Le parcours actuel utilise un **dépôt GitHub public**, sans jeton GitHub sur les postes utilisateurs. Le dépôt peut être créé et rempli avec ces sources ; `data/`, les journaux, `.updates/`, le verrou, `node_modules/`, `dist/` et le binaire Node sont ignorés par Git. Vérifier les fichiers avant le premier commit. Choisir la licence du projet avant sa diffusion publique.

Le workflow `.github/workflows/release.yml` utilise automatiquement le dépôt qui l’exécute (`GITHUB_REPOSITORY`). Il n’y a donc aucune URL à saisir par les utilisateurs. Pour un dépôt source privé, il faudra un dépôt de distribution public séparé ou une stratégie d’authentification spécifique ; cette version ne distribue pas de jeton secret dans le package.

1. Activer GitHub Actions sur le dépôt.
2. Mettre à jour la version dans `package.json` et `package-lock.json`, par exemple avec `npm version patch --no-git-tag-version`.
3. Commiter les sources, puis créer et pousser le tag correspondant, par exemple `v0.1.1`.
4. GitHub Actions installe les outils de développement, compile les styles, exécute les tests, fabrique les cinq paquets et vérifie leur démarrage ainsi que le retour arrière après échec sur chaque système. La publication n’a lieu que si toute la matrice réussit.
5. La release stable contient les paquets `windows-x64.zip`, `linux-x64.tar.gz`, `linux-arm64.tar.gz`, `macos-x64.tar.gz` et `macos-arm64.tar.gz`, préfixés par `atelier-kanban-X.Y.Z-`, chacun avec son fichier `.sha256`. Les notes sont générées à partir de l’historique GitHub ; les enrichir pour les utilisateurs si nécessaire.

Les pull requests, les pushes sur `main` et le lancement manuel produisent des artefacts de test sans publier de release. Seul un tag publie. Node.js est fixé dans la pipeline ; maintenir cette version et republier un package pour livrer ses correctifs aux utilisateurs. Les exécutions ARM64 utilisent les runners natifs GitHub. Windows ARM64 n’est pas proposé dans cette matrice.

Pour construire localement depuis les sources avec le runtime Node.js natif et son fichier `LICENSE` (ou `NODE_LICENSE` pointant vers celui-ci) :

```powershell
npm ci
npm run build:css
npm test
npm run package -- proprietaire/depot
npm run test:release
```

Le paquet contient uniquement une liste explicite de fichiers applicatifs. Il ne contient aucune base personnelle. Un paquet local existant de même version n’est pas écrasé. Linux/macOS utilisent `tar --format=ustar` lors de la construction ; l’installation valide et extrait ce format restreint avec Node, sans lancer un extracteur externe. La licence Node est incluse dans `runtime/`. Ne pas distribuer un paquet de test construit avec un dépôt fictif.

## Changement des noms de lanceurs

Les lanceurs portables sont désormais `Start.cmd` / `Stop.cmd` sous Windows et `Start.command` / `Stop.command` sous Linux/macOS. Les anciennes versions qui utilisent les noms français refusent ces nouvelles entrées dans leur validation d’archive : cette transition nécessite une installation manuelle. Arrêter l’ancienne application, sauvegarder ses données, puis extraire la nouvelle archive dans un dossier distinct et y transférer les données avant le premier lancement. Mettre à jour les raccourcis personnels.

## Parcours utilisateur

La première installation nécessite d’extraire le paquet de son système et d’utiliser `Start.cmd` sous Windows ou `Start.command` sous Linux/macOS. La commande `sh Start.command` peut aussi être utilisée depuis le dossier extrait. Une ancienne version dépourvue du mécanisme de mise à jour doit recevoir cette première installation manuellement, application arrêtée et données sauvegardées.

L’interface recherche une version stable au chargement puis toutes les six heures tant que l’onglet est visible. Le bouton **Mises à jour** déclenche une recherche manuelle. Les recherches sont limitées à une par minute côté serveur. Une panne réseau n’empêche pas l’usage local.

La notification flotte en bas à droite, au-dessus du contenu, et donne accès aux nouveautés et à **Mettre à jour**. Elle peut être fermée puis retrouvée avec **Mises à jour** ; une nouvelle version la fait réapparaître. L’utilisateur confirme le redémarrage. Le serveur refuse la mise à jour pendant un agent, une file active, un serveur Front/Back, un chronomètre, un envoi incertain ou une requête applicative en cours. Durant l’installation, les nouvelles opérations sont bloquées et la notification reste visible. Recharger la page reprend le suivi de l’installation en cours.

Le paquet et son empreinte doivent appartenir au tag annoncé dans le dépôt configuré. Chaque étape du téléchargement utilise HTTPS ; les redirections sont contrôlées avant la requête, limitées à cinq et aux hôtes `github.com`, `release-assets.githubusercontent.com` et `objects.githubusercontent.com`. La durée totale et le volume téléchargé sont bornés. Son SHA256 est comparé à celui publié. L’archive est contrôlée avant extraction : pas de données personnelles, chemins sortants, noms Windows ambigus, liens symboliques ou volume excessif. Le dépôt, la version et le format des données sont vérifiés.

Une sauvegarde SQLite complète est créée avant l’arrêt. Un processus indépendant, utilisant une copie de Node, remplace uniquement les entrées applicatives. `data/` reste à sa place. Le processus relance le serveur et vérifie son identité, sa version et sa réponse HTTP. En cas d’échec, il restaure les fichiers précédents et la sauvegarde SQLite, puis relance la version précédente.

## Exploitation et limites

- L’installation automatique nécessite le runtime embarqué du système et de l’architecture courants, ainsi que le dossier `data/` standard. Le manifeste refuse un paquet d’un autre système ou d’une autre architecture. Une exécution depuis les sources peut consulter les nouveautés, mais doit utiliser le package pour l’installation automatique.
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

Les tests couvrent la comparaison des versions, les réponses GitHub, les empreintes, les états occupés, la préparation, le verrouillage API, les archives malveillantes et la restauration des fichiers. `test:release` extrait le vrai paquet dans un dossier temporaire, effectue un remplacement et vérifie la conservation d’une carte. Un second scénario force un échec au démarrage et vérifie le retour à la version précédente. La CI impose `ATELIER_REQUIRE_RELEASE=1` pour empêcher qu’un paquet absent transforme ces contrôles en tests ignorés.

Les cinq jobs de release doivent réellement passer dans GitHub Actions avant d’annoncer le bouton comme validé sur chaque plateforme. Vérifier aussi l’installation sur des postes avec les outils de développement absents et le fonctionnement réel de leur trousseau. Le coffre POSIX est testé ici avec des doubles des commandes système ; cela ne valide pas un trousseau interactif réel.
