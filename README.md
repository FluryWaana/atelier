# Atelier Kanban

[![Version des sources](https://img.shields.io/github/package-json/v/FluryWaana/atelier/main?label=version%20sources)](package.json)
[![Dernière version publiée](https://img.shields.io/github/v/release/FluryWaana/atelier?label=release)](https://github.com/FluryWaana/atelier/releases/latest)
[![Pipeline sur main](https://github.com/FluryWaana/atelier/actions/workflows/release.yml/badge.svg?branch=main&event=push)](https://github.com/FluryWaana/atelier/actions/workflows/release.yml?query=branch%3Amain)
[![Node.js 24 ou supérieur](https://img.shields.io/badge/Node.js-%E2%89%A5%2024-339933?logo=nodedotjs&logoColor=white)](package.json)

**Un tableau Kanban local pour organiser vos tâches et piloter Codex ou Claude Code, du ticket à la pull request.**

Atelier Kanban réunit projets, agents, branches Git, suivi du temps et connecteurs dans une interface web en français. Le serveur et les données restent sur votre ordinateur ; les intégrations utilisent vos comptes et services en ligne.

**[Télécharger](https://github.com/FluryWaana/atelier/releases/latest)** · [Historique des versions](https://github.com/FluryWaana/atelier/releases) · [Pipeline GitHub Actions](https://github.com/FluryWaana/atelier/actions/workflows/release.yml) · [Signaler un problème](https://github.com/FluryWaana/atelier/issues)

## Fonctionnalités

| Besoin | Dans Atelier Kanban |
| --- | --- |
| Organiser le travail | Kanban, projets, critères de réussite, dépendances, modèles et brainstorming |
| Travailler avec les agents | Codex ou Claude Code, journaux en direct, file d’exécution et worktree dédié par carte |
| Livrer du code | Vérifications, PR/MR brouillon, review et corrections ; parcours automatique facultatif |
| Connecter ses outils | Jira, Azure DevOps, GitHub, GitLab et Tempo via des modules configurables |
| Suivre l’activité | Temps humain, durée des agents, export CSV et bilan quotidien |
| Garder ses données | SQLite local, sauvegardes, restauration et coffre de jetons chiffré |

## Démarrage rapide

1. Téléchargez le package de votre système dans les [Releases](https://github.com/FluryWaana/atelier/releases/latest), puis extrayez-le.
2. Lancez `Demarrer.cmd` sous Windows ou `sh Demarrer.command` sous Linux/macOS depuis le dossier extrait. **Node.js est inclus.**
3. Ouvrez [Atelier Kanban sur votre ordinateur](http://127.0.0.1:4317) si le navigateur ne s’ouvre pas automatiquement.
4. Ajoutez votre dépôt dans **Mes projets**, puis créez une tâche. Pour lancer un agent, installez et authentifiez Git et le CLI choisi : Codex ou Claude Code.

Pour arrêter l’application, utilisez `Arreter.cmd` ou `sh Arreter.command`. Les connexions Jira, dépôts distants et Tempo se configurent dans **Paramètres → Modules** selon vos besoins.

## Sommaire

- [Publication et mises à jour](#publication-et-mises-à-jour)
- [Packages portables](#packages-portables)
- [Démarrage depuis les sources](#démarrage-depuis-les-sources)
- [Brainstorming et création Jira](#brainstorming-et-création-jira)
- [Préparer un ticket avec l’agent](#préparer-un-ticket-avec-lagent)
- [Développement automatique jusqu’à la PR](#développement-automatique-jusquà-la-pr)
- [Jira](#jira) et [branches et worktrees](#branches-et-worktrees)
- [Pull requests et tickets Jira](#pull-requests-et-tickets-jira)
- [Temps humain et Tempo Cloud](#temps-humain-et-tempo-cloud)
- [Utilisation rapide](#utilisation-rapide) et [sauvegardes](#sauvegardes-locales-et-restauration)
- [Pipeline et tests](#vérification), [styles](#styles--bootstrap-et-sass) et [documentation](#documentation-et-contribution)

## Publication et mises à jour

Le parcours GitHub Releases, le bouton **Mettre à jour**, la fabrication du package et les limites sont décrits dans [le guide de publication](docs/releases.md). Le dépôt de distribution est intégré automatiquement au package par GitHub Actions. Les copies exécutées depuis les sources ne proposent pas l’installation automatique.

Les tâches, projets, réglages, agents, workflows, Jira, temps et mises à jour utilisent une séparation domaine / application / adaptateurs. [Le document d’architecture](docs/architecture.md) décrit les responsabilités et les principes SOLID appliqués.

Les **Paramètres** sont une page dédiée. La rubrique **Modules** choisit les fournisseurs de tickets, de dépôts/PR et de temps. Chaque connecteur ajoute ses réglages et peut déclarer des onglets. Pour contribuer un connecteur, suivez [le guide et son exemple exécutable](docs/connectors.md) : un dossier `connectors/mon-module/index.mjs` suffit, sans modifier le cœur. Jira, Azure DevOps, GitHub, GitLab et Tempo sont les modules fournis. Pour les sessions CLI et les jetons, voir [le guide des dépôts](docs/repository-connectors.md).

## Packages portables

Les [releases stables](https://github.com/FluryWaana/atelier/releases/latest) regroupent les archives suivantes, accompagnées de leur empreinte `.sha256`. `X.Y.Z` désigne la version publiée.

| Système | Architecture | Archive |
| --- | --- | --- |
| Windows | x64 | `atelier-kanban-X.Y.Z-windows-x64.zip` |
| Linux | x64 | `atelier-kanban-X.Y.Z-linux-x64.tar.gz` |
| Linux | ARM64 | `atelier-kanban-X.Y.Z-linux-arm64.tar.gz` |
| macOS | Intel | `atelier-kanban-X.Y.Z-macos-x64.tar.gz` |
| macOS | Apple Silicon | `atelier-kanban-X.Y.Z-macos-arm64.tar.gz` |

Dans le ZIP Windows x64, Node.js est inclus : extraire le dossier puis lancer `Demarrer.cmd`. `Arreter.cmd` ferme le serveur après arrêt des agents et chronomètres. Git et les CLI utilisés restent à installer et à authentifier sur le PC cible. Le package ne contient aucune donnée personnelle.

La chaîne GitHub construit aussi les archives Linux x64/ARM64 et macOS Intel/Apple Silicon. Sur ces systèmes, extraire le `.tar.gz`, puis lancer `./Demarrer.command` (ou `sh Demarrer.command`) ; `./Arreter.command` arrête le serveur. Node.js est inclus. Sur Linux, `xdg-open` ouvre le navigateur, `lsof` permet de détecter les ports et Zenity fournit le sélecteur de dossiers ; la saisie manuelle du chemin reste possible. Pour mémoriser des jetons, `secret-tool` (paquet `libsecret-tools` sur Debian/Ubuntu) et un trousseau Secret Service déverrouillé sont nécessaires. macOS utilise le Trousseau d’accès ; Windows conserve DPAPI. Aucun repli vers des jetons en clair n’est prévu.

Pour reconstruire le paquet du système courant, exécuter `npm run package -- proprietaire/depot` avec Node.js >= 24 et sa licence à côté du runtime, ou indiquée via `NODE_LICENSE`. Le paquet et son empreinte SHA256 sont générés dans `dist/` à partir d’une liste explicite de fichiers applicatifs. `npm run package:windows` reste un alias pour les anciens scripts. La validation native des cinq paquets se fait dans GitHub Actions avant publication ; la présence des adaptateurs ne remplace pas cette validation.

## Styles : Bootstrap et Sass

Le thème utilise Bootstrap 5.3.8, compilé avec Dart Sass. `styles/_tokens.scss` définit les couleurs ; `styles/_framework.scss` sélectionne les boutons et contrôles de formulaire Bootstrap ; `styles/_atelier.scss` contient les composants de base ; `styles/_screens.scss` compose les sept vues et `styles/_dialogs.scss` les douze modales. Le point d’entrée est `styles/app.scss`.

Pour modifier les styles, installer Node.js avec npm puis exécuter :

```powershell
npm ci
npm run build:css
# Recompiler automatiquement pendant les modifications :
npm run watch:css
```

Le fichier compilé `public/style.css` est minifié et livré avec l’application portable. Il faut le régénérer après une modification SCSS. Aucune compilation, aucun CDN et aucun JavaScript Bootstrap ne sont nécessaires au lancement. Les modules de grille, carrousel, modal et les utilitaires Bootstrap inutilisés ne sont pas inclus ; les dialogues natifs conservent leur gestion du clavier.

## Démarrage depuis les sources

Installez Node.js 24 ou supérieur. Git et le CLI de l’agent choisi (Codex ou Claude Code) sont nécessaires pour travailler sur un dépôt avec un agent. Aucune dépendance npm à installer pour lancer l’application.

```powershell
git clone https://github.com/FluryWaana/atelier.git
cd atelier
node launcher.mjs
```

Le lanceur ouvre http://127.0.0.1:4317 et laisse le serveur actif en arrière-plan. Le relancer réutilise l’instance existante. `node launcher.mjs --stop` arrête l’application ; `--no-browser` évite d’ouvrir un onglet. Les logs du serveur sont dans `data/server.stdout.log` et `data/server.stderr.log`. `npm start` reste disponible pour un lancement au premier plan. Le serveur écoute uniquement sur la boucle locale. `PORT` permet de changer de port.

1. Dans **Connexions & réglages**, définir le dossier du projet.
2. Créer une tâche, choisir Codex ou Claude, puis **Analyser** ou **Modifier le code**.
3. Lire les résultats dans la carte. En mode manuel, une exécution réussie passe à **À vérifier**, jamais directement à **Terminé**. En mode automatique, ce passage attend les vérifications, la publication et la review décrites ci-dessous.

## Brainstorming et création Jira

Ouvrez **Brainstorming** dans la navigation de gauche. La page propose un formulaire dédié au sujet, au contexte, au résultat attendu et aux contraintes. Vous pouvez **Créer une tâche locale directement**, sans agent ni connexion Jira, ou **Explorer avec l’agent** pour obtenir jusqu’à huit propositions. Le projet est facultatif : sans dépôt, l’agent travaille depuis un dossier dédié au brainstorming.

Les sessions restent dans cette page, hors du Kanban. Utilisez **Reprendre un brainstorming** pour retrouver les propositions et relancer une exploration ; les saisies en cours sont aussi conservées dans ce navigateur. Pour chaque proposition retenue, choisissez **Créer une tâche locale** : elle apparaît dans le tableau et reste modifiable. Le suivi et l’arrêt de l’agent sont disponibles directement dans la page. Les brainstormings précédemment lancés depuis une carte restent accessibles dans l’historique.

Après le premier envoi, la colonne de gauche devient une discussion avec Codex ou Claude et les réglages se replient. Répondez dans la zone de message, puis utilisez la flèche ou **Entrée** pour envoyer (**Maj+Entrée** ajoute une ligne). Les échanges et décisions précédents sont transmis à l’agent, qui affine les propositions à droite. Le message en cours est conservé si l’envoi échoue. Les échanges enregistrés restent disponibles après redémarrage. Les propositions précédentes restent visibles pendant le calcul ; les tâches déjà créées restent inchangées. Chaque envoi utilise les quotas de votre agent. Un historique dépassant 200 000 caractères impose un nouveau brainstorming avec un résumé des décisions.

Relisez et modifiez le titre, la description, les critères et les questions du brouillon. Renseignez la clé du projet Jira et le nom exact du type de ticket, puis choisissez **Relire et créer dans Jira**. Après confirmation, la carte est liée au ticket créé. Ce parcours est aussi disponible sur toute carte locale. Il utilise la connexion Jira des réglages et les quotas habituels de votre agent.

Les champs personnalisés obligatoires ne sont pas pris en charge : utilisez Jira pour ces projets. Si un envoi échoue, vérifiez Jira ; rattachez la clé du ticket existant, ou confirmez son absence avant de réessayer. Aucun nouvel envoi n’est automatique.

## Préparer un ticket avec l’agent

Dans une fiche, **Préparer le ticket → Proposer des critères avec l’agent** enregistre les saisies puis lance l’agent sélectionné en lecture seule sur le dépôt (ou le worktree déjà associé). La colonne reste inchangée ; aucun worktree n’est créé. Cette préparation reste disponible même si une tâche préalable est inachevée. Elle utilise les quotas habituels de Codex ou Claude, avec le suivi, le coût estimé et l’arrêt dans le journal existant.

La proposition contient des critères observables et des questions sur les ambiguïtés du besoin. Modifiez les textes, un élément par ligne, puis choisissez **Remplacer les critères dans la fiche** : les critères sont remplacés et décochés, les questions sont ajoutées aux instructions locales. **Enregistrer la carte** conserve ce résultat. Pour Jira, le ticket distant reste inchangé. Un besoin enregistré modifié depuis la proposition impose une nouvelle préparation. Une réponse invalide reste consultable dans le journal sans modifier les critères existants.

## Développement automatique jusqu’à la PR

Dans **Mes projets → Réglages → Développement automatique**, renseignez la **branche cible de la PR** (par exemple `main`) et la **commande de vérification** (par exemple `npm test && npm run build`), puis activez le parcours. Le mode est désactivé par défaut pour chaque projet. L’activation autorise Atelier à commiter et publier tous les fichiers non ignorés du worktree de la carte. Vérifiez les règles `.gitignore` du dépôt. Le passage en **En cours** ou **Lancer l’agent** en mode **Modifier le code** déroule alors :

1. Développement par l’agent dans le worktree dédié.
2. Exécution de la commande de vérification par Atelier, sans se fier au compte rendu de l’agent.
3. Commit des modifications, push sans forcer, création ou actualisation d’une PR/MR **brouillon** chez le fournisseur de dépôt sélectionné.
4. Nouvelle exécution d’agent pour la review du code publié et du besoin, avec contrôle des commits source/cible et du worktree propre avant et après l’analyse. Chaque critère renseigné reçoit un statut et une justification.
5. Correction des P0/P1 si l’option est cochée, puis nouvelles vérifications, publication et review. Au maximum **deux cycles de correction par parcours**, même après reprise.
6. **À vérifier** et notification persistante dans la vue d’ensemble si aucun P0/P1 ne subsiste et si tous les critères renseignés sont couverts. Le lien de PR, les résultats des vérifications, les nombres de constats et le bilan des critères sont accessibles depuis la carte.

P0 = critique ; P1 = important ; P2 = modéré ; P3 = mineur. Une **suggestion** est une amélioration facultative distincte d’un défaut. Seuls les P0/P1 déclenchent la correction automatique. Après deux cycles, ou si cette correction est désactivée, les problèmes bloquants placent la carte en **Stand by**, avec le message **PR à corriger**. L’absence de constat ne garantit pas l’absence de défaut ; les critères de réussite et la fusion restent manuels. Atelier ne fusionne pas de PR et ne prend pas automatiquement le ticket suivant.

Le contrôle fonctionnel fait partie de cette même review, sans exécution d’agent supplémentaire. L’agent reçoit un instantané du titre, de la description, des instructions et des textes des critères ; dans le parcours automatique, il reçoit aussi la commande et les sorties des dernières vérifications. Chaque critère est classé **Couvert**, **Non couvert** ou **Impossible à vérifier**, avec les références au code/tests, les observations et les limites. Une suite de tests verte ou une case déjà cochée ne suffit pas à établir sa couverture. L’évaluation ne coche ni ne décoche aucune case humaine.

Un critère non couvert ou impossible à vérifier place la carte en **Stand by** avec une demande d’intervention humaine, même sans défaut technique. Ces critères ne déclenchent pas de cycle de correction supplémentaire : la correction automatique reste limitée aux P0/P1. Sans critères renseignés, la carte passe à **À vérifier** avec le message explicite **PR à contrôler : aucun critère de réussite défini** ; aucune couverture fonctionnelle n’est affirmée. Les compteurs sont visibles sur la carte et dans la notification persistante ; les justifications sont dans **Consulter la review**.

Le bilan est rattaché au besoin et aux commits analysés. Modifier le titre, la description, les instructions ou les textes/ordre des critères périme les reviews existantes ; cocher ou décocher leur validation humaine ne les périme pas. Les changements de besoin reçus lors d’une synchronisation Jira sont aussi pris en compte. Un besoin changé pendant une review empêche sa validation automatique. Un bilan manquant, dupliqué, incomplet ou sans justification est refusé. Les anciennes reviews restent consultables avec une mention indiquant l’absence de ce contrôle ; relancez-les pour obtenir un bilan fonctionnel.

Une commande absente empêche le lancement automatique. Elle s’exécute depuis la racine du worktree, avec `cmd.exe` sous Windows, et doit terminer avec un code non nul si une vérification échoue. Elle est limitée à **10 minutes** ; ses 32 000 derniers caractères de sortie sont conservés sur la carte. Les fichiers à publier doivent rester stables pendant les tests et jusqu’à la préparation de publication, sinon Atelier demande de les revérifier. Une commande qui reformate ou génère des fichiers suivis nécessitera une reprise des vérifications sur le contenu résultant. Les dépendances liées restent partagées avec la source : une installation peut les modifier pour les autres worktrees.

La PR reste brouillon ; une PR existante sortie du brouillon ou avec fusion automatique activée bloque une nouvelle publication automatique. Sa description est actualisée avec la dernière vérification, dans la limite Azure de 4 000 caractères ; le compte rendu complet reste local. La review est locale et ne publie aucun commentaire sur la forge. Le lien Jira est tenté après publication ; un échec est affiché sans perdre le lien PR et peut être réessayé séparément. Les statuts finaux utilisent la correspondance Jira du projet si elle est activée.

Les étapes et leurs erreurs sont enregistrées dans SQLite. **Reprendre l’étape** relance l’étape interrompue, sans refaire le développement quand les tests ou la review ont échoué. Une publication interrompue peut demander de repasser les vérifications avant de retrouver la PR existante. Modifier la commande du projet impose de nouvelles vérifications à la reprise ; changer la cible nécessite un nouveau parcours. Les réglages du projet sont verrouillés pendant son travail en cours.

**Arrêter le parcours** interrompt l’agent ou les vérifications et empêche les étapes suivantes. Une opération Git ou API déjà envoyée peut finir avant l’arrêt ; les fichiers et commits existants sont conservés. Après un redémarrage du serveur, le parcours est marqué interrompu et ne redémarre pas seul : vérifiez les éventuels processus restants et le worktree, puis reprenez. Une restauration de sauvegarde invalide aussi le résultat automatique. Les projets non activés et le mode **Analyser** conservent leur fonctionnement manuel.

Référence : [actualisation d’une PR Azure Repos](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1).

Le suivi est en direct par une connexion SSE locale : dernière activité sur les cartes, durée écoulée, dernier signal de l’agent et journal dans le détail. Claude transmet aussi son texte pendant la rédaction ; Codex transmet ses événements de commandes, fichiers et messages. Aucun pourcentage d’avancement n’est inventé. L’absence de nouveaux événements est affichée après 30 secondes sans déclarer l’agent bloqué. La connexion se rétablit automatiquement après une coupure et récupère les journaux manqués. Décocher **Suivre les nouveaux messages** permet de relire le journal sans défilement automatique.

Les agents utilisent leurs connexions et configurations locales existantes. Au besoin, lancer `codex login` ou `claude auth login` dans un terminal. Le tableau crée de nouvelles exécutions ; il ne prend pas le contrôle des conversations déjà ouvertes dans les applications Codex ou Claude.

## Jira

Dans les réglages, choisir Cloud (adresse HTTPS, e-mail et jeton API classique non restreint par scopes) ou Data Center (adresse HTTPS et jeton personnel). Configurer une recherche JQL puis cliquer **Synchroniser Jira**. Les jetons Cloud avec scopes et la passerelle `api.atlassian.com` ne sont pas pris en charge dans cette version.

Le jeton est enregistré dans le coffre chiffré du compte système et n’est jamais renvoyé au navigateur. Il peut aussi être fourni avec la variable d’environnement `JIRA_TOKEN`. Changer de site ou de compte invalide les secrets non ressaisis. **Oublier ce secret**, dans **Paramètres → Jira**, le supprime immédiatement. Les autres réglages sont conservés.

La synchronisation est manuelle, paginée et sans doublons. Jira reste la référence pour les titres, descriptions et statuts Jira. Les cinq colonnes sont À faire, En cours, À vérifier, Terminé et Stand by. Activez les correspondances dans les réglages pour ranger les cartes à la synchronisation et mettre Jira à jour lors d’un déplacement manuel. Chaque correspondance accepte un nom exact ou un ID de statut Jira. Une carte importée permet aussi d’appliquer explicitement une transition proposée par Jira. Retirer une carte du tableau ne supprime jamais le ticket ; une prochaine synchronisation peut la réimporter. Les cartes sorties du filtre JQL restent dans le tableau.

## Branches et worktrees

Dans **Mes projets → Réglages**, le champ **Emplacement du dépôt Git** et **Parcourir…** permettent de corriger le chemin enregistré vers un dossier existant. Cela ne déplace aucun fichier. Les cartes du projet, y compris les archives, suivent le nouveau chemin ; les exécutions passées restent historiques et leurs reviews doivent être revérifiées. Une sauvegarde locale précède la modification. Le changement est refusé tant que le projet possède des worktrees ou des opérations actives, ou si le dossier appartient déjà à un autre projet. La détection peut lire le nouvel emplacement saisi avant son enregistrement.

Choisir un dépôt Git existant avec au moins un commit. Dans les réglages, définir la référence de départ (par exemple `main`, `develop` ou `origin/main`). Par défaut, `HEAD` utilise le commit courant du dépôt source. Aucun fetch automatique n’est effectué.

Chaque carte propose une branche `feature/DEV-123-titre-du-ticket` (ou `feature/LOCAL-identifiant-titre` sans Jira). Le nom est personnalisable avant préparation ; l’ID Jira doit rester présent. Les worktrees sont créés dans un dossier `.atelier-worktrees` à côté du dépôt source. Leur chemin exact apparaît dans la carte.

- Passer une carte en **En cours**, par glisser-déposer ou en enregistrant son statut, prépare le worktree et lance l’agent avec le mode et les instructions enregistrés. Le mode initial est **Modifier le code**.
- **Préparer le worktree** permet de créer le dossier sans lancer l’agent. Le bouton de lancement manuel utilise le même mécanisme et ne provoque pas de double lancement.
- Les relances réutilisent le worktree et ses modifications. La synchronisation Jira conserve cette association.
- **Retirer le worktree** utilise Git sans forcer ; les modifications non commitées empêchent le retrait. La branche et ses commits restent conservés. Retirer le worktree est nécessaire avant de changer de dépôt/branche ou de supprimer la carte.
- Une branche déjà utilisée dans un autre worktree est refusée. Les associations incohérentes et erreurs Git bloquent le lancement ; le dépôt source n’est jamais utilisé comme solution de repli.

Avant chaque création, Atelier récupère la branche de départ sur `origin` et utilise exactement le commit récupéré. Configurez `master` dans les réglages du projet pour partir de `master` à jour ; `HEAD` suit la branche par défaut distante. Le checkout source, ses modifications et ses branches locales restent intacts. Une erreur réseau ou Git bloque la création. Les relances réutilisent le worktree sans récupération ni réinitialisation. Une branche de tâche existante libre garde ses commits, même si la base distante a avancé.

Les dossiers ignorés `node_modules` déjà installés dans la source sont partagés par liens de répertoire (jonctions sous Windows), à la racine et dans les sous-projets contenant des fichiers suivis par Git. Les anciens dossiers NuGet `packages` contenant des `.nupkg` le sont également. Le cache global NuGet est déjà commun aux worktrees ; `bin` et `obj` restent séparés. Aucun téléchargement automatique n’est effectué. Les chemins partagés sont ignorés via `.git/info/exclude`. Retirer le worktree enlève uniquement les liens ; si Git refuse le retrait, les liens sont rétablis.

**Attention : les dépendances liées sont communes à la source et aux worktrees.** Une installation, suppression ou modification dans un dossier lié affecte tous les utilisateurs de ce dossier. Si une tâche change ses dépendances, retirez son lien manuellement puis installez-les localement ; un dossier réel remplaçant un lien bloque le retrait automatique pour préserver ses fichiers. Les dépendances partagées peuvent ne plus correspondre au lockfile de la branche distante : vérifiez leur compatibilité avant de lancer le projet. Les fichiers non commités de la source ne sont pas copiés. Les agents ne font pas de commit, push ou merge à la demande du tableau.

### Lancer le front et le back

Dans **Processus → Configurer le lancement**, renseignez le dossier et la commande de chaque service. Par exemple, pour le Front : dossier `front`, commande `npm run dev -- --port 5173` ; pour le Back : dossier `src/Api`, commande `dotnet run --urls http://localhost:5080`. Un dossier vide utilise la racine du worktree ; une commande vide désactive le service. Les dossiers doivent rester dans le worktree choisi. Ces champs sont également disponibles dans **Mes projets → Réglages**. Les anciennes commandes contenant `cd` continuent de fonctionner en laissant le nouveau champ dossier vide. Sous Windows, les commandes utilisent `cmd.exe`. Configurez l’URL de l’API côté front avec les options ou variables habituelles du projet. Ne mettez pas de secrets dans ces réglages. Après modification des réglages, arrêtez puis relancez l’ensemble pour les appliquer aux services déjà lancés.

Le formulaire Bootstrap présente deux blocs **Front** et **Back**. Pour chaque application, le champ **Dépôt Git** vide désigne le dépôt principal. Sinon, choisissez son dépôt avec **Parcourir…**, puis utilisez **Détecter le Front** ou **Détecter le Back**. Le bouton **Détecter la configuration du projet** analyse les deux dépôts. La détection utilise les chemins actuellement saisis, sans enregistrer de réglages ni exécuter de script. Les résultats du Back ne sont pas proposés dans la liste du Front. Le dépôt principal peut être indifféremment celui du Front ou du Back.

Au lancement des applications, Atelier crée le worktree du dépôt associé sur **le même nom de branche que le worktree principal**. Le dépôt associé part du `HEAD` distant de son `origin` ; si la branche existe déjà et est libre, elle est réutilisée. Un worktree déjà associé à la carte est conservé aux relances. Une branche occupée dans un autre worktree n’est pas récupérée de force. Les deux applications utilisent chacune leur worktree, jamais le dépôt source associé. Les chemins sont visibles dans la carte, dans **Branches & PR** et dans les services lancés. Le retrait des worktrees d’une carte comprend ses worktrees associés : les protections Git et la confirmation de perte des modifications locales s’appliquent à tous. Un dépôt ne peut pas être remplacé dans les réglages tant que son worktree associé existe. Les agents et les PR restent ceux du dépôt principal.

La détection recherche `package.json` et les projets `.csproj` dans les trois premiers niveaux de sous-dossiers (300 dossiers au maximum), en ignorant les dépendances et les sorties de compilation. Elle reconnaît les scripts habituels de Vite, Next.js, Angular et les projets ASP.NET Core, ainsi que npm, pnpm et Yarn. Les scripts Node personnalisés restent proposés sans supposer leur manière de configurer un port. Si plusieurs applications existent, choisissez celle du Front et celle du Back. Les commandes déjà saisies sont conservées jusqu’à une sélection explicite.

Pour **.NET**, la liste inclut les projets exécutables et les bibliothèques (ces dernières ne sont pas sélectionnables). Chaque projet affiche les profils de `Properties/launchSettings.json`, avec leurs noms, leur type et leurs URL déclarées. Choisissez un profil `Project` ou **Sans profil**. IIS Express et les autres profils non exécutables par `dotnet run` sont indiqués mais désactivés. Le profil sélectionné reste chargé par .NET avec ses variables ; leurs valeurs ne sont ni retournées par la détection ni copiées dans Atelier. Pour ASP.NET Core, la surcharge `--urls` remplace les URL du profil par une URL HTTP de boucle locale. Les configurations qui imposent leurs propres endpoints Kestrel peuvent nécessiter une adaptation du projet.

Les commandes reconnues contiennent **`{port}`**. Au lancement, Atelier réserve un port TCP libre sur `127.0.0.1` et remplace ce marqueur, sans modifier les fichiers du dépôt. Vous pouvez aussi saisir un port précis dans **Port fourni par Atelier** ; s’il est occupé ou déjà attribué à un service Atelier, le lancement est refusé. `PORT` et `HOST=127.0.0.1` sont également fournis à ces processus. Plusieurs worktrees d’un même projet peuvent tourner si tous leurs services utilisent ce mécanisme. Sans `{port}`, les ports restent gérés par la commande et une seule pile par projet peut tourner. La réservation est libérée juste avant de lancer le processus : un autre programme peut encore prendre le port entre ces deux étapes ; Vite est lancé avec `--strictPort` pour échouer dans ce cas. Le port attribué est affiché séparément des ports effectivement détectés en écoute. Un redémarrage individuel conserve le port ; relancer l’ensemble permet d’en choisir de nouveaux. L’URL utilisée par le Front pour joindre le Back, ses proxys et le CORS restent à configurer dans le projet.

Références pour les surcharges : [Vite CLI](https://vite.dev/guide/cli), [Next.js CLI](https://nextjs.org/docs/app/api-reference/cli/next), [Angular CLI](https://angular.dev/cli/serve), [dotnet run](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-run).

Dans la carte, préparez le worktree puis utilisez **Démarrer Front / Back**. Les sorties et codes de fin apparaissent dans le journal **Lancer le projet**. **Arrêter Front / Back** termine les processus et leurs enfants sous Windows. Sans ports fournis par Atelier, une seule pile peut tourner par projet ; les ports entre projets différents restent à votre charge. Les commandes doivent conserver les serveurs au premier plan, sans `start` ni détachement. Le retrait du worktree est bloqué tant que sa pile tourne. `Arreter.cmd` arrête aussi ces piles ; elles ne redémarrent pas automatiquement avec Atelier. Les journaux restent en mémoire pendant la session (32 000 caractères par carte).

Pour tester deux dépôts ensemble, choisissez un **Projet lié à lancer pour les tests** dans les réglages du projet de la carte. Par exemple, renseignez uniquement la commande Front dans TOTO et la commande Back dans TOTO2, puis choisissez TOTO2 comme projet lié de TOTO. Depuis une carte TOTO, le front démarre dans le worktree de la carte et le back dans le dépôt local de TOTO2. Le dossier de chaque service apparaît dans la carte, et les journaux identifient le projet. Le dépôt lié garde sa branche et ses fichiers actuels, y compris ses modifications locales ; aucun worktree ni récupération Git n’est créé pour lui. Ses commandes de démarrage peuvent produire leurs fichiers habituels de compilation.

Les deux projets sont démarrés et arrêtés ensemble. Sans ports fournis par Atelier, un projet déjà utilisé par une autre pile ne peut pas être relancé. Un projet en cours ne peut pas être retiré de la liste avant l’arrêt de ses piles. Configurez aussi TOTO comme projet lié de TOTO2 pour tester une modification du back depuis une carte TOTO2 : le back utilisera alors son worktree et le front le dépôt local TOTO. Seul le lien direct est suivi, sans lancement récursif. L’URL de l’API et le CORS restent ceux de la configuration de vos projets.

### Onglet Processus

Le lanceur affiche une ligne par projet : choisissez son worktree, puis cliquez sur **Lancer** ou **Lancer les sélections** pour démarrer tous les projets sélectionnés dans la vue. Chaque projet lance ses commandes Front et Back dans son propre worktree. Pour deux dépôts distincts, sélectionnez un worktree sur chaque ligne ; les liens de projets ne sont pas suivis par ce lanceur. Les sélections restent disponibles pendant la session de la page. Un worktree déjà en cours est ignoré ; si un lancement échoue, les autres sont tentés et les erreurs sont affichées. Avec les ports fournis par Atelier, vous pouvez sélectionner un autre worktree du même projet puis le lancer. Les réglages sont accessibles directement sur chaque ligne.

Sous le lanceur, **Processus** regroupe les services Front / Back déjà lancés, par carte et par projet, avec leurs journaux et boutons d’arrêt. Le filtre de projet inclut les ensembles où ce projet joue le rôle de projet lié. Les agents restent suivis dans **Agents**. Le démarrage d’un ensemble depuis cet onglet lance uniquement le projet de sa carte ; le lancement historique depuis la carte conserve son éventuel projet lié.

Chaque service affiche son état, son dossier, le PID du lanceur et les PID enfants détectés, ainsi que ses adresses IP et ports TCP réellement en écoute. Sous Windows, Atelier relève les processus avec CIM et les sockets avec `netstat`, sans droits administrateur demandés. La vue s’actualise toutes les trois secondes lorsqu’elle est visible ; **Actualiser** permet un relevé manuel. Si la détection échoue, un message le signale et les commandes de lancement/arrêt restent disponibles. Aucun port configuré dans une commande n’est présenté comme écoutant sans vérification.

**Démarrer**, **Arrêter** et **Redémarrer** pilotent un service et ses enfants sans arrêter son voisin. Le redémarrage d’un service réutilise sa commande de lancement initiale ; pour appliquer de nouveaux réglages de projet, arrêtez l’ensemble puis utilisez **Démarrer l’ensemble**. Les boutons groupés arrêtent un ensemble ou tous ceux de la vue courante. Le journal de chaque ensemble est consultable directement dans cet onglet. Une carte archivée permet encore l’arrêt, mais doit être restaurée pour relancer.

**Ouvrir HTTP/HTTPS** ouvre l’adresse d’écoute. Une adresse générique `0.0.0.0` ou `::` est remplacée par l’adresse de boucle locale pour ce lien. Le protocole est HTTP par défaut, ou HTTPS si le service annonce une URL HTTPS pour ce port dans ses sorties. Un port TCP peut servir un autre protocole ; le lien navigateur n’effectue aucun contrôle HTTP. Les services doivent rester au premier plan pour que leur arbre de processus soit suivi. Seuls les processus lancés par Atelier sont pilotés ; les applications externes et les processus détachés ne sont pas repris automatiquement après un redémarrage d’Atelier.

Documentation : [Git worktree](https://git-scm.com/docs/git-worktree).

## Pull requests et tickets Jira

Depuis une carte, **Préparer la PR** charge le dépôt Azure Repos, la branche cible, un titre contenant l’ID Jira, une description avec le lien du ticket, les commits et les changements locaux. Le diff affiché est limité à 60 000 caractères (8 000 par fichier nouveau). Vérifier cet aperçu, compléter les tests réalisés, puis cliquer **Pousser et créer la PR brouillon**.

Si le worktree est modifié, cocher explicitement l’inclusion de tous les fichiers listés : Atelier crée alors un commit avec le titre choisi. Les fichiers non ignorés, y compris les nouveaux fichiers, sont inclus. Si les changements évoluent depuis l’aperçu, la publication est refusée jusqu’à son rechargement. Un échec de commit ou de push conserve le travail local.

Cette première intégration utilise **Azure DevOps Services** avec un remote `origin` HTTPS (`dev.azure.com`) ou SSH (`ssh.dev.azure.com`) et une seule destination de push. Sans PAT, l’application utilise la session Azure CLI ouverte avec `az login` et récupère un jeton temporaire pour le tenant annoncé par l’organisation Azure DevOps du dépôt, sans changer le compte CLI par défaut. Azure CLI doit être accessible dans le PATH du serveur. Un PAT enregistré dans les réglages ou fourni par `AZURE_DEVOPS_PAT` reste prioritaire ; il nécessite les droits **Code : lecture et écriture**. Le push Git utilise séparément les identifiants Git déjà configurés sur le PC (Git Credential Manager ou SSH). Azure DevOps Server sur site n’est pas encore pris en charge. Le push n’est jamais forcé. La PR est créée en brouillon ; une PR ouverte de la même branche vers la même cible est réutilisée. Les agents continuent de travailler localement ; la publication est déclenchée depuis la carte après inspection.

Le lien retourné est enregistré immédiatement sur la carte. Pour un ticket Jira, Atelier ajoute un lien distant via `remotelink` avec un identifiant stable pour éviter les doublons. Si Jira est indisponible, **Réessayer le lien Jira** reprend uniquement la liaison. Ce lien apparaît dans les liens du ticket ; l’affichage dans son panneau Développement dépend de l’intégration Azure DevOps/Jira de l’entreprise. La création de PR ne fusionne rien et ne ferme pas le ticket.

Références : [API Azure Repos](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/create?view=azure-devops-rest-7.1), [liens distants Jira](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-remote-links/).

## Exécutions et données

- SQLite : `data/kanban.sqlite`, ignoré par Git. Sauvegarder le dossier `data` après arrêt du serveur.
- Un seul agent à la fois par dossier, chemins résolus pour éviter les alias. Les modifications portent sur le worktree dédié de la carte. Plusieurs cartes peuvent travailler en parallèle dans des worktrees distincts.
- Codex : `exec --json`, approbations non interactives, sandbox `read-only` ou `workspace-write`. Les restrictions de sa configuration et de Windows restent applicables.
- Claude : `--print --output-format stream-json`, mode `plan` ou `acceptEdits`. Les demandes de permission interactives sont refusées ; certaines commandes peuvent donc rester bloquées. Aucun contournement de permissions n’est activé.
- Arrêt manuel de l’arbre de processus sous Windows ; limite de 60 minutes par exécution. L’arrêt ne restaure pas les fichiers déjà modifiés.
- Journaux limités à 5 Mo par exécution. Les journaux peuvent contenir du code et des informations du dépôt. Les résultats sont du texte, sans interprétation HTML.
- Les exécutions en cours au redémarrage sont marquées interrompues. Après un arrêt brutal du serveur, vérifier aussi les processus agents dans le Gestionnaire des tâches avant de relancer.
- L’interface, les cartes et les journaux sont locaux. Les agents et Jira communiquent avec leurs services en ligne et consomment les quotas de vos comptes.

## Vérification

Le badge **Release** en haut de cette page affiche l’état du workflow sur `main` après un push. La version des sources est lue depuis `package.json` sur `main` ; le badge **release** désigne la dernière version stable publiée. Ces versions peuvent différer pendant le développement. Avant la première publication, aucune release n’est disponible.

Le [workflow Release](.github/workflows/release.yml) exécute sur les cinq systèmes et architectures du tableau :

1. L’installation des outils de développement et la compilation des styles.
2. La suite de tests, avec les contrôles PowerShell d’archives en complément sous Windows.
3. La fabrication des packages et de leurs empreintes SHA256.
4. Les tests du package extrait : installation, redémarrage, conservation des données et retour arrière après échec.

Les pushes sur `main`, les pull requests et les lancements manuels produisent des artefacts de test. Un tag `vX.Y.Z` correspondant à `package.json` publie une release **uniquement si les cinq builds réussissent**. Consultez les [exécutions et leurs journaux](https://github.com/FluryWaana/atelier/actions/workflows/release.yml) pour le détail.

Pour lancer la suite localement :

```powershell
npm test
```

Avec le runtime inclus dans la distribution Windows :

```powershell
.\runtime\node.exe --test test/*.test.mjs
```

Tests sans appel payant à un modèle ni modification d’un Jira réel : cycle des cartes, restrictions HTTP, verrouillage du dossier, exécutions, erreurs, arrêt, pagination Jira, confidentialité du jeton et protocole CLI. Le fonctionnement réel dépend des autorisations et quotas de vos comptes.

Intégrations : [Codex non interactif](https://learn.chatgpt.com/docs/non-interactive-mode), [Claude Code programmatique](https://code.claude.com/docs/en/headless), [Jira Cloud REST](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/).

## Projets locaux

**Mes projets** permet d’enregistrer un nom et le chemin d’un dossier existant. Cliquer sur un projet filtre le Kanban par son dépôt ; le filtre **Tous les projets** réaffiche toutes les cartes. Une nouvelle tâche reprend le dépôt du projet sélectionné et propose également la liste des projets enregistrés. Les chemins restent modifiables sur les tâches sans worktree. Retirer un projet de la liste ne supprime aucun fichier, aucune carte ni aucun worktree. Les projets sont conservés dans SQLite. Le bouton **Parcourir…** ouvre le sélecteur de dossiers Windows. Annuler conserve le chemin déjà saisi. La saisie manuelle reste disponible.


## Espace projet et suivi

Sélectionner un projet ouvre son espace : **Kanban**, **Agents**, **Worktrees**, **Temps**, avec un lien local conservant le projet et l’onglet. Les cartes sont rattachées par leur dépôt source. L’onglet Agents présente l’activité, la durée, le journal via la carte et l’arrêt. Un silence de plus de 60 secondes est affiché comme « état à vérifier », pas comme une preuve de blocage. Les refus de permission structurés sont signalés. Les CLI restent non interactives : pour répondre à une question ou contourner un blocage fonctionnel, compléter les instructions de la carte puis relancer ; aucune session interactive n’est reprise.

Les nouvelles exécutions affichent aussi les tokens input/output, le cache lu/écrit, le modèle et un coût API approximatif en USD dans **Agents** et dans le détail de la carte. L’input inclut le cache ; le total ne le compte qu’une fois. Le coût et le temps cumulé suivent le périmètre affiché dans **Agents**, toutes dates. Claude fournit son estimation CLI ; Codex utilise les tarifs API standard du 21 octobre 2026. Le modèle Codex est lu dans les métadonnées du tour de sa session locale (`CODEX_HOME` ou `~/.codex/sessions`), en utilisant l’identifiant exact de l’exécution. Si ces métadonnées sont absentes, le modèle explicitement passé au CLI est conservé. Un modèle ou un tarif inconnu laisse le coût indisponible, sans lui substituer un autre modèle. Ce sont des estimations en contexte court, hors outils, majorations et conditions d’abonnement. Les bilans arrivent en fin de tour Codex ou en fin d’exécution Claude ; un arrêt peut laisser des données partielles ou absentes. Les anciennes exécutions sans bilan et les anciennes estimations basées sur un modèle de référence sont exclues du cumul de coût.

**Worktrees** regroupe les dossiers gérés par Atelier et les liens de PR enregistrés. **Actualiser l’état Git** relève les fichiers modifiés et les écarts de branche. Ce relevé est ponctuel, pas une surveillance du disque. **Actualiser les PR** relève leur état distant à la demande ; aucun statut de tâche n’est changé sans action humaine. **Nettoyer après livraison → Retirer sans forcer** conserve les branches et refuse les modifications locales.

**Tester les connexions** vérifie Git, les sessions Codex/Claude, Jira, l’API Azure du projet sélectionné et Tempo Cloud. Il utilise les réglages déjà enregistrés. Une lecture réussie ne garantit pas les droits d’écriture ou de push. Les erreurs restent visibles dans le dialogue actif.

Le compte rendu public de la dernière exécution réussie est proposé dans la description de PR, avec sa date et l’agent. Il faut le relire avec le diff actuel : ce n’est pas une nouvelle analyse ni une preuve que des tests ont été exécutés. Aucune nouvelle consommation de modèle pour assembler ce brouillon.

## Temps humain et Tempo Cloud

Depuis la carte ou l’onglet **Temps**, saisir une durée ou démarrer un chronomètre. Un seul chronomètre humain peut tourner sur le PC ; les durées des agents sont calculées séparément et ne deviennent jamais automatiquement des temps facturables. Le temps humain est enregistré toutes les 5 secondes. Après un arrêt brutal, le chronomètre est figé au dernier point enregistré et marqué à vérifier ; les périodes où le serveur est arrêté ne sont pas comptées. La durée maximale d’une saisie est 24 heures. La date et l’heure de début sont celles du navigateur ; une session traversant minuit reste rattachée au jour de début, à répartir manuellement si nécessaire.

Les saisies locales peuvent être corrigées ou retirées après arrêt du chronomètre. **Envoyer à Tempo…** présente le ticket, la date, la durée et la description avant validation. Renseigner un jeton **Tempo Cloud** dans les réglages (ou `TEMPO_TOKEN`) et la connexion Jira Cloud. Le jeton reste en mémoire et n’est pas transmis aux agents. L’auteur est le compte retourné par Jira `/myself`. L’intégration utilise [l’API officielle Tempo v4](https://apidocs.tempo.io/).

Une saisie reçue est verrouillée localement ; sa correction doit se faire dans Tempo. Si la réponse d’envoi est perdue, **Vérifier dans Tempo** recherche le marqueur stable `atelier:<id>` ajouté à sa description, sans nouvel envoi. Si aucune entrée n’est retrouvée, vérifier directement dans Tempo puis utiliser **Débloquer après vérification…** avant de retenter. Ne pas retirer le marqueur tant que la réconciliation est nécessaire. Une carte avec un historique de temps ne peut pas être supprimée du tableau.

Limites : Tempo Data Center et les attributs Tempo personnalisés obligatoires ne sont pas pris en charge ; dans ce cas le suivi local fonctionne et les refus d’envoi sont affichés. Aucun envoi réel ni test payant d’agent n’est effectué par les tests automatisés.

## Utilisation rapide

L’accueil vide propose de choisir un projet puis de créer une tâche. L’ajout d’un projet sélectionne immédiatement son espace ; **Parcourir…** propose le nom du dossier si le nom est encore vide. Lorsqu’un seul projet est enregistré, il est présélectionné pour une nouvelle tâche.

Les modèles **Fonctionnalité**, **Correction** et **Analyse** proposent une description structurée. Un texte personnalisé déjà saisi n’est jamais remplacé. Le modèle Analyse sélectionne le mode lecture seule. Choisir l’agent dès la création ne le lance pas : la tâche reste À faire.

Les filtres **À vérifier** et **À reprendre** montrent les livrables à relire et les tâches dont la dernière exécution a échoué, a été interrompue ou annulée. Les tâches terminées sont exclues de À reprendre. Un chronomètre humain actif reste visible dans tous les onglets et peut être arrêté depuis le bandeau.

La fiche de tâche met le lancement de l’agent en premier. Dépôt/branche, worktree, PR, temps et journal utilisent des sections repliables ; le journal est ouvert quand un agent travaille. Les réglages Jira, Azure et Tempo sont regroupés de la même manière.

Raccourcis hors champ de saisie et hors fenêtre modale : **N** crée une tâche, **/** place le curseur dans la recherche du Kanban, **Ctrl+K** ouvre la recherche globale (projets, tâches, tickets Jira et actions). Les exécutions terminées pendant la consultation du tableau déclenchent un message local ; aucune notification système ni permission de navigateur n’est nécessaire.


### Vue d’ensemble et rangement

L’onglet **Aperçu** rassemble les tâches à relire, les exécutions à reprendre, les agents actifs et les temps à valider. Les compteurs ouvrent directement la vue utile. La recherche **Ctrl+K** ignore les accents et retrouve aussi les archives ; utilisez les flèches puis Entrée.

Dans une fiche, ajoutez jusqu’à 20 **critères de réussite**, cochez ceux que vous avez vérifiés puis enregistrez. Ils accompagnent le prochain lancement d’agent et le brouillon de PR. L’agent ne les valide pas automatiquement.

**Dupliquer comme tâche locale** reprend le contenu et les critères (décochés), sans recopier le ticket Jira, la branche, le worktree, la PR ni l’historique.

Une tâche terminée peut être **archivée**, puis restaurée depuis **Archives**. Ses temps, journaux et worktrees restent disponibles dans leurs onglets. Arrêtez son chronomètre avant l’archivage. L’archive ne supprime aucun fichier et ne change pas Jira ; restaurez la tâche pour y travailler à nouveau.


### Préparer le suivi de temps

Dans **Temps**, filtrez par dates (incluses) et par état d’envoi Tempo. Le total humain suit ces filtres ; la durée des agents reste un total distinct, toutes dates. **Tout afficher** réinitialise les filtres. **Exporter en CSV** télécharge les saisies affichées et arrêtées, avec les durées exactes en secondes. Les chronomètres actifs sont exclus ; ce fichier ne déclenche aucun envoi Tempo. Les champs pouvant être interprétés comme formules sont neutralisés pour l’ouverture dans un tableur.

Le **compte rendu de l’agent** apparaît directement dans la fiche dès qu’il est disponible. Le sélecteur d’exécution permet de consulter un compte rendu précédent ; les journaux détaillés restent accessibles en dessous. Les critères de réussite restent à valider manuellement.


### Review de pull request

Dans la fiche d’une tâche avec PR, ouvrez **Pull request**, puis **Review avec Codex/Claude**. L’agent enregistré sur la carte analyse les changements en mode lecture seule. Le statut local de la tâche reste inchangé. Les constats sont classés P0 (critique), P1 (important), P2 (normal), P3 (mineur), avec fichier/ligne, scénario, conséquence et correction proposée.

Le jeton Azure permet de lire la PR active. Son dépôt et sa branche doivent correspondre au worktree, qui doit être propre et au commit source publié. Atelier récupère la branche cible avec Git (authentification Git existante), vérifie son commit et donne à l’agent la comparaison depuis l’ancêtre commun. Une PR de fork n’est pas prise en charge. Aucun commentaire, vote ou changement distant n’est publié.

Le résultat est conservé avec les identifiants des commits. Relancez après une nouvelle publication : ce rapport n’est pas une validation permanente de la PR. Les anciennes reviews sont consultables via le sélecteur d’exécution. Un résultat non structuré ou une interruption reste explicitement signalé, jamais assimilé à une review sans problème. Cette fonction lance une nouvelle review IA ; elle n’importe pas les commentaires des reviewers Azure.

Référence API : [Azure Get Pull Request](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-request?view=azure-devops-rest-7.1).


### Navigation et interface

Les onglets sélectionnent la vue ; la ligne **Projet** et la recherche ciblent le contenu. **Branches & PR** regroupe les worktrees et pull requests. Dans une fiche, les raccourcis **Contexte / Agent / Résultat / PR & review** mènent aux sections utiles. Les critères validés sont visibles sur les cartes du Kanban. Le thème sombre adapte les colonnes à la largeur disponible et conserve les indications textuelles en complément des couleurs.

L’interface est en HTML, CSS et JavaScript natifs, sans framework ni dépendance npm. Le serveur utilise les modules intégrés de Node.js, notamment `node:sqlite`, et les CLI Git, Codex et Claude installées sur le PC.


Retirer un projet depuis **Mes projets** demande confirmation et archive désormais toutes ses tâches, même inachevées, en conservant leur statut pour une restauration ultérieure. Aucun fichier ni ticket Jira n’est supprimé. Les agents, chronomètres ou envois en cours bloquent l’opération. Pour un projet retiré précédemment, la section **Tâches sans projet enregistré** permet d’archiver ses tâches restantes.


### Mettre à jour Jira lors d’un changement de colonne

Dans **Connexions & réglages → Jira → Colonnes du Kanban → statuts Jira**, renseignez les noms exacts ou IDs des statuts cibles, puis activez la mise à jour automatique et enregistrez. Une colonne sans correspondance reste locale. Le réglage s’applique au site connecté, tous projets confondus ; utilisez les transitions manuelles si leurs workflows diffèrent. Aucun ticket existant n’est modifié à l’activation.

Les changements de colonne par le sélecteur ou le glisser-déposer déclenchent la transition disponible. Les changements automatiques à la fin d’un agent restent locaux. Atelier vérifie le statut distant avant et après l’envoi : réessayer après une réponse perdue ne renvoie pas la transition si le ticket est déjà arrivé au statut cible. En cas d’échec, l’avancement local reste enregistré et une alerte persistante propose **Réessayer la mise à jour Jira**. Les transitions ambiguës ou nécessitant des champs supplémentaires doivent être effectuées manuellement. Une transition Jira peut déclencher les notifications et automatisations de votre workflow.

Référence : [API Jira, transitions des tickets](https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/).


### Préférences propres à chaque projet

**Mes projets → Réglages** définit l’agent préféré, la branche de départ et le comportement Jira (hériter, désactiver, correspondances propres). L’agent préféré s’applique aux nouvelles tâches ; une carte garde son agent explicitement choisi. La branche de départ s’applique aux nouveaux worktrees uniquement. Les jetons et l’adresse du site Jira restent globaux.

### Brouillons des fiches

Les nouvelles tâches et les fiches existantes conservent leurs saisies dans le stockage local du navigateur. Après fermeture ou rechargement, **Reprendre** restitue le brouillon sans lancer l’agent ni modifier le statut. **Enregistrer** applique les modifications et efface le brouillon. Les réglages contenant des jetons ne sont pas conservés en brouillon. Changer de navigateur ou vider ses données supprime ces brouillons ; ils ne sont pas inclus dans les sauvegardes SQLite.

### Du constat à la correction

Dans une review, cochez les constats puis **Corriger la sélection avec l’agent**. Atelier vérifie que les commits publiés correspondent encore au rapport et que le worktree est propre. L’agent corrige dans le même worktree sans publication automatique. Relisez ensuite, publiez les changements et relancez la review. Le lien à la review et les constats sélectionnés restent dans l’historique de l’exécution. Toute exécution de modification ou nouvelle publication via Atelier marque les reviews antérieures comme à refaire. **Vérifier l’actualité** contrôle les commits distants et le worktree ; les changements faits hors Atelier ne sont pas surveillés en permanence.

### Sauvegardes locales et restauration

Le menu **Sauvegardes** crée des instantanés SQLite dans `data/backups`. Ils contiennent projets, cartes, réglages, temps et journaux ; aucun jeton de connexion stocké en mémoire. Ce sont des copies sur le même disque : copiez ce dossier ailleurs pour vous protéger d’une panne matérielle. Les fichiers Git ne sont pas sauvegardés par cette fonction.

Une restauration remplace les données locales après confirmation et crée d’abord une sauvegarde de secours. Elle ne modifie ni les dépôts/worktrees sur disque ni Jira/Azure/Tempo. Les jetons sont oubliés, les reviews signalées comme à refaire. Les branches et PR peuvent avoir évolué depuis l’instantané : vérifiez-les avant de reprendre. L’opération est bloquée pendant une exécution, une synchronisation, un chronomètre ou un envoi Tempo ; une ancienne sauvegarde est refusée si elle ferait perdre la trace d’envois Tempo actuels. Les sauvegardes contenant des opérations non résolues sont refusées.

### Bilan quotidien

L’onglet **Temps** affiche le total humain et le détail par ticket pour la journée sélectionnée, avec les saisies à envoyer/vérifier. Le bilan suit le projet sélectionné et inclut ses tâches archivées. **Afficher les saisies de cette journée** applique le filtre de date à la liste et au CSV. Les durées sont regroupées par la date de saisie, sans découpage automatique d’un chronomètre traversant minuit.

### Suivre la livraison et préparer le travail

- **État des PR** : dans **Branches & PR**, **Actualiser les PR** relève les PR du projet sélectionné, archives incluses. La fiche propose aussi une actualisation individuelle. Les badges distinguent brouillon, ouverte, fusionnée, abandonnée, conflits et contrôles en échec/en cours. Les derniers statuts par contexte sont conservés ; ceux d’une ancienne itération sont exclus. La date du relevé et les erreurs restent visibles. Une PR reste consultable après retrait du worktree. Les règles de branche et les pipelines ne sont pas tous représentés par ces statuts : leur validation reste dans Azure. Une modification des commits distants périme les reviews concernées. Après fusion, **Terminer la tâche après fusion** demande confirmation et utilise la correspondance Jira habituelle.
- **Action suivante** : les cartes et leurs fiches indiquent quoi consulter ou reprendre après un échec de vérification, publication, review, synchronisation Jira ou agent. Le message s’appuie sur les états enregistrés, sans analyse IA supplémentaire.
- **Nettoyage après livraison** : cette section de **Branches & PR** propose les worktrees des tâches terminées ou archivées et des PR relevées comme fusionnées. Actualisez Git pour lire leur état. **Retirer sans forcer** demande confirmation puis utilise les protections Git pour le worktree principal et ses associés. Les branches et commits sont conservés ; une erreur laisse les associations restantes disponibles. Le bouton de retrait manuel historique reste distinct et avertit explicitement avant une éventuelle suppression de modifications.
- **Modèles personnels** : **Enregistrer comme modèle** dans une fiche reprend sa description, ses instructions, son mode et ses critères déjà enregistrés. Enregistrez vos modifications de fiche d’abord. Les critères sont décochés ; titre, agent, dépôt, dépendances, branche, liens et historiques ne sont pas repris. Les modèles sont disponibles dans **Nouvelle tâche**, peuvent être retirés et sont inclus dans les sauvegardes locales. Maximum : 50 modèles.
- **Bloquée par** : sélectionnez les tâches préalables dans la fiche (Ctrl pour une sélection multiple, 20 maximum), puis enregistrez. Tous les lancements d’agents attendent qu’elles soient **Terminées** ; archiver une tâche inachevée ne la débloque pas. Les cycles sont refusés. Il faut retirer les références avant de supprimer une tâche utilisée comme préalable.
- **File d’exécution** : **Ajouter à la file** utilise les réglages enregistrés de la carte. Dans **Agents**, la file globale affiche son ordre, permet de retirer les tâches en attente, de démarrer ou de mettre en pause. Un seul agent de la file tourne à la fois ; elle attend aussi les opérations manuelles déjà actives. Les nouveaux lancements manuels demandent de mettre la file en pause et d’attendre la fin de sa tâche active. Une pause laisse terminer le travail actif ; son arrêt reste disponible dans la fiche. Un échec, une annulation, un critère bloquant ou une dépendance inachevée suspend la file sur la tâche concernée. La reprise réutilise l’étape automatique interrompue lorsque c’est possible ; une exécution manuelle en échec est relancée. Après redémarrage ou restauration, la file reste en pause et ne lance rien seule. La file est un choix explicite, sans prise automatique de tickets hors de sa liste.
- **Bilan à copier** : disponible dans **Aperçu** et **Temps**, pour la journée et le projet choisis, archives incluses. Le texte regroupe tâches travaillées actuellement terminées, exécutions finies ce jour, PR associées, blocages actuels et temps humain enregistré. Les journées des exécutions suivent l’heure locale ; les temps suivent leur date de saisie. Les portions actives des chronomètres sont exclues. Les blocages et statuts sont ceux du moment, pas une reconstitution historique. Aucun appel IA ni envoi extérieur ; le bouton copie seulement le texte dans le presse-papiers.

Références Azure : [état d’une PR](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-request?view=azure-devops-rest-7.1), [statuts des contrôles](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-statuses/list?view=azure-devops-rest-7.1).

## Documentation et contribution

| Guide | Contenu |
| --- | --- |
| [Architecture](docs/architecture.md) | Organisation du code et responsabilités des couches |
| [Créer un connecteur](docs/connectors.md) | Contrat des modules et exemple exécutable |
| [Connecter les dépôts](docs/repository-connectors.md) | Fournisseurs de PR/MR, sessions CLI et jetons |
| [Publier et mettre à jour](docs/releases.md) | Versions, packages, pipeline et restauration après échec |
| [Consignes du dépôt](AGENTS.md) | Conventions et vérifications attendues |

Pour proposer une modification, décrivez le problème, le comportement obtenu et les vérifications réalisées dans la pull request. Exécutez `npm test` et, si les styles changent, `npm ci` puis `npm run build:css`. Joignez une capture pour une modification d’interface. Ne versionnez pas de jetons, bases locales, sauvegardes ou journaux.

### Licence

Aucune licence de distribution du projet n’est actuellement définie dans ce dépôt. Les composants tiers conservent leurs licences respectives ; les packages incluent celle du runtime Node.js.
