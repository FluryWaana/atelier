# Sources de tickets et export du temps

## Fonctionnement retenu

Atelier fonctionne sans outil externe : les tickets et le temps sont locaux par défaut.
Deux préférences indépendantes, ticketProvider et timeProvider, sélectionnent le fournisseur des prochaines actions. Changer une préférence ne convertit pas les tickets, ne déplace pas les saisies et ne renvoie aucune écriture distante.

Les paramètres présentent « Tickets », « Suivi du temps » et le gestionnaire « Connecteurs ». Le catalogue commun contient Local, les adaptateurs intégrés Jira/Tempo et les modules de processus installés. Les capacités du manifeste déterminent les catégories disponibles : recherche/lecture/création pour les tickets, triplet timePreview/timeSend/timeReconcile complet pour le temps. Une intégration de dépôts seule ne devient pas une source de tickets.

Jira et Tempo restent des adaptateurs Rust compilés dans Atelier, avec leurs routes et protections existantes. Leurs formulaires suivent immédiatement le choix en cours, avant son enregistrement. Ils ne sont pas présentés comme des exécutables de modules externes. Les modules de processus utilisent le protocole et les formulaires de manifeste existants. Le profil choisi apparaît directement sous le sélecteur. Le gestionnaire reste disponible pour installer et retirer les modules. Ajouter un module externe compatible ne nécessite pas de nouvel onglet ni de code dans les paramètres.

Le choix Local n’exécute aucun module. L’enregistrement des préférences passe par PUT /api/integrations/defaults, séparément des configurations de connexion : aucune consultation du coffre ni validation d’une ancienne adresse Jira ou d’un ancien dépôt. La requête typée contient kind, provider et expected. Dans une même opération du worker SQLite, l’application vérifie la capacité actuellement installée et l’ancienne préférence, puis enregistre un seul document de paramètres. Un conflit conserve le brouillon et retourne HTTP 409.

## Identités et opérations existantes

Les tickets liés conservent connector/instance/id ; les aperçus et journaux de temps conservent leur fournisseur. Les interfaces de reprise Jira/Tempo restent accessibles pour les intentions existantes même lorsque la préférence repasse à Local. Les nouvelles créations Jira et les nouveaux exports Tempo ne sont proposés que pour le défaut correspondant. Le temps en mode Local est indiqué comme enregistré localement.

Le retrait d’un module remet uniquement ses préférences par défaut sur Local, dans la transaction de retrait du registre et des secrets. Les cartes liées sont conservées. Les gardes existantes refusent toujours le retrait d’un module impliqué dans un effet distant inachevé. Un échec de persistance annule le retrait et le changement de préférence ensemble.

## Vérifications

Tests synthétiques ajoutés pour les capacités, choix indépendants, fournisseurs inconnus/incompatibles, conflits, champs HTTP inattendus, conservation des identités et rollback du retrait. Scénarios WebDriver adaptés aux catégories génériques et aux choix de modules. Les anciennes fixtures de paramètres restent historiques ; le test explicite les deux nouvelles valeurs par défaut du contrat courant.

Vérifications locales du 29 octobre : cargo fmt --all -- --check ; cargo check -p atelier-core -p atelier-app -p xtask --tests --locked ; cargo check -p atelier-ui --target wasm32-unknown-unknown --locked (répertoire de compilation séparé integration-check). Compilation native et WASM réussie, avec avertissements de cache de liens physiques Windows. Les cibles de tests sont compilées sans exécution. Tests, Clippy, scénarios navigateur et CI restent non exécutés pour cet incrément. Aucune donnée utilisateur ni compte réel utilisés.


Le choix en cours est distinct du fournisseur enregistré. Passer de Local à Jira, Tempo ou un module externe affiche le formulaire sans modifier le défaut. Le bouton Enregistrer le choix persiste uniquement cette préférence. Les réglages et secrets du module conservent leur propre enregistrement. Les brouillons de connexion bloquent un changement de fournisseur et restent conservés entre les onglets. Le formulaire des modules externes reste monté en une seule instance. Un autre profil ne remplace pas un brouillon en cours. Les erreurs de chargement et les modules retirés sont affichés explicitement. Scénarios de prévisualisation sans enregistrement ajoutés au parcours WebDriver, compilés sans exécution.
