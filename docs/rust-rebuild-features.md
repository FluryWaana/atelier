# Reconstruction Rust — couverture fonctionnelle

Ce document suit les fonctions du nouveau produit, sans imposer les formats ou API Node. Décision du propriétaire : application préproduction, mêmes fonctionnalités améliorées, aucune rétrocompatibilité. Voir le [plan actif](rust-migration-plan.md).

Une fonction est terminée lorsque son cas d'usage, son écran, ses erreurs et ses tests de parcours sont livrés. Les briques du prototype ci-dessous sont réutilisables ; aucune ligne n'est encore déclarée complètement qualifiée. Les sources citées sont des repères, complétés par l'inventaire des sources et le README historique.

| Fonction à conserver | Référence existante | Briques Rust / travail restant | Lot |
| --- | --- | --- | --- |
| Projets multiples, configuration, héritage, dépôts Front/Back et projets liés | `lib/application/projects.mjs`, vues projets/réglages | CRUD et transactions présents ; contrat typé et écrans complets à livrer | R01/R02 |
| Tableau, colonnes, filtres, recherche, navigation vers une tâche, archives | `public/views/board.mjs`, `task-workspace.mjs`, `lib/application/tasks.mjs` | Tableau WASM à cinq colonnes, recherche titre/description, filtres projet/archives et CSS autonome candidats ; navigation et parcours complets à livrer | R01 |
| Création/édition/duplication/suppression des cartes, critères, dépendances/blocages et modèles | `lib/domain/`, `lib/application/tasks.mjs`, éditeur de tâche | Nombreux cas d'usage/tests réutilisables ; parcours complet et nouveaux contrats à qualifier | R01 |
| Paramètres, modèles/efforts, profils et choix d'agent par projet/tâche | réglages, `agent-providers`, `default-agent` | Stockage chiffré et CRUD présents ; écran cohérent et diagnostics à livrer | R01/R03 |
| Brainstorms, discussion, propositions et conversion en tâches | `brainstorm`, vues brainstorm | Création/session partielle ; conversation et conversion complètes à livrer | R05 |
| Branches et worktrees dédiés, bases distantes, dépôts de services | `lib/worktrees.mjs`, `application/card-worktrees.mjs` | À reconstruire ; concurrence, branches existantes et suppressions confirmées à tester | R02 |
| Contexte des agents et dépendances partagées sans écraser les fichiers locaux | `infrastructure/worktree-agent-context.mjs`, `sharedDependencies` | À reconstruire et tester sur dépôts synthétiques | R02 |
| Explorateur, aperçus, changements/diffs, fichiers suivis/non suivis | `infrastructure/task-files.mjs`, vues fichiers/diff | Lectures ancrées réutilisables ; Git et interface à livrer | R02 |
| Détection des lancements Node/.NET, services Front/Back, ports, journaux et arrêt/redémarrage | `launch-detection`, `launch-ports`, `dev-servers`, `process-inspection` | Scanner/ports candidats, garde HTTP en correction ; gestionnaire et écrans à reconstruire | R02 |
| Codex, Claude Code et commandes personnalisées : découverte, versions, authentification et installation/mise à jour CLI | `agents`, `agent-account`, `claude-account`, `agent-updates`, credentials | Profils/coffre réutilisables ; adaptateurs et diagnostics à livrer avec faux CLI | R03 |
| Exécutions, streaming, logs, annulation, reprise et interactions/approbations | `application/runs.mjs`, protocoles/interactions agents | Processus Windows et capture bornée candidats ; orchestration et interface à livrer | R03 |
| Usage, durée, quotas et estimations de coût disponibles | modules agent usage/quota/time, vues activité | À reconstruire ; ne jamais présenter une estimation comme un coût facturé | R03/R05 |
| Tickets Jira/Azure : recherche, création, association, mappings et synchronisation | connecteurs, `jira-sync`, workflows tickets | À reconstruire avec réservations, pagination et erreurs de service | R04 |
| Dépôts et PR/MR GitHub/GitLab/Azure, preview, brouillon, publication, checks et review | forges, pull-requests, reviews | À reconstruire ; aucun effet réel pendant les tests | R04/R05 |
| Connecteurs extensibles, paramètres, diagnostics, traductions et actions déclaratives | `domain/connectors`, loader et exemples | Nouveau protocole versionné et exemple Rust ; pas de compatibilité des modules JS | R04 |
| File d'attente, automatisation par projet, préparation/vérification/review/publication | `application/automation.mjs`, task-queue, workflows | À reconstruire avec reprise et protection des effets concurrents | R05 |
| Temps humain/agent distinct, saisies, chronomètres, synthèses et CSV | time-tracking, vues temps, agent-time | Chronomètres/transactions réutilisables ; écrans et rapports à livrer | R05 |
| Export confirmé vers Tempo/Jira et réconciliation | tempo-export, connector-time | À reconstruire ; réservation, idempotence et reprises obligatoires | R05 |
| Sauvegardes/restauration et transfert d'une installation portable Rust | backups, portable-import, installation | Transactions/coffre réutilisables ; nouveau format uniquement, sans import Node | R01/R06 |
| Session locale, démarrage/arrêt, choix de dossier, navigateur et tray | launcher, local-session/http, browser/folders/tray | Protections et probes réutilisables ; lanceur/tray produit à livrer | R06 |
| Windows portable/Setup, Linux portable/DEB/RPM, macOS portable ; mises à jour signées | scripts de paquets, installateurs, updater | Paquets Rust et update/rollback Rust→Rust à construire | R06/R07 |
| Anglais US et français France, clavier, focus, brouillons, états d'erreur et navigation rechargeable | `public/`, locales et tests UI | À livrer dans chaque écran ; tests navigateur et revue visuelle | Tous |

## Points de contrôle

- Toute fonction retrouvée dans les sources ou l'interface doit être rattachée à une ligne ; compléter ce tableau si nécessaire. L'absence dans ce premier regroupement n'autorise pas sa suppression.
- Consigner pour chaque ligne achevée le commit, les scénarios exécutés, la CI et les limites restantes. Un test de domaine seul ne valide pas son écran ou son adaptateur natif.
- Mesurer les améliorations de consommation/réactivité sur des fixtures. Ne pas promettre de gain uniquement parce que le langage change.
- Point archivé entièrement vert : [`7326122`](https://github.com/FluryWaana/atelier/actions/runs/40120248210), trois journaux relus. Scanner HTTP corrigé, onze arbres natifs comparés, ports, lancement suspendu/descendants détachés/orphelins et capture avec dépassement/délai/annulation passent sous Windows. Clippy natif/WASM, DPAPI entre comptes et les deux parcours Edge passent également. Node de référence : 443 tests, 439 réussis, aucun échec, quatre exclusions. Ces résultats qualifient ces briques Windows, pas les fonctionnalités complètes ni les autres OS.
