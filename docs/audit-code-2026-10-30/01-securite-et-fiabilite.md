# Sécurité et fiabilité du code Atelier Kanban

L'application expose une API locale qui peut lire des dépôts, lancer des processus et modifier des données. Les frontières importantes sont donc le navigateur, le système de fichiers, les exécutables et les services distants. Un poste ou un compte utilisateur déjà compromis sort du niveau de protection que la session HTTP peut fournir.

Les liens ci-dessous pointent vers les sources du dépôt. Les fonctions citées permettent de retrouver les observations même si les numéros de ligne changent.

## À corriger en priorité

### 1 Durabilité incomplète des publications du journal sous Linux

**Niveau : élevé pour la récupération après coupure. Statut : absence de synchronisation confirmée par lecture, perte de données non reproduite.**

Dans [publish.rs](../../crates/atelier-app/src/infrastructure/publish.rs), `write_private_from` crée un fichier temporaire, écrit son contenu, appelle `file.sync_all()`, ferme le fichier puis le renomme. Le dossier destination n'est pas synchronisé ensuite. La fonction `sync_directory` existe déjà pour Linux dans le même module, mais cette publication ne l'appelle pas.

Le chemin [updates.rs](../../crates/atelier-app/src/infrastructure/updates.rs), fonction `save`, utilise directement `publish::write_private` pour `update-operation.json`. `phase` s'appuie sur cette sauvegarde pour enregistrer l'avancement de la mise à jour. La recherche des appels et des synchronisations dans ce fichier ne montre pas de synchronisation compensatrice du dossier.

**Situation concrète :** une phase est annoncée comme enregistrée, puis le poste perd brutalement son alimentation après le renommage. La persistance de l'entrée du dossier n'est pas assurée par la synchronisation du fichier seul. Le journal peut donc ne pas représenter la dernière phase acquittée après redémarrage. Le résultat exact dépend du système de fichiers et du stockage. Il ne s'agit pas d'une corruption SQLite démontrée.

