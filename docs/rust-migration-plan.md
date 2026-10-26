# Reconstruction Rust — plan actif

## Décision du 24 octobre 2026

Le propriétaire confirme qu'aucune version n'est en production. L'objectif est de **refaire l'application en Rust, avec les mêmes fonctionnalités et une meilleure qualité**, sans rétrocompatibilité. Cette décision remplace le programme M00–M11 de migration historique.

Le point précédent est conservé au commit `7326122` et dans la branche locale `archive/rust-migration-compat-2026-10-24`. L'[ancien plan](https://github.com/FluryWaana/atelier/blob/7326122/docs/rust-migration-plan.md), les relevés M01–M04 et la matrice de parité sont des références historiques. Leurs résultats restent des preuves des commits cités, pas une qualification du nouveau produit.

Nous conservons les briques Rust utiles et leurs tests de comportement. Il n'est pas nécessaire de reproduire les conversions JavaScript, les unités UTF-16 isolées, les erreurs accidentelles, le schéma SQLite Node, ses enveloppes de jetons ou ses anciens paquets. Les changements de contrat sont assumés et documentés. Aucune fonction utilisateur ne disparaît au prétexte de simplifier le portage.

## Périmètre

L'[inventaire fonctionnel actif](rust-rebuild-features.md) est le suivi d'achèvement. Le code et l'interface Node servent de référence pour découvrir les fonctions, sans imposer leur implémentation ni leurs API. L'[inventaire des sources existantes](rust-migration-inventory.md) aide à ne rien oublier.

Sortent du périmètre : import des anciennes bases, round trips Node/Rust, verrou interopérable entre les deux applications, transition N0/N1/R1, restauration d'une release Node, compatibilité binaire des extensions JavaScript et reproduction exacte des contrats historiques.

Restent requis : persistance fiable, sauvegarde/restauration de la nouvelle application, sécurité locale, protection des secrets, arrêt des processus, intégrations, extensibilité, interface complète, installation et mise à jour entre versions Rust. Les sauvegardes et le rollback Rust restent nécessaires pour éviter les pertes de données du futur produit.

## Architecture retenue

- `atelier-core` : modèles métier typés, validations, cas d'usage, erreurs explicites et contrats publics. Aucun accès disque, réseau, processus ou OS.
- `atelier-app` : composition, serveur HTTP loopback, worker SQLite, Git, processus, connecteurs et adaptateurs natifs.
- `atelier-ui` : interface Leptos CSR/WASM, avec composants et état explicites. Les écrans sont livrés avec leurs cas d'usage, pas dans un dernier portage global.
- `xtask` : génération des assets, tests de parcours, fabrication et contrôle des paquets.

Le workspace, l'outillage verrouillé et les protections déjà présentes sont réutilisés. Pas de nouvelle couche générique sans appelant concret. Les modèles ouverts du prototype sont remplacés lorsqu'un parcours est reconstruit ; conserver tout son moteur de compatibilité n'est pas un objectif.

La nouvelle base porte l'identité SQLite `0x41544b52` (`ATKR`) et la version de format `1`. L'application refuse une base ancienne/inconnue ou une version non prise en charge avec un diagnostic clair ; elle ne l'efface ni ne la migre implicitement. L'initialisation marque uniquement un fichier réellement vide, dans une transaction. Le même contrôle s'applique aux sauvegardes et transferts portables Rust. Ces contrôles passent sous Windows à `411ffb9`, journaux relus ; la qualification POSIX reste différée. Les développements utilisent exclusivement des répertoires synthétiques séparés de `data/`.

SQLite garde une file d'accès bornée et des transactions courtes. Une opération externe réserve son état, libère la transaction, effectue l'I/O puis revalide avant commit. Les jetons ne sont jamais exposés dans les réponses ou journaux ; l'absence de coffre natif ne déclenche pas un stockage en clair. Les programmes enfants ont des flux et délais bornés et un propriétaire chargé de leur arrêt.

L'extensibilité conserve les capacités actuelles (tickets, dépôts, temps, diagnostics, paramètres et actions), via un protocole de processus versionné et un exemple Rust. Aucun moteur Node embarqué ni compatibilité avec les anciens modules JavaScript.

## Ordre des lots et critères de sortie

