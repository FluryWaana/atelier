# Corrections après l'audit du 30 octobre 2026

Ce suivi distingue les correctifs des preuves de qualification. L'inventaire et les quatre documents initiaux restent le relevé de l'état audité. Les modifications locales présentes avant cette intervention sont conservées.

| Constat | Correction apportée | Vérification disponible et limite |
| --- | --- | --- |
| 1 Durabilité Linux | La publication privée synchronise le dossier destination puis le staging après renommage, avec les handles ancrés. Les erreurs remontent à l'appelant, notamment au journal de mise à jour. Un échec après publication conserve le fichier publié. | Régression écrite avec échec injecté sur chacune des deux synchronisations. Chemins existants de publication et d'ancrage conservés. Compilation Windows seulement, ni exécution Linux ni panne électrique simulée. |
| 2 Contrats historiques | Les traits Temps et Modèles reçoivent `&str`. Les cas d'usage refusent les identifiants absents, non textuels ou blancs avant toute écriture. Les adaptateurs réutilisent `Store::document`. `document_input` et son émulation SQLite Node sont supprimés. La sélection de l'agent par défaut valide des chemins et agents texte. Le corps HTTP est borné à 400 000 octets, sans comptage Node supplémentaire. | Régressions écrites pour les types refusés, l'absence d'écriture, les identifiants texte numériques, les cartes manquantes, les chemins Windows, les agents invalides et la limite HTTP. Le JSON ouvert et `JsonString` restent utilisés dans les autres contrats. Ce correctif applique le périmètre minimal proposé par l'audit et ne prétend pas remplacer tous les modèles dynamiques du produit. |
| 3 Politique HTTP | Chaque enregistrement API porte une politique explicite. Le middleware de sécurité est séparé de la garde de mutation. Verrou exclusif, garde projet et annulation sont déclarés avec la route. Les actions Temps externes ont des routes dédiées. Le repli refuse les actions inconnues, y compris les variantes encodées. | Vérification écrite contre les enregistrements sans politique. Tests existants de concurrence et d'annulation conservés. Régression HTTP écrite pour les actions Temps refusées. Aucun scénario exécuté. |
| 4 Consigne CI | Le README retire l'autorisation périmée des relances et indique l'interdiction actuelle. | Relecture du README, d'AGENTS.md et du déclenchement manuel du workflow. Aucun lancement ni relance. |
| 5 Session SSE | L'interface obtient avec son Bearer un cookie indépendant, HttpOnly et SameSite Strict, limité au chemin des événements. Son nom distingue les ports locaux. Le serveur accepte ce cookie uniquement pour `GET /api/events`. La connexion EventSource ne contient aucun secret dans son URL. L'ancien paramètre `session` n'authentifie plus le flux. | Régressions écrites pour le refus de la query historique, la portée limitée du cookie, les ports distincts, les mauvais secrets, le refus d'un Bearer incorrect et Origin. Notifications, reconnexion et arrêt restent dans le parcours existant. Le comportement navigateur reste à qualifier. |
| 6 Dépendance inutilisée | La dépendance directe `tower-http` avec `fs` est retirée. Cargo retire les paquets devenus inutiles du verrou et conserve la version transitive requise par reqwest. | Compilation native et WASM avec le verrou mis à jour. Aucun gain de performance ou de taille revendiqué. |

## Changements intentionnels des contrats de test

Les identifiants JSON non textuels ne sont plus convertis en paramètres SQLite. Le scénario historique consacré à ces conversions est remplacé par `card-identifiers.rs`, qui vérifie les refus et la conservation des données. Les trois attentes concernées du parcours Temps deviennent `message.invalid_card_id`. Les 38 autres étapes du parcours sont conservées.

Un chemin de dépôt contenant un surrogate isolé ne peut plus sélectionner l'agent par défaut. Le scénario positif utilise désormais un chemin Unicode valide. Un test distinct exige le refus des chemins et agents invalides. Il ne s'agit pas d'une régénération automatique des attentes.

La taille HTTP est mesurée en octets encodés. Le scénario de dépassement utilise plus de 400 000 octets. Un autre scénario vérifie qu'une requête Unicode sous cette borne n'est pas refusée par l'ancien seuil UTF-16. Les limites propres aux champs métier restent appliquées.

## Vérifications locales

