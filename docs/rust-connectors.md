# Connecteurs par processus Rust — contrat candidat v1

Ce contrat remplace les modules JavaScript de référence. Il ne charge pas de code Node et n’ajoute pas de crate au workspace. Types/validations purs, exemple Rust, exécuteur Windows, registre/coffre et API de lecture sont candidats. **Les écrans, l’application des tickets et les autres capacités métier, les écritures avec reprise et POSIX restent à livrer. Aucun exemple ni scénario n’est compilé/exécuté à ce stade.** Le protocole/exécuteur/API seuls ne qualifient aucune intégration.

## Transport

Une invocation crée un processus natif, transmet un document JSON UTF-8 sur stdin puis ferme stdin. Le processus produit exactement un document JSON sur stdout et termine. Les espaces finaux sont autorisés ; seconde réponse, texte de progression, document tronqué, version ou identité divergente sont refusés. Entrée au plus 256 Kio, réponse 512 Kio. Stderr sera capturé séparément et ne sera jamais une réponse ou un journal utilisateur brut.

Le lanceur doit utiliser un exécutable explicitement installé, un environnement sélectionné et un dossier de travail privé, sans dossier de données ou de dépôt transmis implicitement. Pas de shell, script Node, recherche d’un module dans un dépôt ni exécution automatique à l’ouverture. L’exécutable reste un programme de confiance avec les droits de l’utilisateur : le protocole ne constitue pas un sandbox OS. Le futur adaptateur doit contenir/arrêter ses descendants et borner temps/flux avant d’exposer ces opérations.

Une requête contient `protocol: "atelier-connector/1"`, `requestId`, `module`, `method`, `operationId`, `action`, `settings`, `secrets` et `input`. Les identifiants commencent par une lettre minuscule, puis lettres minuscules/chiffres/tirets, 64 octets maximum. La réponse reprend protocole, requestId, module et method, avec `status` (`ok`, `error`, `uncertain`), `message` et `output`. Les champs inconnus de ces enveloppes sont refusés. Le contrôle d’enveloppe ne remplace pas la validation métier du résultat avant une application locale.

La découverte `describe` reçoit des settings/secrets vides, input null et aucune action/intention. Elle retourne le manifeste. Exemple de requête, à transmettre uniquement au binaire synthétique construit dans la campagne finale :

```json
{"protocol":"atelier-connector/1","requestId":"request-synthetic-1","module":"demo","method":"describe","operationId":null,"action":null,"settings":{},"secrets":{},"input":null}
```

## Manifeste et réglages

Le manifeste déclare id, nom, description, langue source (fr-FR/en-US), méthodes, champs, onglets/actions et traductions. Noms des connecteurs intégrés réservés ; doublons refusés. Limites : 32 champs/méthodes, 8 onglets, 16 actions par onglet, 100 choix par select, 200 messages par langue, manifeste 128 Kio maximum. Chaque champ est text, url, password, checkbox, select ou textarea. Un select exige des choix distincts ; les autres types n’acceptent aucun choix. Un checkbox exige un booléen, sans coercition ; les autres valeurs publiques sont des chaînes de 4 000 unités maximum. Les labels/traductions sont du texte, jamais du HTML, JavaScript ou CSS exécutable.

Les champs password sont distincts des réglages publics et ne peuvent apparaître dans `settings`. Seuls les secrets déclarés par ce module peuvent être transmis dans `secrets`. Le futur registre doit les conserver au coffre dans une portée propre au module et ne renvoyer que leur présence au frontend. Toute modification d’adresse ou de champ `credentialScope` doit retirer les anciens secrets non remplacés explicitement et incrémenter la révision. Changement de binaire, suppression et restauration doivent invalider cette portée. Le futur adaptateur doit refuser les secrets reflétés dans stdout, objets imbriqués et clés JSON compris, et remplacer les erreurs de processus par des messages constants.

## Méthodes et effets

| Groupe | Méthodes |
| --- | --- |
| Description/diagnostic | describe, check |
| Tickets | ticketsSearch, ticketsRead, ticketsReset, ticketsCreate, ticketsTransitions, ticketsTransition |
| Dépôts | repositoriesPreview, repositoriesPublish, repositoriesRefresh, repositoriesReview |
| Temps | timePreview, timeSend, timeReconcile |
| Actions/rapprochement | action, reconcile |

`ticketsCreate`, `ticketsTransition`, `repositoriesPublish` et `timeSend` annoncent un effet distant ; `ticketsReset` annonce un effet local. Une action déclare explicitement read/local/remote. Les autres méthodes sont des lectures. Check est obligatoire ; describe est implicite et n’est pas répété dans le manifeste. Une méthode/action distante exige la méthode reconcile. Chaque écriture exige un operationId persistant ; les rapprochements exigent l’identifiant original.

Le futur hôte doit conserver une intention avant tout lancement d’écriture, revalider profil/révision/instantanés avant application et ne jamais rejouer une écriture après erreur ou arrêt. Reconcile décrit l’observation de l’opération originale ; il ne la réexécute pas. Une absence ne prouve pas l’impossibilité d’un effet retardé. `status: ok` est une réponse du programme, pas une preuve indépendante d’un effet distant ou d’un critère de tâche validé. Les modèles des résultats, identités de site/dépôt/ticket, snapshots de review, pagination et confirmations doivent être raccordés à leurs cas d’usage avant de déclarer une capacité disponible.