Cette distinction entre fichier et dossier est explicitée par la [documentation Linux de fsync](https://man7.org/linux/man-pages/man2/fsync.2.html), consultée pour vérifier ce point.

**Correction minimale proposée :** réutiliser la synchronisation du dossier ancré après la publication Linux, avant d'acquitter la sauvegarde. Lorsque staging et destination diffèrent, définir et appliquer les synchronisations nécessaires sur les deux dossiers. Propager les erreurs. Examiner les autres appelants de la fonction partagée avant de changer son contrat.

**Sans correction :** la garantie de récupération d'une mise à jour après panne électrique reste plus faible que la garantie d'écriture atomique. La première concerne ce qui survit au redémarrage. La seconde évite principalement la lecture d'un fichier partiellement écrit.

**Validation attendue :** tests synthétiques du chemin commun et des erreurs de synchronisation, puis qualification de reprise avec panne de stockage simulée dans un environnement jetable. Un arrêt normal du processus ne reproduit pas une panne électrique. Les tests existants de `publish.rs` couvrent notamment l'ancrage, les permissions, le nettoyage et la conservation d'une destination, sans démontrer la durabilité de ce journal après coupure.

## Protections observées

| Frontière | Protection visible | Source et portée de la preuve |
| --- | --- | --- |
| Écoute HTTP | Écoute sur IPv4 loopback dans le binaire et refus d'un listener non loopback dans le serveur | [main.rs](../../crates/atelier-app/src/main.rs), lancement du serveur. [http.rs](../../crates/atelier-app/src/http.rs), `serve_mode` |
| Navigateur étranger | Contrôle exact de Host et contrôle d'Origin lorsqu'il est présent | `http.rs`, `authorize`. L'absence d'Origin ne contourne pas l'exigence de session API |
| Session API | Jeton requis, comparaison en temps constant, exigence JSON et en-tête local pour les mutations | `http.rs`, `authorize`. [session.rs](../../crates/atelier-app/src/infrastructure/session.rs), `same_secret` |
| Création de session | 32 octets issus du générateur du système | `session.rs`, `new_token`. Une erreur du générateur provoque un refus |
| Découverte du serveur | Preuve HMAC sur challenge et port, distincte d'une réponse métier authentifiée | `session.rs`, `proof`. `http.rs`, `health` et [local-http.rs](../../crates/atelier-app/tests/local-http.rs) |
| Contenu de page | CSP, refus d'intégration dans une frame, `nosniff`, `no-store` | `http.rs`, `CSP` et `security`. Le chargeur Trunk est externalisé par [xtask](../../xtask/src/main.rs) |
| Corps HTTP | Limite de lecture de 400 000 octets et validation d'un objet JSON | `http.rs`, `object`. La limite supplémentaire en unités UTF-16 est discutée dans le document qualité |
| Assets | Noms autorisés, fichiers contrôlés, chargement borné et capture des octets au démarrage | [assets.rs](../../crates/atelier-app/src/http/assets.rs), `asset_router` |
| SQL | Valeurs liées aux paramètres, noms de tables issus d'une enum fermée | [store.rs](../../crates/atelier-app/src/infrastructure/store.rs), `Documents`, `document`, `put`, `delete`. Le SQL formaté observé n'est pas à lui seul une injection |
| Format des données | Identité ATKR et version 1 contrôlées, initialisation d'une base vide dans une transaction | `store.rs`, `initialize`, `validate_format`. Le refus d'un format inconnu est explicite |
| Accès concurrent SQLite | Worker dédié et canal de capacité 64, fermeture ordonnée | [database.rs](../../crates/atelier-app/src/infrastructure/database.rs). Cette borne ne limite pas à elle seule le nombre de requêtes HTTP en attente |
| Sauvegarde et restauration | Contrôle d'intégrité de la source et sauvegarde de secours avant restauration | [backups.rs](../../crates/atelier-app/src/infrastructure/backups.rs), `restore_checked`. Parcours complets non exécutés ici |
| Parcours de fichiers | Validation des chemins et contrôles de liens ou d'identité aux frontières | [domain/files.rs](../../crates/atelier-core/src/domain/files.rs), [checked_file.rs](../../crates/atelier-app/src/infrastructure/checked_file.rs), [repository_access.rs](../../crates/atelier-app/src/infrastructure/repository_access.rs). Cela ne constitue pas une preuve de tous les chemins natifs |
| Secrets stockés | DPAPI Windows, coffre natif et chiffrement authentifié dans l'adaptateur de jetons | `session.rs`, [token_vault.rs](../../crates/atelier-app/src/infrastructure/token_vault.rs). La session locale Unix utilise un fichier privé, distinct des jetons d'intégration |
| Configuration publique | Retour de drapeaux de présence de jetons plutôt que leur contenu dans `public_config` | [application/settings.rs](../../crates/atelier-core/src/application/settings.rs). L'absence de toute fuite par tous les endpoints n'est pas attestée |
| Appels Tempo | HTTPS en mode produit, pas de proxy, pas de redirection ni de retry automatique, délais et réponse bornés | [tempo/client.rs](../../crates/atelier-app/src/infrastructure/tempo/client.rs), `prepare`, `request` |
| Appels Jira et forge | HTTPS produit, redirections refusées et délais explicites | [jira.rs](../../crates/atelier-app/src/infrastructure/jira.rs), [forge.rs](../../crates/atelier-app/src/infrastructure/forge.rs). Leur politique proxy diffère de Tempo, il ne faut pas généraliser `no_proxy` à tous les clients |
| Export distant incertain | Réservation locale, contrôle de l'aperçu, état d'incertitude et rapprochement | [tempo_export.rs](../../crates/atelier-app/src/infrastructure/tempo_export.rs), `reserve`, `active`, `remember`, `record`, et [handler HTTP](../../crates/atelier-app/src/http/tempo_export.rs). Aucun succès distribué ne peut être déduit d'un simple délai dépassé |
| Processus | Capture de sortie bornée, délai et nettoyage du processus possédé | [process_tree.rs](../../crates/atelier-app/src/infrastructure/process_tree.rs), `capture_child` |
| Processus Windows | Job Object possédé, lancement suspendu et rattachement avant reprise | [process_tree/windows.rs](../../crates/atelier-app/src/infrastructure/process_tree/windows.rs), `spawn_ready` |
| Extensions | Protocole versionné, tailles bornées, empreinte du binaire et copie privée Unix vérifiée | [extensions.rs](../../crates/atelier-app/src/infrastructure/extensions.rs), [contrat](../../crates/atelier-core/src/application/extensions.rs). Ces mécanismes ne sont pas une sandbox de permissions système |
| Paquets | Métadonnées et empreintes vérifiées par le chemin de signature Ed25519 | [update_package.rs](../../crates/atelier-app/src/infrastructure/update_package.rs). Le modèle `Trust` lu exige une clé marquée synthétique. Ce candidat n'atteste pas une chaîne de distribution de production |

## Durcissement et limites connues

### 5 Le jeton de session apparaît dans une URL de flux

**Niveau : faible et conditionnel. Statut : transport du jeton confirmé, fuite effective non démontrée.**

[live.rs](../../crates/atelier-ui/src/live.rs), `BoardState::listen`, construit `/api/events?session={token}` pour `EventSource`. [http.rs](../../crates/atelier-app/src/http.rs), `authorize`, accepte ce paramètre pour le flux. Un outil qui enregistre l'URL complète de cette requête peut ainsi conserver un jeton donnant accès aux autres endpoints API de cette session.

La revue n'a pas établi qu'un journal d'accès actif collecte actuellement ces URL. Ce point ne doit donc pas être décrit comme une fuite existante. La suppression du fragment initial par `session()` ne supprime pas la query de cette requête SSE.

**Durcissement minimal :** exclure les paramètres sensibles de tout journal ou diagnostic réseau ajouté. Si les flux doivent être journalisés, utiliser un mécanisme de flux à portée restreinte ou un transport authentifié adapté. Préserver les contrôles Host et Origin ainsi que la session API. Le choix du transport nécessite une validation du comportement navigateur.

### Limites déjà déclarées dans le code

- Sous Unix, les groupes de processus ne contiennent pas les descendants qui appellent `setsid` ou `setpgid`. Le commentaire `shortcut:` de [process_tree/unix.rs](../../crates/atelier-app/src/infrastructure/process_tree/unix.rs) le dit explicitement. Ce n'est pas présenté comme une faille nouvellement découverte.
- Le contrôle de taille d'un téléchargement Git se fait par inspection périodique. Il peut dépasser temporairement son seuil. Cette limite est déclarée dans [git_transport.rs](../../crates/atelier-app/src/infrastructure/git_transport.rs), `download_guard`.
- Les exécutables agents, commandes et extensions disposent des droits du compte qui les lance. Un environnement filtré et un contrôle d'empreinte n'empêchent pas un binaire malveillant choisi par l'utilisateur d'utiliser ces droits.
- Le jeton local est conservé dans `localStorage` et `sessionStorage` par [main.rs](../../crates/atelier-ui/src/main.rs), `session`. Un script exécuté dans la même origine peut le lire. La CSP réduit la surface mais ne prouve pas l'absence de XSS. Aucune XSS exploitable n'a été démontrée dans cette revue.

## Ce qui reste à établir

Une recherche transversale des appels d'insertion HTML directe et de désactivation explicite de vérification TLS dans les sources examinées n'a pas remonté ces motifs. Une recherche textuelle ne suffit pas à prouver l'absence de XSS ou de problème TLS.

L'état des vulnérabilités publiées des dépendances n'a pas été vérifié contre une base d'avis de sécurité. Le verrouillage des versions ne suffit pas à conclure qu'elles sont sûres. La validité des coffres natifs, les permissions effectives, les scénarios de course, les arrêts réels et la résistance aux pannes restent à qualifier sur les systèmes visés.
