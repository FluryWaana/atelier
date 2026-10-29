# Champs de formulaire

Le bloc final de `style.css` définit l’apparence commune des champs : couleurs,
typographie, hauteur, bordures, focus, lecture seule, désactivation et erreur
(`aria-invalid="true"`). Les classes existantes `form-control` et `form-select`
utilisent ce même contrat ; il s’applique aussi automatiquement aux éléments
HTML natifs. Les écrans gardent la responsabilité des largeurs et espacements.

Les anciennes règles sont regroupées dans la couche CSS `legacy` pour que les
styles communs priment sans ajouter de sélecteurs par écran ni de `!important`.
Modifier les variables `--field-*` pour faire évoluer tous les champs ensemble.
Les cases à cocher, boutons radio et champs spécialisés gardent leur rendu dédié.

Les selects simples utilisent `appearance: base-select` et `::picker(select)`
pour harmoniser aussi le menu ouvert, ses options et la sélection. Le clavier,
la recherche par saisie et la fermeture restent gérés par le navigateur. Sans
prise en charge de cette API, le menu reste natif ; les listes multiples et
les selects avec plusieurs lignes restent également natifs.

Aucune dépendance supplémentaire ni génération Sass/Node : Trunk embarque
directement `style.css`. Le parcours WebDriver existant vérifie le style commun
entre recherche et sélection de projet, ainsi que l’activation du picker si
le navigateur le prend en charge. Ce contrôle ne remplace pas une qualification
visuelle et clavier sur chaque navigateur cible.

## Palette

Les variables du bloc final de `style.css` définissent le fond noir, les surfaces
neutres et les états : vert pour activité/réussite, ambre pour vérification,
rouge pour erreur/destruction. Les actions principales sont claires avec texte
sombre ; les onglets et compteurs restent neutres. Les dégradés décoratifs ont
été remplacés par des surfaces unies. Le logo existant reste inchangé.

Direction Taste : `true-black-flat`, seed `987013130`. La navigation et la
composition existantes sont conservées pour cette revue de couleurs.
## Premier démarrage

L’accueil propose une création de tâche et l’ajout facultatif d’un projet, sans
étapes numérotées ni connecteur obligatoire. Il disparaît dès qu’une tâche existe
(y compris archivée), indépendamment de la recherche et du projet sélectionné ;
les brainstorms seuls ne comptent pas comme une première tâche. Le bouton de
création de l’en-tête revient ensuite. Fermer le formulaire rend le focus au
bouton de création visible.