# Nettoyage des contrats et responsabilités

Ce complément décrit la refonte engagée après l'audit du 30 octobre. Il ne constitue pas une certification SOLID ni une qualification fonctionnelle du rebuild.

## Changements

- Le domaine, les adaptateurs, l'interface et leurs appelants utilisent `String` et `&str`. Le stockage personnalisé en unités UTF-16 et le décodage WTF-8 ont été retirés.
- `Document` conserve les champs JSON ouverts et leur représentation numérique brute. Ses frontières refusent les séquences Unicode invalides, y compris dans les métadonnées imbriquées. Les mutations ne peuvent pas introduire ces valeurs.
- Les champs connus utilisent des lectures booléennes, textuelles, objet ou liste. Les conversions implicites de valeurs JSON ont été supprimées. Une durée machine est un nombre entier de secondes. La saisie humaine conserve le format explicite `HH:MM:SS`.
- Les liens de dépendance utilisent une liste d'identifiants. Une chaîne contenant un identifiant n'est plus assimilée à une liste.
- La duplication tronque les titres à une frontière de caractère Unicode. Les limites de longueur restent compatibles avec les champs de saisie du navigateur.
- Toutes les valeurs de secrets sont validées avant tout appel au protecteur. Un champ valide ne masque plus un autre champ mal typé.
- Les routes HTTP, la sécurité du transport et l'arbitrage des mutations ont des modules distincts. Chaque route conserve sa politique de concurrence explicite.
- Les handlers réutilisent une classification des erreurs métier. Les erreurs de stockage et de tâches asynchrones reconnues renvoient un message public sans détails internes. La saturation des diagnostics possède une variante métier dédiée.
- Le type de résultat partagé appartient au noyau. Les adaptateurs le réexportent. Les variantes métier restent propres à leurs cas d'usage.
- Les deux représentations du catalogue de profils partagent une seule construction des profils par défaut.

## Principes appliqués

La responsabilité du transport reste dans `atelier-app`. Les cas d'usage et leurs traits restent dans `atelier-core`, sans accès disque, réseau ou processus. Les traits existants restent limités à leurs consommateurs. Aucun framework de commandes, interface systématique ou dépendance supplémentaire n'a été ajouté.

Le JSON ouvert reste nécessaire pour les extensions et les métadonnées de capacités variables. Il ne justifie plus les coercitions des champs connus. Le remplacement intégral de tous les documents par des structures fermées supprimerait ces capacités et n'est pas retenu.

## Contrats intentionnellement modifiés

Les surrogates isolés, les durées sous forme de chaînes ou de tableaux, les faux booléens et les secrets non textuels sont refusés. Les fixtures positives utilisent désormais des entrées valides. Les cas de durée qui documentaient une conversion implicite attendent explicitement l'erreur de durée. Les attentes ne sont pas régénérées depuis l'exécution du produit.

Ce choix suit la reconstruction sans compatibilité historique. Aucun accès aux données utilisateur ni conversion d'une base personnelle n'a été effectué. Une ancienne base synthétique contenant ces valeurs peut être refusée.

## Vérifications

Les cibles natives et leurs cibles de tests ont été vérifiées avec `cargo check --workspace --exclude atelier-ui --all-targets --locked --offline`. La cible WASM a été vérifiée séparément avec `cargo fix -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked --offline --allow-dirty --allow-staged`. `cargo fix` applique uniquement les suggestions du compilateur et n'exécute pas les tests.

Le contenu isolé du commit a également été compilé avec `cargo check` sur les deux cibles, indépendamment des changements locaux antérieurs. `cargo fmt --all -- --check` passe sur l'arbre de travail. La construction de production `cargo xtask build-ui` a réussi avec Trunk 0.22.1 et wasm-bindgen 0.2.130.

Des régressions ont été écrites pour les refus Unicode et de types, la conservation des métadonnées, les secrets, les frontières de titre, les statuts HTTP, la confidentialité des erreurs internes et l'absence de création après une requête invalide. Un garde-fou d'architecture vérifie les dépendances du noyau.

Les tests, Clippy, les parcours navigateur, les qualifications Linux et macOS et la CI restent non exécutés. Les parcours sensibles et les fixtures adaptées restent donc à qualifier. Le checklist actif du rebuild reste ouvert.
