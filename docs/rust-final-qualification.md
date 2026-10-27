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