| Lot | Résultat attendu | Preuve de sortie | État |
| --- | --- | --- | --- |
| R00 | Périmètre corrigé, archive Git, suivi fonctionnel, CI adaptée | Instructions cohérentes, aucun effacement de données, derniers échecs explicités | Validé à `28629e4` |
| R01 | Application autonome : nouvelle base, session, projets, tableau, cartes, modèles, paramètres et navigation | Parcours navigateur complets, redémarrage, concurrence et rollback, erreurs utilisables | Socle validé sous Windows/Edge à `9943165` : réglages, concurrence, sauvegarde/restauration et français/anglais compris ; qualification des autres systèmes/navigateurs réservée à R07, préférences d'exécution avancées en R02/R03 |
| R02 | Espace de travail : Git/worktrees, contexte, fichiers/diffs et serveurs Front/Back | Dépôts synthétiques, confirmation des suppressions, sorties de dossier refusées, processus nettoyés | En cours : autorisations locales de dépôts, explorateur/diffs, worktrees/contexte/dépendances, fetch HTTPS isolé et commits privés Windows/POSIX candidats ; services Front/Back et inspection des ports Windows candidats, services POSIX incomplets ; aucun incrément récent qualifié |
| R03 | Profils, diagnostics, exécution et interactions Codex/Claude/commandes personnalisées | Faux agents compilés, flux importants, arrêt, erreur, reprise, secrets masqués ; aucun appel payant | En cours : profils/coffre/diagnostics, commandes et sessions interactives Codex/Claude Windows, historique, logs/aperçu masqués, annulation et récupération persistante candidats ; mises à jour CLI, usages/quotas avancés et POSIX restent ouverts ; tests différés |
| R04 | Tickets, dépôts, PR, synchronisation et connecteurs intégrés/extensibles | Serveurs HTTP de test, pagination, authentification, erreurs, délais et contrats d’extension | En cours : Jira Cloud/Data Center (configuration, recherche, import/création, transitions, mappings globaux/projet, synchronisation unitaire/en lot) candidat ; PR/MR GitHub/GitLab/Azure, brouillon, contenu, création/push et contrôles/politiques Azure candidats ; import/rafraîchissement de tickets par extensions candidat ; tickets Azure, autres capacités/écritures des modules, agents POSIX et qualification restent ouverts |
| R05 | Brainstorms, préparation/review/publication, file/automatisation, temps et exports | Parcours métier de bout en bout, reprise après arrêt, idempotence et concurrence des effets | En cours : brainstorm, préparation confirmée, review/correction liée aux critères/commits, parcours de projet avec commit/publication confirmés et file persistante Windows candidats ; temps manuel/chronomètre, rapports/CSV et export Tempo avec reprise candidats ; autres exports, parcours POSIX et preuves TLS complètes restent ouverts |
| R06 | Lanceur, tray, installation, sauvegardes, mises à jour et paquets Rust | Vrais paquets de test, lancement/arrêt, update Rust→Rust, échec/rollback sans perte | Payloads/ZIP/signature, lanceur, installation CLI versionnée et update/rollback de code avec journal/écran candidats ; scénarios natifs et navigateur écrits uniquement, aucun paquet réel produit ; wrappers Setup/DEB/RPM, désinstallation, tray/intégration bureau et qualification restent à livrer |
| R07 | Qualification et remplacement final des sources anciennes | Checklist finale complète, matrice native, paquets sans Node, documentation exacte | À faire |

Chaque lot inclut ses écrans, états vides, erreurs, clavier et traductions. Les éléments réutilisés doivent être confrontés au nouveau contrat ; une case n'est pas validée par l'existence d'un fichier. Les fonctions encore absentes sont indiquées clairement et n'affichent aucun succès simulé.

## Qualité attendue

« En mieux » signifie des résultats vérifiables : formulaires et erreurs cohérents, brouillons conservés, navigation clavier, opérations annulables, absence de blocage de l'interface, consommation bornée, persistance et nettoyage fiables. Les gains de performance sont mesurés sur des jeux synthétiques ; Rust seul n'est pas une preuve de gain.

Les tests privilégient les invariants et parcours utiles. Les anciens tests restent utiles lorsqu'ils couvrent ces besoins ; les comparaisons exhaustives avec Node ne sont plus un critère de livraison. Ne pas faire passer une régression de sécurité ou une perte fonctionnelle pour une simplification de contrat.

## Exécution et vérification

