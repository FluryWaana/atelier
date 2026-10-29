# Colonnes du Kanban et correspondances Jira

Le bouton « Configurer les colonnes » du Kanban ouvre les réglages du projet choisi. Depuis la vue de tous les projets, on choisit d’abord le projet à configurer. L’enregistrement sélectionne ce projet dans le tableau pour montrer le résultat.

Les cinq étapes métier restent todo, doing, review, done et standby. Leurs noms affichés et leur ordre sont configurables par projet. Les cartes conservent leurs identifiants et leurs étapes. Les noms sont aussi affichés dans l’éditeur de carte. La vue de tous les projets conserve les cinq intitulés communs. Ajouter ou supprimer des étapes métier ne fait pas partie de cet incrément.

## Récupération du tableau Jira

L’utilisateur saisit l’identifiant de son tableau puis demande ses colonnes. La lecture utilise GET /rest/agile/1.0/board/{boardId}/configuration. Le client conserve le chemin de contexte Jira Data Center et le passage Cloud par api.atlassian.com pour les jetons concernés. L’accès est borné, soumis à la session locale et à la révision de connexion, puis revalidé après la lecture. Les erreurs d’accès restent visibles. Aucun compte réel n’est utilisé pendant le développement.

La réponse est projetée en colonnes ordonnées avec tous leurs identifiants de statut. Les URL retournées par Jira ne sont jamais suivies. Le parseur refuse les identités divergentes, les statuts dupliqués, les contrôles dans les noms et les réponses trop grandes. Les champs retournés au navigateur sont vérifiés contre le jeton.

Sources du contrat : [Jira Cloud](https://developer.atlassian.com/cloud/jira/software/rest/api-group-board/#api-rest-agile-1-0-board-boardid-configuration-get) et [Jira Data Center](https://developer.atlassian.com/server/jira/platform/rest/v11000/api-group-board/).

## Correspondances manuelles

Chaque colonne Jira peut être associée à une colonne locale ou laissée sans correspondance. Tous ses statuts sont conservés et associés ensemble. Plusieurs colonnes Jira peuvent pointer vers la même étape locale. Aucun rapprochement par nom n’est effectué. Une lecture suivante conserve les associations d’identifiants du même tableau et du même compte. Un groupe devenu partiel doit être revu avant enregistrement.

La configuration est enregistrée dans le projet avec une condition sur l’ancienne configuration. Un changement de site, de compte ou de mode Jira refuse l’enregistrement. Une révision devenue obsolète pendant la sauvegarde provoque aussi un conflit. L’adresse, le compte et le mode Cloud ou Data Center bornent la portée des associations. Choisir Local comme source de tickets ne désactive pas les réglages locaux des colonnes.

L’enregistrement ne déplace aucune carte et ne transmet aucune transition à Jira. Les nouveaux imports utilisent la colonne explicitement associée au statut reçu. Les cartes déjà importées ou attachées gardent leur progression locale. Une synchronisation propose une colonne issue du matching et conserve la confirmation distincte existante. Modifier les correspondances invalide un aperçu de synchronisation ancien. Une configuration Kanban explicite remplace le matching historique par nom pour la lecture des statuts.

## Vérification

Scénarios synthétiques préparés pour les limites et groupes de statuts, la portée site et compte, les configurations concurrentes, un échec SQLite sans perte de cartes, les lectures Cloud et Data Center, les imports nouveaux et répétés et un parcours navigateur de renommage, ordre, rechargement et conflit.

Le formatage et les compilations natives et WASM passent le 30 octobre. Commandes utilisées : cargo fmt --all -- --check, cargo check -p atelier-core -p atelier-app -p xtask --tests --locked et cargo check -p atelier-ui --target wasm32-unknown-unknown --locked avec le répertoire de compilation séparé integration-check. Les avertissements concernent le cache de liens physiques Windows. Les tests sont compilés sans être exécutés. Les tests, Clippy, scénarios navigateur et CI ne sont pas exécutés pour cet incrément. Aucun accès à data/ ni à un compte réel.