- `cargo fmt --all -- --check` réussit.
- `cargo check --workspace --exclude atelier-ui --all-targets --locked --offline` réussit sur `x86_64-pc-windows-msvc`, cibles de tests comprises.
- `cargo check -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked --offline` réussit.

Le contrôle natif inclut le travail local préexistant. Ces commandes ne lancent aucun test. Cargo signale la copie du cache incrémental faute de liens physiques disponibles. Les journaux locaux sont dans `.artifacts/audit-fixes-native-check.log` et `.artifacts/audit-fixes-wasm-check.log`. Ils ne sont pas des artefacts de qualification publiés.

Conformément aux consignes actuelles, tests locaux, Clippy, parcours navigateur et CI ne sont pas exécutés. La durabilité réelle Linux, les parcours multi-OS, les coffres natifs, les paquets, les avis de sécurité des dépendances et les mesures de croissance restent à qualifier. Ces correctifs ne valident pas R07 et n'autorisent aucune publication.

## Suite de l'audit clean code

Les sept constats de la seconde revue sont traités dans les sources. Les gardes de mutation reconstruisent leur cible avec les paramètres décodés du routeur. Les identifiants contenant un séparateur décodé ou un caractère de contrôle sont refusés. Les mutations de cartes consultent aussi les exécutions persistées et la suppression consulte la file réelle.

Le chargement du tableau et le démarrage restent disponibles lorsque le coffre ne peut pas être lu. Le nettoyage des anciens connecteurs est alors différé sans supprimer leurs identifiants. Les indicateurs de présence des jetons valent `null`, car leur état est inconnu. Un message traduit explique l'indisponibilité des connexions. Les écritures nécessitant le coffre restent refusées et les données chiffrées sont conservées.

Les handlers Temps, Runs, Worktrees et Fichiers réutilisent la classification HTTP commune. Les erreurs internes reçoivent un message public et un statut 500. Les erreurs métier gardent leur classification. Les branches de conflit conservent leur statut par défaut lorsqu'aucune erreur métier plus précise n'est disponible.

Les assertions de suppression refusent séparément les dépendances et les identifiants de temps mal typés. Les journaux sont bornés en octets UTF-8 à une frontière de caractère. Le contrat de découpage JavaScript et son attente de caractère de remplacement sont retirés.

Le formulaire de création, ses modèles, son brouillon et ses actions sont regroupés dans `new_task.rs`. Le composant reste monté pour conserver la saisie après une fermeture. La navigation et les filtres restent dans `App`. Les vues sont mises en forme avec leptosfmt 0.1.33, largeur 110 et style `AlwaysUnlessLit`, puis avec `cargo fmt`. Les expressions génériques d'attributs restent entre accolades pour être comprises par Leptos. Cet outil ponctuel n'est ni une dépendance du produit ni une nouvelle étape obligatoire de fabrication.

Des régressions synthétiques sont écrites pour les URL encodées, les séparateurs et l'UTF-8 invalides, l'activité persistée, la confidentialité des erreurs, les frontières Unicode des journaux et le travail local avec un coffre indisponible. Le parcours HTTP du coffre vérifie également la conservation des identifiants et des données après l'arrêt. Ces tests ne sont pas exécutés.

Les modifications locales antérieures de formulaires, de modales et d'accès aux dépôts sont conservées. Un instantané séparé des corrections est préparé pour vérifier et committer le travail de cet audit sans y intégrer ces autres changements. Le suivi du rebuild reste ouvert. Aucune qualification fonctionnelle, multi-OS ou de publication n'est déduite de ces corrections.

### Preuves de compilation de cette correction

`cargo fmt --all -- --check` réussit sur l'arbre de travail et sur l'instantané isolé. Les commandes `cargo check --workspace --exclude atelier-ui --all-targets --locked --offline` et `cargo check -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked --offline` réussissent sur les deux états, cibles de tests comprises. Les contrôles isolés utilisent explicitement le dossier `target` du workspace. Les avertissements restants concernent la copie du cache incrémental lorsque les liens physiques ne sont pas disponibles.

Les journaux locaux sont `.artifacts/clean-code-native-check.log`, `.artifacts/clean-code-wasm-check.log`, `.artifacts/clean-code-isolated-native-check.log` et `.artifacts/clean-code-isolated-wasm-check.log`. Ils restent hors Git. Aucun test, Clippy, scénario navigateur ou workflow CI n'est exécuté. La reconstruction et sa qualification finale ne sont pas déclarées terminées.
