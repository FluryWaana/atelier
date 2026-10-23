# M01 — première tranche Rust exécutable

Base : `ec6dbe4` (M00 commis), tranche verticale commise dans `c161512`, puis prototypes Windows. Incréments du 23 octobre 2026. **M01 en cours, pas de bascule de production.** Le workspace et le parcours vertical fonctionnent localement sur Windows x64 MSVC. La validation native complète reste ouverte.

## Périmètre livré

Quatre crates : `atelier-core` porte validation du titre et contrat JSON extensible ; `atelier-app` assemble HTTP, SQLite sur thread dédié avec file bornée, session et verrou de dossier ; `atelier-ui` fournit le formulaire Leptos CSR ; `xtask` construit et teste le parcours. Les tests de dépendances empêchent les adaptateurs natifs de rejoindre core/UI. Les frontières internes restent également à relire lors des lots suivants.

La seule commande produit Rust disponible est `atelier serve-prototype --data-dir DOSSIER --assets .artifacts/rust-ui [--port 4318]`. Elle exige un dossier vide au premier démarrage, puis son marqueur `.atelier-rust-prototype`. Elle refuse un dossier Node existant. Deux instances Rust ne peuvent pas ouvrir le même dossier, y compris via `.` ; M02 a remplacé le verrou initial par la coordination SQLite commune avec l'adaptateur N1. Son intégration dans la livraison Node de transition reste en M09.

L'API du prototype propose `GET /api/health`, `GET /api/state`, `POST /api/cards` (titre seul) et `POST /api/prototype/stop`. Le tableau renvoyé ne contient que les cartes. Les autres contrats métier restent à porter. Les champs de création non pris en charge sont refusés explicitement. Les documents JSON persistés conservent les champs additionnels et null. Aucune compatibilité complète de la base Node n'est revendiquée.

HTTP écoute sur loopback, contrôle Host/Origin, authentification Bearer, preuve HMAC, mutations JSON et taille. La preuve HMAC possède un vecteur provenant de `sessionProof` Node. La session change à chaque lancement. Les assets sont limités aux fichiers générés reconnus. La CSP ajoute uniquement `'wasm-unsafe-eval'` à `script-src 'self'` ; pas de `'unsafe-eval'` ni `'unsafe-inline'`. `xtask` externalise le chargeur produit par Trunk et refuse un format inattendu. Le code JavaScript de liaison reste généré.

Le compte Windows chiffre la session par DPAPI directement depuis Rust. Le test compare les deux directions avec les mêmes commandes PowerShell que `windowsProtect`, sur un secret synthétique. Ce test ne valide ni les trousseaux POSIX ni le comportement sous un autre compte. Les sessions POSIX suivent le format Node en fichier privé ; ce fichier n'est pas le coffre des jetons d'intégration.

## Outils et dépendances