- Aucun build, test, Clippy ou navigateur sur le PC du propriétaire. Édition, Git, formatage et inventaire légers autorisés.
- Décision explicite du propriétaire : aucun lancement ni relancement de tests GitHub pendant les implémentations ; une seule vérification à la fin. Le workflow Rust ne se déclenche plus sur push/PR et reste disponible manuellement. La vérification finale regroupe les contrôles natifs, WASM et navigateur ; Linux/macOS restent différés jusqu'à ce point.
- Données synthétiques uniquement ; aucun compte réel, appel payant, tag ou publication de release.
- Commits Conventional Commits en anglais et push des incréments autorisés, sans déclenchement de CI. Les nouveaux incréments restent non vérifiés jusqu'à la campagne finale ; les anciens journaux peuvent être consultés sans relancer de tests.
- Le code Node et les anciens outils sont retirés de la branche active lorsque leur fonction est couverte dans la reconstruction, ou archivés comme référence. Leur runtime n'entre jamais dans les paquets Rust.

## Checklist finale

Point candidat supplémentaire : registre/exécuteur de connecteurs par processus, réglages bilingues et import local confirmé de tickets paginés raccordés, avec portée tenant/module, rapport/projet exacts, rollback et conservation des cartes humaines. Scénarios purs/SQLite/navigateur uniquement écrits, aucun compilé/exécuté. Les autres capacités des modules, écritures/reprises, POSIX et livraison restent ouvertes ; aucune case ci-dessous ni lot supplémentaire n’est validé. Campagne finale non lancée.

- [ ] Chaque ligne de l'inventaire fonctionnel a un parcours réel, ses erreurs et une preuve au commit livré.
- [ ] Tous les écrans sont utilisables : clavier, focus, brouillons, navigation, traductions, états vides/occupés/erreur.
- [ ] Redémarrage, transactions, sauvegarde/restauration et secrets vérifiés ; aucune perte silencieuse.
- [ ] Processus et descendants qualifiés, y compris arrêt, annulation et limites réelles documentées.
- [ ] Connecteurs et workflows résistent aux erreurs, reprises et effets concurrents sans double publication/export.
- [ ] Windows x64, Linux x64/ARM64 et macOS x64/ARM64 qualifiés sur runners/comptes dédiés.
- [ ] Navigateurs et bureaux réellement exécutés, sans présenter un test configuré ou ignoré comme réussi.
- [ ] Paquets/installeurs/lanceur/tray et update/rollback Rust testés sur de vrais artefacts.
- [ ] Application construite et utilisée sans runtime applicatif Node ; dépendances de fabrication explicitement recensées.
- [ ] Documentation, commandes, licences, inventaire et résultats CI correspondent au produit livré.
- [ ] Aucun tag ni publication effectués dans ce travail ; seule la préparation des artefacts est autorisée.

La reconstruction reste en cours tant que cette checklist n'est pas satisfaite.

Reprise du 25 octobre : modifications HTTPS non commitées de `be8793d` conservées et complétées (portée exacte, refus des caractères de contrôle avant parsing, proxy désactivé, traductions et scénarios synthétiques). Fetch authentifié Windows candidat, sans build/test/Clippy/navigateur/CI. Voir le point de contrôle fonctionnel pour les limites ; aucun lot supplémentaire ni élément de la checklist finale n’est déclaré validé. Le workflow Rust reste exclusivement manuel ; la campagne finale n’a pas été lancée.

Suite de reprise : première review de PR/MR raccordée au moteur et à son panneau, figée sur le besoin et les commits source/cible/base vérifiés. Revalidation explicite après exécution, sans validation automatique des critères. Scénarios écrits uniquement ; les autres workflows, forges, plateformes et paquets restent à terminer avant la campagne finale.

Suite du 25 octobre : commit local explicite candidat Windows, du choix des fichiers à un aperçu immuable puis déplacement conditionnel de branche, avec index humain conservé et reprise persistante sans rejeu. Panneau/traductions et scénarios Git/SQLite/brouillon navigateur écrits ; aucun compilé ni exécuté. Les limites de sélection, plateformes et parcours navigateur heureux sont suivies dans l’inventaire fonctionnel. Aucun lot ni élément de la checklist finale n’est validé par cet incrément ; campagne finale toujours non lancée.

