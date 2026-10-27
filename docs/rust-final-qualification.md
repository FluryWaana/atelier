# Campagne finale Rust

Première exécution finale : [40423799275](https://github.com/FluryWaana/atelier/actions/runs/40423799275), déclenchée manuellement le 27 octobre 2026 sur `1ba8e3b56825af9d9fef6f212bf47462503eaa32`, avec `full_matrix=true`.

| Périmètre | État observé |
| --- | --- |
| Formatage local et génération légère de l'inventaire | Commandes exécutées avec code de sortie 0 avant commit ; aucune qualification fonctionnelle |
| Matrice native Windows x64, Linux x64/ARM64, macOS x64/ARM64 | Échec des cinq jobs à Clippy sur `extension_repositories.rs:121` (`collapsible_if`), avant tests |
| WASM et WebDriver Edge/Chrome/Firefox/Safari | Échec des trois jobs navigateur pendant la compilation d'atelier-app ; aucun parcours exécuté avec succès |
| Fabrication et qualification des artefacts natifs | Job delivery ignoré après échecs des dépendances ; aucun paquet qualifié |
| Interactions manuelles macOS tray/sélecteur et sélecteur Linux | Preuves de bureau manquantes ; protocole et limites dans [les paquets Rust](rust-packages.md) |
| Retrait physique final des références Node | Différé jusqu'à qualification ; exclusion des payloads implémentée mais artefacts encore non vérifiés |

Les résultats doivent être relus dans les vrais journaux et associés au commit exécuté. Un job configuré, ignoré, annulé ou bloqué ne constitue pas une réussite. Les éventuelles corrections et leur vérification seront consignées ici sans effacer les échecs initiaux. Aucun tag, release ou déploiement.

## Diagnostics de la première exécution

Les journaux natifs et navigateur ont été téléchargés et relus. La préparation de Git OpenSSL macOS a réellement réussi sur ARM et Intel ; cela ne prouve pas les scénarios TLS, non atteints.

La compilation navigateur signale les frontières de visibilité de fonctions utilisées par HTTP (`forge::configuration`, `task_creation::path_string`, fonctions du journal CLI Windows), la possession de l'état dans la mutation détachée, l'import POSIX `Ipv4Addr`, une tranche `str` passée à `Document::set` et le format `LowerHex` absent des sorties SHA de la dépendance verrouillée. Les correctifs réutilisent l'encodeur hexadécimal existant, conservent les SHA et leur représentation, et ajoutent un vecteur connu. Une condition imbriquée et des imports/mutabilités inutiles sont corrigés sans neutraliser Clippy. Ces corrections restent à requalifier ; aucun test ni contrôle lourd local.

## Corrections et blocage de la requalification

Les corrections sont commitées et poussées à `9959d31`. Formatage et génération légère de l'inventaire exécutés avec code de sortie 0 ; aucun build, test, Clippy ou scénario local. Cela ne prouve pas la compilation ni la réussite des correctifs.

La tentative de déclenchement du workflow manuel sur les corrections a été **rejetée avant exécution par le contrôle automatique d'approbation** : la consigne autorisait une seule campagne finale, déjà lancée ; une relance complète est considérée comme une seconde campagne coûteuse exigeant un accord explicite du propriétaire. Aucune seconde exécution n'a été créée et aucun contournement ni relance indirecte n'est autorisé.

Intervention nécessaire : autorisation explicite de requalifier les corrections via le workflow manuel complet. En attendant, tous les tests natifs/TLS, parcours navigateur, artefacts/installations/mises à jour restent sans preuve de réussite ; les interactions de bureau manuelles et le retrait physique final des références Node restent également ouverts. La reconstruction n'est pas terminée.

## Requalification autorisée

Après accord explicite du propriétaire (« go »), le workflow complet a été déclenché sur `7f48996` (code corrigé à `9959d31`) : [40424550305](https://github.com/FluryWaana/atelier/actions/runs/40424550305). Matrice complète, contrôles conservés, aucune exécution lourde locale. Résultats en attente ; aucune réussite supplémentaire revendiquée.

Les trois jobs navigateur échouent ensuite sur l'import manquant de `plain_metadata` dans `xtask/src/package.rs` : les anciennes erreurs de compilation atelier-app n'apparaissent plus, mais aucun parcours WebDriver n'est atteint. Les jobs natifs terminés signalent désormais Clippy dans atelier-app : conditions imbriquées, emprunts inutiles, découpage de tranches et position d'un module de tests ; `input<T>` nécessite aussi une erreur de taille réduite. Les correctifs conservent les contrôles et utilisent `Box<Response>` avec propagation aux 39 branches appelantes. Aucun lint désactivé. Formatage et inventaire légers uniquement exécutés sur le PC ; correctifs non requalifiés. Les jobs encore en cours et la livraison non atteinte ne sont pas considérés réussis.

Les correctifs de cette exécution sont poussés à `32a6a8f`. Les trois jobs navigateur et quatre jobs natifs ont terminé en échec. Après une longue installation des prérequis, Linux x64 a atteint Clippy puis a été annulé lors de la demande d'arrêt de l'exécution déjà en échec. Le statut final global est `cancelled` ; la livraison a également été annulée sans produire de preuve. Aucun de ces statuts ne constitue une réussite. L'autorisation des prochaines relances correctives est demandée explicitement ; aucun nouveau workflow n'est lancé en attendant.

Le propriétaire a ensuite explicitement autorisé les relances correctives nécessaires jusqu'au passage des contrôles ou à un blocage humain, sans ajout de fonctionnalités ni réduction des contrôles. Exécution suivante : [40425548838](https://github.com/FluryWaana/atelier/actions/runs/40425548838) sur `9327068` (code `32a6a8f`), matrice complète. Aucun résultat acquis au déclenchement.

Cette exécution atteint la compilation des cibles de test natives et du binaire Leptos/WASM. Les diagnostics natifs concernent deux conversions `JsonString` dans `local-commit`, une valeur empruntée puis déplacée dans `native-tray`, une mutabilité inutile dans `native-import` et trois diagnostics Clippy des scénarios UI. Les journaux navigateur révèlent des erreurs de syntaxe de macros dans Jira/forge, une collision `Effect`, une dépendance serde directe manquante, la contrainte d'égalité d'un mémo de cartes, des appels Option sur Result et des valeurs consommées deux fois dans des vues. Les correctifs sont limités à ces causes ; aucun contrôle n'est désactivé. La dépendance serde utilise la version déjà verrouillée, sans résolution. Aucun scénario produit n'est encore déclaré réussi.

Les relances intermédiaires peuvent utiliser la matrice Windows existante (`full_matrix=false`) pour limiter les coûts de découverte des erreurs communes de compilation/WASM. Elles ne qualifient jamais les autres plateformes : une exécution complète reste nécessaire pour la checklist finale.

`40425548838` termine en échec ; livraison ignorée. Correctifs committés et poussés à `7ab8da8` (formatage/inventaire seulement locaux). Vérification intermédiaire Windows/WASM : [40426153120](https://github.com/FluryWaana/atelier/actions/runs/40426153120), `full_matrix=false`, sans réduction des exigences finales. Résultats en attente.

`40426153120` termine en échec. Windows natif signale quatre closures redondantes dans `time-tracking.rs:212–215`, corrigées. Le journal navigateur confirme la compilation native et Leptos/WASM debug, puis le refus « Fichier trop volumineux » au démarrage du serveur ; aucun parcours produit n'est déclaré réussi. Le build UI était encore en debug : il passe en release dans la commande Trunk existante, et le refus de lecture d'asset indique désormais son nom. La borne de 64 Mio par asset reste inchangée. Ces correctifs doivent encore être exécutés.

Le cache Windows natif/navigateur conserve désormais les compilations partielles avec `actions/cache/save` au même SHA, mêmes chemins et clés. Le code de l'action épinglée a été lu : sa sauvegarde automatique exige un job réussi et son option `save-always` est explicitement déclarée non fonctionnelle. Les contrôles, résultats et artefacts ne sont pas modifiés ; le gain réel reste à observer. Les 22 outils de lancement/livraison Node remplacés sont archivés à l'identique à `a27b376`, sans déplacement des sources/fixtures utilisées par Rust.

## Exécution sur `0138593`

[40427503293](https://github.com/FluryWaana/atelier/actions/runs/40427503293), Windows/WASM intermédiaire : échec global, livraison ignorée. Les journaux confirment Clippy natif terminé, puis les 118 tests de bibliothèque atelier-app : **115 réussis, 2 échoués, 1 ignoré**. Ce résultat ne couvre pas les autres cibles de tests que Cargo n'a pas atteintes après l'échec.

La restauration échoue sur une ancienne assertion qui omet l'invalidation des révisions Tempo/modules ; le test est renforcé pour contrôler séparément révisions supérieures aux états sauvegardé et courant, réglages restaurés, coffre vidé et secours conservé. La production n'est pas modifiée pour satisfaire cette ancienne attente. L'autre échec est une expiration de la lecture de santé loopback pendant le scénario de récupération. Le correctif partagé limite les reprises à trois GET de preuve sans identifiants ; un timeout persistant reste une erreur, jamais une preuve d'absence. Aucune lecture authentifiée ni mutation n'est renvoyée automatiquement. Régressions réseau synthétiques écrites, non exécutées à ce stade.

Le build Leptos/WASM release réussit et le serveur/browser démarrent. Le parcours échoue sur la première découverte du module : la capture `rust-ui-failed.png` montre « Champ de connecteur invalide ou répété. ». Le faux module utilisait l'identifiant interdit `fixtureRoot`, corrigé en `fixture-root` dans les déclarations/configurations/sélecteurs, sans assouplir le contrat de production ni l'assertion. Ce n'est pas une preuve du parcours complet. Les deux caches de compilation partielle ont réellement été sauvegardés, d'après les journaux.

Le propriétaire demande ensuite le retrait complet des sources de l'ancien projet et une racine organisée autour de Rust. Sources/archives Node et comparateurs retirés, fixtures JSON utiles relogées dans les crates sans modification de contenu, pont legacy/CLI `serve-prototype` retirés ; les parcours navigateur utilisent désormais le vrai `serve`. L'inventaire est remplacé par `cargo xtask inventory [--check]`, sans Node. La génération légère locale du document utilise un équivalent PowerShell, sans compiler Rust. Ces changements doivent encore être qualifiés. Les fichiers locaux ignorés et les données utilisateur ne sont pas effacés.

À la demande de simplification du lancement local, `Preview.cmd` et `scripts/preview.ps1` préparent un aperçu Windows avec outils locaux, compilation séquentielle limitée et données distinctes sous `.artifacts/local-preview/data`. Le script est relu mais **n'a pas été exécuté** ; aucune autorisation d'exécuter un build ou test sur le PC n'est déduite de son écriture.

## Matrice complète sur `66acf34`

Une tentative de relance intermédiaire réduite a été rejetée avant création par le contrôle automatique. Après vérification de l'accord explicite déjà donné et du dépôt propre, la matrice complète a été autorisée : [40431199519](https://github.com/FluryWaana/atelier/actions/runs/40431199519). Résultat global : **échec**, livraison ignorée.

| Cible native, bibliothèque atelier-app | Réussis | Échoués | Ignorés | Diagnostic réellement lu |
| --- | --- | --- | --- | --- |
| Windows x64 | 116 | 1 | 1 | Expiration de santé sur port libre pendant récupération |
| Linux x64 | 118 | 5 | 1 | Descripteurs de dossiers `O_PATH` utilisés pour synchronisation/permissions |
| Linux ARM64 | 118 | 5 | 1 | Mêmes erreurs de descripteurs, journaux ARM64 relus |
| macOS ARM64 | 104 | 9 | 1 | Alias `/var`, SQLite NOFOLLOW et observation de processus Darwin |
| macOS x64 | 104 | 9 | 1 | Mêmes familles, journal Intel relu |

Les trois jobs navigateur refusent le fichier `.trunk-lock` laissé par Trunk dans les assets. La livraison, les autres suites arrêtées après l'échec de bibliothèque et les contrôles ultérieurs ne sont pas qualifiés par ces nombres.

Correctifs candidats : listing d'assets partagé excluant uniquement le marqueur Trunk régulier vide sans lien, sans le supprimer ni le servir ; descripteur de dossier rouvert relativement au FD ancré pour `fsync` et `fchmod` ; observation Darwin par `proc_pidinfo` gardant le leader possédé non récolté ; racines temporaires physiques dédiées dans la qualification macOS et dans le helper de snapshots SQLite de production. Les validations des chemins utilisateur ne sont pas assouplies. Régressions écrites, résultats à requalifier.

L'essai local manuel signalé par le propriétaire échoue également avant le lancement du serveur sur la lecture de santé d'un port libre. Une lecture OS constate l'absence de listener et de dossier de données d'aperçu : aucun contenu métier n'est consulté. Le correctif partagé prouve d'abord la disponibilité du port par une réservation noyau brève ; seul `AddrInUse` autorise la tentative HTTP. L'attente du seul enfant lancé reste bornée à vingt secondes et ne reprend que les expirations de lecture. Aucun processus étranger n'est arrêté et aucun timeout n'est assimilé à une absence.

## Compilations locales désormais autorisées

Le propriétaire a explicitement levé les limites de ressources et autorisé les builds sur le PC. Les tests, Clippy et scénarios automatisés locaux n'ont pas été autorisés ni exécutés. `Preview.cmd` ne limite plus les jobs, la priorité ou l'affinité CPU.

Le premier build local des cibles natives détecte une visibilité insuffisante de `single_link`, corrigée en portée interne à la crate. Ensuite `cargo build --workspace --exclude atelier-ui --all-targets --locked` réussit (3 min 50 s) et `cargo check --workspace --exclude atelier-ui --all-targets --locked` réussit (1 min 01 s). Ce sont des preuves de compilation Windows, y compris des exécutables de tests compilés sans exécution. Des avertissements de copie du cache incrémental après échec de hardlink sont présents ; ils ne sont pas présentés comme des diagnostics de tests réussis.

Le premier build UI dev sans symboles de débogage compile mais produit un WASM de 277 613 094 octets, inutilisable dans la borne de 64 Mio : il n'est pas qualifié. Après profil incrémental légèrement optimisé, destination dev distincte et conservation des noms Rust encodés (`data-no-demangle` de Trunk épinglé), `cargo xtask build-ui --dev` réussit avec **20 412 294 octets**, compilation UI de 2 min 27 s ; une relance sans changement est mesurée à **4,16 s**. La commande contrôle les bornes après génération. Ces chiffres n'isolent pas chaque changement et ne mesurent ni reconstruction après édition ni scénario d'utilisation. Le build release reste distinct et requis pour la livraison.

La commande Rust compilée `xtask inventory --check` a également réussi localement. Aucun compte réel, appel payant, accès au `data/` utilisateur, tag, release ou déploiement n'a été utilisé pour ces vérifications.

Correctifs poussés : `807b40d` (démarrage), `69890ab` (adaptateurs POSIX), `4b80a73` (assets et aperçu). Requalification complète autorisée : [40436598406](https://github.com/FluryWaana/atelier/actions/runs/40436598406) sur `4b80a73`. Résultats en attente ; les compilations locales ne remplacent aucun des parcours de cette matrice.

## Résultats réels sur `4b80a73`

La campagne `40436598406` se termine en **échec**, livraison ignorée. Bibliothèque atelier-app : Windows **120 réussis, 0 échoué, 2 ignorés** ; Linux x64 et ARM64 **129 réussis, 0 échoué, 2 ignorés** ; macOS x64 et ARM64 **115 réussis, 4 échoués, 2 ignorés**. Les anciens échecs des descripteurs de dossiers, chemins temporaires et démarrage sur port libre ne réapparaissent pas dans ces suites. Ces nombres ne couvrent pas les cibles suivantes arrêtées par Cargo.

Les quatre échecs macOS concernent désormais `killpg` sur un groupe ne contenant que des zombies : Darwin renvoie `EPERM`. Le correctif conserve le refus pour un groupe vivant ou une inspection indisponible ; il exige une nouvelle preuve du leader zombie possédé et une inspection complète sans membre vivant. Sous Windows, la suite s'arrête ensuite sur l'association de forge : deux formes du même chemin (`\\?\` et ordinaire) étaient comparées sans normalisation commune. Sous Linux, elle s'arrête sur la sauvegarde directe pendant une création Jira ; le garde Jira existant est désormais aussi appliqué à cette frontière, avant création de fichier. Régressions conservées et renforcées, requalification requise.

Chrome et Edge atteignent l'import de ticket : la carte est réellement créée, mais la désérialisation UI depuis `Value` échoue pour `Card/Document`. Le parsing depuis texte JSON corrige cette frontière sans changer l'assertion du parcours. Safari s'arrête auparavant sur la conservation d'un brouillon de forge après conflit ; la cause n'est pas encore prouvée. Le diagnostic d'échec recueille désormais uniquement les états publics synthétiques des contrôles concernés, et les captures sont conservées pour les trois plateformes. Aucun de ces parcours incomplets n'est déclaré réussi.

La campagne corrective poursuit toutes les cibles Cargo avec `--no-fail-fast` afin de recueillir les échecs indépendants dans la même exécution ; le code de sortie reste en échec si une seule cible échoue. Les étapes suivantes et la livraison restent conditionnées aux contrôles réussis.

Les correctifs natifs de ce lot compilent localement sous Windows avec toutes les cibles (`cargo build --workspace --exclude atelier-ui --all-targets --locked`, 3 min 12 s), sans exécution de test. Darwin reste à requalifier sur ses runners.

Le contrôle de compilation UI `cargo check -p atelier-ui --target wasm32-unknown-unknown --locked` réussit également localement (25,15 s), ainsi que le formatage et le contrôle d'inventaire. Aucun parcours ni test n'est déduit de ces contrôles.

## Collecte complète sur `9d787e8`

[40438226602](https://github.com/FluryWaana/atelier/actions/runs/40438226602) : **échec**, livraison ignorée. L'association de forge Windows passe ses trois tests ; les gardes Jira corrigés ne réapparaissent pas comme échecs. Bibliothèque atelier-app : Windows 120 réussis/0 échoué/2 ignorés, Linux x64 et ARM64 129/0/2, macOS ARM64 118/1/2. Les tests d'observation/arrêt Darwin passent ; le test de service encore échoué exigeait un PID enfant alors que le shell macOS remplaçait son propre processus par le dernier programme. La fixture conserve désormais explicitement son shell parent.

`--no-fail-fast` expose 11 cibles échouées sous Windows, 7 sur chaque Linux et 10 sur macOS ARM64. Les correctifs traitent séparément : fixtures initialisées avant leur contenu, chemins Git Windows sans préfixe étendu, vraie traversée lexicale refusée, accès synthétique des commits sous `repositories`, contrôle de persistance d'une conversation dans `brainstorms`, accès aux dépôts hors périmètre nécessitant une autorisation, export Tempo exigeant son instantané, rejet initial d'assets interdits avant toute session et isolation des tests qui partagent le résolveur borné. Les anciens vecteurs JSON restent inchangés ; les vecteurs de modification locale de texte lié à un ticket vérifient explicitement le refus du nouveau contrat et l'absence d'événement.

Défauts de production corrigés dans ce lot, encore à requalifier : conservation des pipes jusqu'au nettoyage de l'enfant supervisé ; instantanés de module hachés après normalisation JSON pour ignorer uniquement l'ordre/espacement des objets ; `check-ignore` exempté du drapeau global `--literal-pathspecs` qu'il refuse, en gardant les chemins validés et les autres commandes littérales. Le [code de Git](https://github.com/git/git/blob/master/builtin/check-ignore.c) refuse cette magie via son masque de pathspec. Les hooks restent explicitement neutralisés avec `NUL` ou `/dev/null` selon la plateforme. Neuf traductions explicites manquantes sont ajoutées.

Chrome/Edge passent l'import puis échouent après le rapprochement d'une transition, lors de la fermeture du dialogue ; le scénario restaure une valeur vide par WebDriver `clear`, qui ne notifie pas uniformément le modèle via `input`. La notification du champ vidé est explicite et l'absence réelle du dialogue est attendue avant navigation. Les clics attendent aussi que leur contrôle soit activé, sans contourner le clic natif ni les gardes. Safari dépasse cette fois le conflit de forge et s'arrête sur la duplication ; aucune cause unique n'est déclarée prouvée, l'attente de contrôle activé et les diagnostics d'éditeur doivent être relus. L'upload des captures est déplacé après tous les scénarios, car il précédait les étapes Linux/macOS.

`cargo check --workspace --exclude atelier-ui --all-targets --locked` réussit localement sur le lot correctif (20,61 s), sans exécution. Les suites échouées et les paquets restent non qualifiés.

Le journal macOS Intel confirme les mêmes dix cibles, plus un arrêt interactif non confirmé dans le délai synthétique de quinze secondes (onze cibles au total). L'ordre de nettoyage garde maintenant les lecteurs jusqu'à la tentative d'arrêt ; toute erreur d'arrêt reste propagée et la réservation conservée. Le délai et les assertions de résultat restent inchangés ; le prochain échec éventuel indiquera type/mode/statut/attention. Les bibliothèques macOS Intel et ARM64 ont chacune 118 réussites, un échec de fixture service et deux tests ignorés.

Après ces dernières corrections, le contrôle natif de toutes les cibles réussit à nouveau (15,25 s). Son premier essai avait détecté un format de diagnostic `Debug` non pris en charge, corrigé dans le test avant ce succès. Le binaire local a été reconstruit (30,59 s avant le dernier changement interactif) ; le build UI dev après modification réussit en 1 min 32 s de compilation, avec un WASM de 20 409 464 octets. Aucun test, Clippy ni navigateur n'a été exécuté sur le PC.