Toolchain Rust `1.99.1`, édition 2024, cible WASM `wasm32-unknown-unknown`, Trunk `0.22.0`, wasm-bindgen CLI `0.2.129`. Les dépendances directes sont épinglées et les transitives dans `Cargo.lock`. Source de version : [distribution Rust](https://static.rust-lang.org/dist/channel-rust-1.99.1.toml), [Trunk 0.22.0](https://github.com/trunk-rs/trunk/releases/tag/v0.22.0), [wasm-bindgen 0.2.129](https://github.com/wasm-bindgen/wasm-bindgen/releases/tag/0.2.129).

| Usage | Dépendances directes et justification |
| --- | --- |
| Contrats | Serde 1.0.229 / serde_json 1.0.154 : préserver le JSON et les champs additionnels |
| Serveur | Axum 0.8.9 / Tokio 1.54.1 / tower-http 0.7.1 : routage, arrêt asynchrone et fichiers statiques |
| Données | rusqlite 0.40.2 avec SQLite embarqué ; time 0.3.55 pour dates UTC ISO avec millisecondes |
| Session | getrandom 0.4.3, hmac 0.13.0, sha2 0.11.1, subtle 2.6.1 ; aucune primitive réécrite |
| Système | uuid 1.29.0 pour identifiants ; windows-sys 0.61.2 pour DPAPI ; verrou par la bibliothèque standard |
| UI | Leptos 0.8.22, gloo-net 0.7.0, web-sys 0.3.106, wasm-bindgen 0.2.129 et futures 0.4.79 |
| Tests | tempfile 3.27.0, reqwest 0.13.5 et Fantoccini 0.22.1 : dossiers isolés, requêtes locales et WebDriver depuis Rust |

Métadonnées des licences directes vérifiées via crates.io : MIT et/ou Apache-2.0, sauf subtle BSD-3-Clause ; projet AGPL-3.0-or-later conservé. L'audit complet des dépendances transitives, avis de sécurité et notices distribuées reste à rendre bloquant avant livraison. Le navigateur et son pilote doivent avoir des versions compatibles ; leurs versions sont relevées à chaque exécution, pas figées indépendamment des mises à jour du navigateur.

## Reproduire

Prérequis Windows : Rust/rustup, Build Tools C++ avec MSVC et SDK Windows. Ici, Build Tools 18.11.1 et SDK 10.0.26100.0 ont été détectés. Rust, Trunk, wasm-bindgen et EdgeDriver ont été installés dans `.artifacts/rust-tools`, sans changement du PATH système. Pour cette installation locale dans PowerShell :

```powershell
$env:CARGO_HOME = Join-Path (Get-Location) '.artifacts/rust-tools/cargo'
$env:RUSTUP_HOME = Join-Path (Get-Location) '.artifacts/rust-tools/rustup'
$env:PATH = "$env:CARGO_HOME/bin;$(Get-Location)/.artifacts/rust-tools/trunk;$(Get-Location)/.artifacts/rust-tools/wasm-bindgen-0.2.129-x86_64-pc-windows-msvc;$(Get-Location)/.artifacts/rust-tools/edgedriver;$env:PATH"
```

Avec une installation habituelle, mettre les outils épinglés sur PATH (`cargo install --locked trunk --version 0.22.0` et `cargo install --locked wasm-bindgen-cli --version 0.2.129`). Puis :

```text
cargo fetch --locked
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
cargo xtask build-ui
cargo xtask test-ui
```

`build-ui` est hors ligne après préparation du cache et exige les versions exactes des deux outils. Les captures sont `.artifacts/rust-ui-created.png` et `.artifacts/rust-ui-reopened.png`, exclusivement synthétiques. Le test navigateur démarre WebDriver avec un profil temporaire, ouvre une session par le fragment URL (jamais en argument de processus), crée la tâche, attend le DOM, arrête le processus serveur, le redémarre, refuse l'ancienne session et relit la tâche. Il ferme les processus qu'il a créés, y compris en échec. Il ne touche pas au profil navigateur personnel.

`ATELIER_WEBDRIVER` sélectionne le binaire ; `ATELIER_TEST_BROWSER` vaut `edge`, `chrome`, `firefox` ou `safari`. Safari nécessite la permission native d'automatisation. La présence de ces options n'est pas une preuve d'exécution sur ces navigateurs.

## Preuves et travaux restants

Résultats locaux : compilation Windows MSVC et WASM réussie ; tests domaine, HTTP, SQLite, DPAPI, CSP et isolation réussis ; parcours Edge 155.0.4283.62 avec EdgeDriver de même version réussi. Captures relues : titre Unicode/texte HTML affiché comme texte, formulaire et carte lisibles après redémarrage. Les résultats finaux des commandes et de la suite Node sont consignés dans le suivi du plan.

Le workflow `rust.yml` ajoute les cinq runners natifs et un parcours Chrome Linux ; il ne remplace aucun contrôle Node et ne publie rien. Les exécutions sur [371a1a9](https://github.com/FluryWaana/atelier/actions/runs/40031351769) et [52dcb43](https://github.com/FluryWaana/atelier/actions/runs/40032438931) sont entièrement réussies : Windows x64, Linux x64/ARM64, macOS x64/ARM64 et Chrome Linux. L'incrément suivant ajoute Firefox Linux, Safari macOS et la comparaison historique de stockage/restauration ; leur configuration ne vaut pas encore preuve.

### Prototypes Windows supplémentaires

`cargo test -p xtask --test windows-native --locked` exécute deux scénarios avec un helper Rust de test, sans nouveau crate et sans modifier le produit Node :

- `job_close_stops_parent_and_descendant_without_touching_another_process` : création d'un Job Object avec arrêt à la fermeture, association du parent avant autorisation explicite de créer son enfant, attente native des deux fins ; un processus témoin reste actif. Cela prouve la primitive Windows. M04 doit encore traiter le lancement d'un programme arbitraire sans course avant association, l'arrêt gracieux et les différentes CLI.
- `staged_worker_verifies_identity_waits_and_replaces_a_real_executable` : copies d'exécutables dans un dossier temporaire marqué ; Windows refuse l'écrasement de l'image en cours ; un worker copié dans `stage` refuse une date de création incorrecte, conserve un handle du bon processus, attend sa fin, remplace le binaire puis le test lance le candidat. Ce helper n'est pas l'updater transactionnel M09 : pas de journal, signature, sauvegarde ni rollback applicatif.

Sources de décision : [Job Objects Microsoft](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [identité par GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes). Les tests utilisent les bindings `windows-sys` déjà verrouillés et des handles possédés, pas une recherche destructive de PID.

`cargo xtask test-tray` a réussi sur le bureau Windows courant : ajout d'une icône temporaire, version 4, rectangle réel retourné par le shell puis retrait vérifié. La fenêtre reste cachée, l'icône est retirée à la sortie y compris en échec. API : [Shell_NotifyIconW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw). Ce contrôle exige un bureau ; il échoue explicitement s'il manque, sans test ignoré. Il est distinct de la suite CI sans bureau et ne prouve pas les menus, clics, gestion du focus ou redémarrage d'Explorer exigés en M08.

La suite Rust compte maintenant **12 tests réussis**, zéro ignoré, plus ce contrôle tray explicite ; formatage et Clippy natif/WASM réussis. Les helpers restent dans `xtask`, hors de tout paquet produit.

Preuves suivantes : [CI `f9b1598`](https://github.com/FluryWaana/atelier/actions/runs/40034051039) entièrement réussie, cinq runners natifs, Chrome/Firefox Linux et Safari macOS. [CI `33d594a`](https://github.com/FluryWaana/atelier/actions/runs/40034806672), cinq runners natifs réussis avec création/relecture/suppression d'un secret synthétique dans les trousseaux Linux et macOS, sur les deux architectures.

Les cinq jobs natifs de [fec382f](https://github.com/FluryWaana/atelier/actions/runs/40038851567) réussissent, y compris les probes tray Linux/macOS sur les deux architectures. Avec les preuves antérieures Windows, coffres et quatre navigateurs, **M01 est validé pour la faisabilité**. Les refus natifs de coffres verrouillés/autres comptes restent nécessaires pour M02, et les comportements complets des trays pour M08. La branche de migration est poussée avec autorisation explicite ; aucune release publiée.

### Prototypes tray POSIX

`xtask/tests/support/tray_posix.rs` ajoute le même contrôle explicite aux deux plateformes. Linux exige un vrai hôte StatusNotifier, observe l'enregistrement D-Bus, vérifie identifiant, statut et pixels, puis attend le retrait après destruction. La CI démarre un panneau XFCE sous Xvfb dans une session D-Bus/configuration temporaire ; ce n'est pas un faux watcher. [XFCE documente ce support dans son panneau](https://xfce.org/about/tour416). macOS crée l'icône sur le thread principal depuis une callback de la boucle AppKit, exige une fenêtre visible et un rectangle non nul, puis vérifie son retrait.

Les dépendances de test sont épinglées : [tray-icon 0.26.1](https://docs.rs/tray-icon/0.26.1/tray_icon/) (MIT/Apache-2.0), backend KSNI sans GTK, [zbus 5.19.0](https://docs.rs/zbus/5.19.0/zbus/) (MIT), objc2 0.6.5 et bindings AppKit/Foundation 0.3.2 (MIT, Apache-2.0/Zlib selon composant), block2 0.6.2 (MIT). Elles restent dans `xtask`, hors du produit. Un contrôle de types et Clippy croisé précède la CI ; seul le résultat sur le vrai système compte comme preuve native. Ces essais ne valident pas encore les clics, menus, focus ou redémarrage du panneau de M08.

La CI met désormais Trunk/wasm-bindgen en cache dans un répertoire réservé, par OS, architecture et versions exactes. Le contrôle des versions dans `build-ui` reste obligatoire ; aucune installation Cargo générale n'est restaurée par ce cache.

M02 doit durcir les lectures de session par descripteur (permissions/identité/liens/reparse points/courses), les chemins et les fermetures exceptionnelles, porter toutes les tables et sauvegardes, puis prouver Node → Rust → Node. Le prototype est limité aux données temporaires ; il ne doit jamais être utilisé avec une base personnelle. M03–M11 restent à faire. Aucun gain de performance ni paquet Rust livrable n'est annoncé.
