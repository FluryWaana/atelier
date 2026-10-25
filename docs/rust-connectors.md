# Connecteurs par processus Rust — contrat candidat v1

Ce contrat remplace les modules JavaScript de référence. Il ne charge pas de code Node et n’ajoute pas de crate au workspace. La première livraison comprend les types/validations purs dans `atelier-core::application::extensions` et l’exemple `crates/atelier-app/examples/connector-demo.rs`. **L’exécuteur natif, le registre, le coffre par module, les écrans et le raccordement des capacités restent à livrer. Aucun exemple ni scénario n’est compilé/exécuté à ce stade.** Le protocole écrit seul ne qualifie aucune intégration.

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

Le connecteur demo fournit describe/check, une recherche et lecture d’un ticket `DEMO-1`, un champ titre, des traductions et une action de comptage. Aucun compte, secret, fichier, accès réseau, agent payant ou écriture distante. Les URLs example.org sont des identités de démonstration, jamais visitées. Il n’annonce pas les capacités dépôts/temps qu’il ne fournit pas.

La construction prévue est `cargo build -p atelier-app --example connector-demo --locked`, uniquement dans l’environnement de qualification autorisé à la fin. L’exemple ne doit pas entrer dans les paquets de l’application. Les scénarios purs écrits couvrent manifeste invalide, doublons, opérations sans rapprochement, intention manquante, types publics/secrets distincts, taille, réponse tronquée/double et identité divergente. **Tous sont non exécutés.** Restent requis : vrais échanges avec le binaire compilé, flux/délais/descendants, registre/coffre/concurrence/reprise, parcours utilisateur des capacités, plateformes et contrôle des paquets sans Node.
