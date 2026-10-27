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
