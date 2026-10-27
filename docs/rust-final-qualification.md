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
