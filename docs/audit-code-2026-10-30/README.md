# Audit du code Atelier Kanban

Audit statique daté du 30 octobre 2026. Atelier Kanban est une application locale de gestion de tâches, de travail avec des agents, de revue Git et de suivi du temps. La charge retenue est celle d'un utilisateur sur son poste, avec plusieurs onglets et plusieurs opérations ou processus simultanés.

**Verdict : l'architecture présente de bonnes séparations et des protections concrètes. La durabilité du journal de mise à jour Linux doit être renforcée. Le typage métier reste partiellement dépendant de mécanismes historiques JavaScript. La qualification de sécurité et de fonctionnement de l'ensemble reste ouverte.** Aucune note chiffrée ou certification de conformité n'est attribuée.

## Documents

| Document | Contenu |
| --- | --- |
| [Sécurité et fiabilité](01-securite-et-fiabilite.md) | Protections observées, défaut de durabilité, surfaces sensibles et limites |
| [Clean code Rust et SOLID](02-clean-code-rust-solid.md) | Analyse de l'architecture, du typage, des responsabilités et des contrats |
| [Technologies et raisons de leur utilisation](03-technologies.md) | Langages, bibliothèques directes, stockage, sécurité, intégrations et outils |
| [Méthode et plan de vérification](04-methode-et-verifications.md) | État audité, couverture réelle, priorités et validations restantes |
| [Inventaire des fichiers](inventaire-fichiers.csv) | Chemins, empreintes SHA256 et niveau de lecture pour retrouver l'état examiné |
| [Corrections après l'audit](05-corrections.md) | Correctifs apportés ensuite, changements de contrats et vérifications encore ouvertes |
| [Mise à jour globale des dépendances](06-dependencies.md) | Versions stables retenues, adaptations, contrôles et avis de sécurité transitifs restants |
| [Nettoyage des contrats et responsabilités](07-clean-code-solid.md) | Unicode natif, validation typée, séparation HTTP et qualification restante |

## État de référence

La branche est `main`. Le commit de base est `7a5bf7d264c6f3f456539fb588e524b65c6a04f4`. Des modifications locales existaient avant l'audit. Les observations décrivent les fichiers de travail présents pendant la revue, pas uniquement ce commit. Les empreintes de l'inventaire permettent d'identifier cette différence.

Le corpus inventorié comprend 467 fichiers avant ajout de ce dossier, dont 341 fichiers Rust représentant 98 745 lignes physiques au relevé initial. Il couvre les sources, les tests, les fixtures, les documents et les configurations des chemins indiqués dans la méthode. Ce chiffre décrit un inventaire, pas 98 745 lignes validées individuellement.

**Tous les fichiers n'ont pas fait l'objet d'une lecture humaine ligne par ligne.** Les chemins sensibles ont été examinés par extraits et suivis jusqu'à leurs appelants. Les recherches transversales ont couvert les sources actives. L'inventaire distingue ces niveaux. Présenter cette revue comme exhaustive serait inexact.

## Lecture des conclusions

- **Constat confirmé par lecture** : le comportement ou la structure est visible dans le code cité. Cela ne signifie pas qu'un scénario a été exécuté.
- **Risque conditionnel** : le mécanisme existe, mais son effet dépend d'une situation précisée, comme une coupure brutale ou la collecte des URL.
- **Point à qualifier** : les preuves disponibles ne suffisent pas à conclure.

Les numéros de constats sont communs aux documents. Aucun correctif du produit n'est inclus. Aucun accès aux données utilisateur, compte réel ou agent payant n'a été effectué. Aucun test, Clippy, scénario navigateur, build ou workflow CI n'a été lancé pour cet audit.