Suite temps : réglages régionaux/coffre Tempo et export Jira Cloud→Tempo candidats, aperçu du compte/ticket, réservation avant tentative unique et relecture des données, reprise sans rejeu et historique des décisions manuelles conservé. Scénarios HTTP/SQLite et navigateur sur fixtures écrits uniquement ; aucun exécuté. Les connecteurs restants, attributs d’équipe et qualification TLS/navigateur complète restent suivis dans l’inventaire ; R05/R06/R07 et la checklist finale ne sont pas validés.

## Journal R00

Le point archivé `7326122` est entièrement vert en CI Windows, journaux relus : la correction des chemins synthétiques passe, ainsi que les nouveaux scénarios de processus. Les instructions, le README et les références d'architecture distinguent désormais le produit cible du code Node historique. Le workflow Rust conserve Clippy, les tests Rust, les preuves natives Windows et le parcours WASM ; il ne reconstruit plus les paquets Node ni les comparateurs historiques à chaque push. Les outils de référence restent disponibles. La [CI de `28629e4`](https://github.com/FluryWaana/atelier/actions/runs/40121046698) confirme les deux jobs Windows : journaux natif et Edge relus, Clippy natif/WASM et tests Rust sans échec, isolation DPAPI entre comptes vérifiée, création/redémarrage/persistance/rotation de session dans le navigateur réussis. R00 est validé ; aucune qualification produit ni POSIX n'en découle.

Point candidat — actions distantes de connecteurs : aperçu immuable, confirmation liée à sa version, réservation avant lancement, identité de requête renouvelée et rapprochement en lecture seule sans renvoi après incertitude. Résultat déclaré appliqué/non tenté/inconnu distinct du statut protocolaire ; aucun simple OK ne confirme un effet. Protections des configurations, suppressions, sauvegardes, restaurations et mises à jour, journal borné sans purge et interface bilingue. Scénarios SQLite écrits seulement ; aucun build/test/Clippy/navigateur/workflow exécuté. Méthodes typées de tickets/dépôts/temps, scénario positif du processus natif et POSIX encore ouverts ; aucune qualification supplémentaire.
Précision de périmètre du 26 octobre : la référence Node Azure ne fournit que les opérations de dépôts/PR Azure Repos. Les tickets Azure Boards ont été ajoutés à tort comme exigence générale à partir du regroupement « Jira/Azure ». Le propriétaire a autorisé la poursuite de l’incrément déjà engagé ; cela n’ajoute pas de transitions Azure ni d’autres nouvelles capacités aux critères de livraison. La lecture déjà poussée et la création ci-dessous restent des ajouts candidats, pas des preuves de remplacement d’une fonctionnalité Node.

Création de tickets par connecteur candidate : formulaire type/titre/description, aperçu figé et journal commun étendu à ticketsCreate avec entrée typée, enveloppe/méthode et reprise exactes. Azure utilise le projet UUID, JSON Patch sans contournement des règles, description texte échappée et tag unique d’opération. Préflight par tag, tentative unique puis relecture du projet/type/tag/texte ; un rapprochement sans résultat reste incertain. Ni création locale automatique ni répétition après réponse perdue. Une édition humaine du contenu ou du marqueur bloque la confirmation plutôt que d’être remplacée. Scénarios HTTP synthétiques écrits pour réponse perdue, observation sans deuxième écriture, résultat absent/ambigu/étranger/modifié et scénario SQLite d’entrée typée figée ; non compilés/non exécutés. Parcours navigateur positif encore non écrit pour cette création. Aucun lot qualifié et campagne finale non lancée.
Point candidat R06 — sélection de dossier : bouton dans les autorisations locales de dépôts, ouverture explicitement demandée, chemin reporté sans autorisation implicite, confirmation d’accès retirée après sélection et brouillon conservé après annulation/erreur. Helper Atelier distinct lancé avant toute ouverture des données, sans arguments injectés ni environnement de jetons ; capture/délai/arrêt du propriétaire existants. Windows utilise le dialogue Shell/COM sur son thread STA et libère le PIDL ; macOS osascript et Linux Zenity reprennent les sélecteurs de référence, sans runtime Node. Les chemins restent validés par le registre avant toute autorisation effective.

Scénarios de validation de sortie/cancel, refus HTTP sans session/prototype et refus des arguments du helper avant ouverture de données écrits seulement. Aucun dialogue natif, test ou build exécuté ; interactions graphiques positives, focus réel, nettoyage et dépendance Zenity restent à qualifier sur les bureaux de la campagne finale. Fonction disponible depuis l’écran d’autorisations ; pas de nouveau parcours d’installation ajouté par cet incrément.