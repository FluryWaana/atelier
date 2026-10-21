# Dépôts Git : Azure DevOps, GitHub et GitLab

Dans **Paramètres → Modules → Dépôts Git**, choisissez le fournisseur, puis
enregistrez. La rubrique **Dépôts Git** affiche ses paramètres. Les modules
GitHub et GitLab sont fournis avec Atelier ; aucune extension npm n’est nécessaire.
Ils gèrent les PR/MR, pas les issues : le fournisseur de tickets reste indépendant.

## Fournisseur par projet

Dans **Mes projets → Réglages → Fournisseur Git**, laissez **Hériter du fournisseur
général** ou choisissez un connecteur installé. Ce choix concerne les PR/MR du
dépôt principal du projet, y compris le parcours automatique. Il n’altère pas
les remotes Git ni les dépôts Front/Back liés au lancement des applications.

La priorité est : fournisseur de la PR/MR existante, puis fournisseur du projet,
puis fournisseur général. Vider le choix du projet rétablit l’héritage. Un module
supprimé provoque une erreur explicite ; il ne déclenche pas de repli silencieux.
Les tests de connexion suivent aussi le fournisseur du projet correspondant au dépôt.

Les adresses, sessions CLI et jetons restent configurés par connecteur dans les
paramètres généraux. Les réglages d’un connecteur utilisé par un projet y sont
visibles même si un autre fournisseur est sélectionné globalement.

## Authentification

Utilisez de préférence la session CLI déjà ouverte sur votre poste. Un jeton
personnel saisi dans Atelier est prioritaire ; laissez ce champ vide pour utiliser
la CLI, ou cliquez **Oublier ce secret** si un jeton était déjà enregistré.

| Fournisseur | Session CLI, sans jeton dans Atelier | Jeton personnel facultatif |
| --- | --- | --- |
| Azure DevOps | Installer Azure CLI, puis `az login` | PAT avec Code en lecture/écriture |
| GitHub | Installer GitHub CLI, puis `gh auth login --hostname github.com` | Jeton classique avec accès au dépôt (`repo` pour les dépôts privés), ou jeton finement limité au dépôt avec Contents en lecture, Pull requests en lecture/écriture, Checks et Commit statuses en lecture |
| GitLab | Installer GitLab CLI, puis `glab auth login --hostname gitlab.com` | Jeton avec le scope `api` et un rôle permettant de créer les MR du projet |

Les CLI sont externes au paquet Atelier et doivent être accessibles dans `PATH`.
Atelier lit les jetons via `gh auth token --hostname …`, `glab auth token --hostname …`
ou la commande Azure CLI existante. Il ne lance pas de connexion interactive et
ne stocke pas les jetons obtenus auprès des CLI. Le stockage de la session CLI
reste géré par cet outil et le système.

Les jetons saisis dans Atelier restent dans son coffre chiffré ; ils ne sont pas
retournés au navigateur ni ajoutés aux commandes Git ou aux instructions des agents.
Changer l’adresse de la forge oublie le jeton enregistré, sauf s’il est ressaisi
dans la même sauvegarde. Les redirections de l’API ne sont pas suivies avec le jeton.

**L’authentification de l’API et celle de Git sont distinctes.** Les `fetch` et
`push` utilisent la clé SSH ou le gestionnaire d’identifiants Git déjà configuré.
Un jeton enregistré dans Atelier n’est pas injecté dans Git. Pour GitHub HTTPS,
`gh auth setup-git` permet de configurer volontairement le helper Git de GitHub CLI.
Vérifiez les droits de push du compte Git sur le dépôt avant publication.

## Instances privées

GitHub utilise `https://github.com` par défaut. Pour GitHub Enterprise Server,
renseignez l’origine HTTPS, par exemple `https://github.entreprise.fr` ; l’API
utilisée sera `/api/v3`. GitLab utilise `https://gitlab.com` par défaut ; une
instance privée peut inclure un préfixe, par exemple `https://git.entreprise.fr/gitlab`.
Son API est située sous `/api/v4`.

Connectez la CLI au même hôte, avec `--hostname votre-hote`, ou renseignez un jeton
de cette instance. Le remote `origin`, en HTTPS ou SSH, doit correspondre à la
forge configurée. Les sous-groupes GitLab sont acceptés. Les alias SSH qui changent
le nom d’hôte ne sont pas résolus par Atelier : utilisez le nom réel dans le remote.
Les certificats HTTPS doivent être reconnus par Node.js ; aucun contournement TLS
n’est effectué. **Tester les connexions** vérifie la lecture du dépôt, pas un push.

## Publication, suivi et review

- L’aperçu montre les modifications, la branche cible et le texte à publier.
  Une empreinte vérifie que les fichiers n’ont pas changé avant le commit et le push.
- La création produit une PR GitHub brouillon ou une MR GitLab `Draft:`.
  Une PR/MR ouverte de la même branche et de la même cible est réutilisée.
  Après une réponse de création perdue, Atelier recherche la PR/MR avant de réessayer.
- La publication automatique refuse une PR/MR sortie du brouillon ou dont la
  fusion automatique est activée. Aucun push forcé ni fusion n’est exécuté.
- Le suivi affiche l’état ouvert, fusionné ou fermé, les conflits, les statuts
  de commit et check runs GitHub, ou le dernier pipeline GitLab du commit source,
  ainsi que les votes/approbations disponibles. Une lecture incomplète est signalée.
- La review IA reste locale : aucun commentaire ou vote n’est publié. Elle exige
  un worktree propre au commit source distant et vérifie le commit cible récupéré.

Les PR/MR issues de forks ne sont pas prises en charge. Les règles de protection,
approbations obligatoires et détails de tous les jobs restent à vérifier dans la
forge. Une PR existante conserve son fournisseur lorsque le choix global change ;
retirer son module rend ses opérations distantes indisponibles.

Tests simulés : `test/forge-repositories.test.mjs`. Ils couvrent les API et les
commandes Git/CLI sans publier dans un vrai dépôt ni lire de vrais identifiants.
