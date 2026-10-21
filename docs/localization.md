# Langues de l’interface

Atelier démarre en **English (US)**. **Settings → General → Language** permet de
choisir **Français (France)**. Le choix est conservé dans ce navigateur, sans
modifier la base locale. Le changement recharge la page ; enregistrer les autres
réglages avant de changer de langue. Le paramètre `?lang=fr-FR` permet aussi de
partager un lien et fonctionne si le stockage du navigateur est bloqué.

Les dates, nombres, coûts, confirmations, modèles intégrés et exports suivent
la langue choisie. Les titres et descriptions saisis, les tickets importés,
les journaux des CLI et les réponses des agents restent dans leur langue d’origine.
Les prompts métier des agents ne sont pas modifiés par la langue de l’interface.

## Ajouter une langue

1. Copier `public/locales/en-US.mjs` vers un nouveau catalogue, par exemple
   `public/locales/es-ES.mjs`.
2. Traduire les **valeurs**, sans changer les clés ni les paramètres `{0}`, `{1}`.
   Ces paramètres peuvent être déplacés pour respecter la grammaire.
   Les clés sont les textes sources français ; ce sont des identifiants stables.
3. Importer le catalogue dans `public/locales/index.mjs`, puis ajouter son entrée :

```js
import esES from './es-ES.mjs';
// Dans languages :
'es-ES': { label: 'Español (España)', messages: esES },
```

Le sélecteur et les ressources servies par le serveur sont mis à jour
à partir des catalogues ; aucun changement des vues ni de `server.mjs` n’est requis.
Redémarrer le serveur après l’ajout d’un fichier. Une traduction absente utilise
l’anglais, puis le texte source. Le français utilise directement les clés sources,
ce qui explique son catalogue vide. Utiliser une balise de langue BCP 47 comprise
par `Intl` pour les nombres et les dates.

Pour un nouveau texte, appeler `msg('Texte source')` ou
`msg('Projet de {0}', nom)`, et ajouter la traduction anglaise. Dans du HTML,
`markupText('Projet de {0}', esc(nom))` échappe le texte du catalogue et conserve
les arguments déjà échappés par la vue. Ne jamais y passer des données brutes.
Les textes du HTML initial sont traduits une fois au démarrage, avant le rendu
des données utilisateur. Ne pas traduire les identifiants, chemins ou valeurs de formulaire.

`messageText` traduit à l’affichage les erreurs et messages d’état connus du
serveur, en conservant leurs paramètres. Les messages inconnus restent lisibles
sans être remplacés par une erreur générique.

## Traductions des modules

Chaque module fournit ses propres traductions via deux propriétés optionnelles :

```js
sourceLocale: 'fr-FR',
translations: {
  'en-US': {
    'Mon module': 'My module',
    'Adresse du service': 'Service address',
    'Vérifier': 'Check',
  },
  'es-ES': {
    'Mon module': 'Mi módulo',
    'Adresse du service': 'Dirección del servicio',
    'Vérifier': 'Comprobar',
  },
},
```

Les clés sont les textes d’origine du module. Traduire `name`, `description`, les
libellés des champs et options, ainsi que les libellés et descriptions d’onglets
et les actions. Les résultats d’actions textuels peuvent également figurer dans
ce catalogue ; les résultats JSON et données distantes restent inchangés.

La langue source utilise le texte d’origine. Les autres langues utilisent leur
catalogue, puis l’anglais, puis le texte d’origine. Les catalogues ne doivent
contenir que du texte, jamais de secrets, de HTML à exécuter ou de fonctions.
Le navigateur échappe les traductions avant de les afficher. Les valeurs et
identifiants techniques des champs ne changent pas.

Les propriétés sont facultatives pour conserver la compatibilité des modules
existants. Un module sans catalogue garde ses libellés d’origine si aucune
traduction intégrée n’est disponible. Voir `examples/connectors/demo/index.mjs`
pour un module complet et `test/i18n.test.mjs` pour les tests de contrat et de rendu.

## Vérification

Exécuter `npm test` avec Node.js 24 ou le runtime fourni. Les tests couvrent le
défaut anglais, le français, le repli d’une troisième langue, les paramètres,
les valeurs utilisateur, les catalogues de modules et la préférence du navigateur.
Vérifier aussi les écrans et fenêtres en anglais et en français après une
modification de textes ou l’ajout d’une langue.