## Exemple synthétique et preuves à recueillir

Le registre candidat conserve jusqu’à 16 modules/2 Mio de métadonnées, avec une génération globale monotone. Installation confirmée après nouvelle lecture de l’image et comparaison du manifeste réellement relu à l’aperçu ; les doublons sont refusés. Les réglages publics/password sont séparés ; seules les présences de secrets reviennent au frontend. Modification de base HTTPS ou d’un champ credentialScope retirant les anciens secrets non remplacés explicitement, avec révision nouvelle et ancien rapport invalidé. Champs/types/adresses/actions indisponibles et révisions périmées refusés avant déchiffrement ; chiffrement puis transaction courte pour réglages/coffre. Retrait confirmé supprimant seulement profil/secrets locaux, cartes conservées. Restauration incrémentant la génération au-delà des deux instantanés et invalidant rapports/identifiants ; un retrait/réinstallation ne réutilise pas une ancienne révision.

Les routes candidates `/api/extensions`, `/probe`, `/install`, `/{id}` et `/{id}/invoke` exigent la session locale. Probe/install/configuration/appels Windows sérialisés par la garde d’opérations existante ; vérification d’empreinte avant chaque processus. Check, recherche/lecture de tickets et actions read peuvent produire un rapport daté, appliqué uniquement au profil/révision encore présents. Le compteur de tickets importés est calculé par l’hôte pour les actions, sans confiance dans une valeur envoyée par le client. Aucune carte n’est créée ou modifiée automatiquement. **Les écritures, rapprochements et méthodes encore sans cas d’usage sont refusés ; aucun effet distant n’est simulé.** Les formulaires et l’import confirmé restent à raccorder. Scénarios de registre/coffre/révisions/scope/données publiques privés et panne de chiffrement écrits, non exécutés.

L’exécuteur Windows candidat accepte une image native absolue locale, refuse UNC/scripts/chemins relatifs, ouvre le fichier sans partage d’écriture/suppression et calcule son SHA-256 avec borne réelle de 128 Mio. Une image nouvellement inspectée peut seulement fournir son manifeste, sans contexte ni secret ; les autres appels exigent une empreinte enregistrée identique. Identité du fichier recontrôlée avant/après appel et handle conservé pendant son exécution. Il s’agit d’un contrôle de l’image sélectionnée, pas d’un sandbox contre un programme déjà autorisé avec les droits de l’utilisateur.

Les appels préparés sont consommés une fois et reçoivent exclusivement leur requête sur stdin, jamais les secrets en argument ou fichier hôte. Réutilisation de la résolution native/environnement sélectionné et de la supervision Job Object. HOME/USERPROFILE/AppData/temp pointent sur un temporaire privé ; locale et variables d’injection/authentification implicites écartées. Une URL de configuration doit être une base HTTPS sans identifiants/query/fragment. Découverte limitée à 15 s, autres appels bornés par l’hôte entre 100 ms et 180 s ; 512 Kio par flux capturé. Sortie non zéro, timeout, dépassement, identité/trame invalide ou secret reflété (valeurs et clés JSON imbriquées) refusent la réponse, sans relayer stdout/stderr brut. Succès accepté après arrêt des descendants et nettoyage du temporaire ; un échec/crash peut laisser un temporaire privé, sans effacement d’un chemin récupéré depuis la base. Ce composant n’est pas encore une route utilisateur : le futur registre doit réserver les intentions et figer révision/portée avant dispatch.

Scénarios Windows écrits sur le faux exécutable compilé du workspace : découverte sans secret, diagnostic avec home privé, empreinte périmée, remplacement de l’image refusé pendant le pin puis empreinte invalidée après modification, secrets dans valeur/clé/stderr, identité étrangère, réponse supplémentaire/tronquée, flux excessif et timeout avec descendant observé par handle. Scénario pur d’environnement sans mutation des variables du processus parent et borne de lecture de l’image. **Tous non exécutés et code non compilé** ; l’exemple autonome reste aussi à qualifier, le faux exécutable ne le remplace pas.

Le connecteur demo fournit describe/check, une recherche et lecture d’un ticket `DEMO-1`, un champ titre, des traductions et une action de comptage. Aucun compte, secret, fichier, accès réseau, agent payant ou écriture distante. Les URLs example.org sont des identités de démonstration, jamais visitées. Il n’annonce pas les capacités dépôts/temps qu’il ne fournit pas.

La construction prévue est `cargo build -p atelier-app --example connector-demo --locked`, uniquement dans l’environnement de qualification autorisé à la fin. L’exemple ne doit pas entrer dans les paquets de l’application. Les scénarios purs écrits couvrent manifeste invalide, doublons, opérations sans rapprochement, intention manquante, types publics/secrets distincts, taille, réponse tronquée/double et identité divergente. **Tous sont non exécutés.** Restent requis : vrais échanges avec le binaire compilé, flux/délais/descendants, registre/coffre/concurrence/reprise, parcours utilisateur des capacités, plateformes et contrôle des paquets sans Node.
