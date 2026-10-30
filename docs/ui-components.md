# Composants de l'interface

Les composants Leptos de `crates/atelier-ui/src/form.rs`, `button.rs`, `card.rs` et `modal.rs` portent les interactions communes. Aucun kit JavaScript ou pipeline Sass supplémentaire n'est nécessaire.

## Champs

- `Input` conserve les types HTML natifs, dont texte, nombre, date, heure, email et URL. `Textarea`, `Select`, `Checkbox` et `Switch` couvrent les autres contrôles courants.
- Les attributs natifs se placent après `{..}`. Les propriétés du composant se placent avant. Les booléens natifs doivent avoir une valeur explicite, par exemple `required=true`.
- `Field` associe le libellé, l'aide et les erreurs au contrôle avec `aria-describedby` et `aria-invalid`. Un `id` explicite doit être unique. Sans identifiant fourni, le composant en génère un pour chaque instance, y compris dans une liste. Une erreur de validation apparaît à la sortie du champ et se corrige pendant la saisie. La propriété `error` accepte une erreur métier ou serveur traduite.
- `Form` vérifie les contraintes HTML avant de soumettre et place le focus sur le premier contrôle invalide. `on_invalid` permet d'ouvrir son onglet. Les sections repliées contenant le champ invalide sont ouvertes avant le focus. `on_submit` reçoit le SubmitEvent une fois le formulaire valide. `busy` empêche les soumissions répétées. La validation serveur reste obligatoire.
- `Input copy=true` ajoute un bouton copier et un retour accessible de réussite ou d'échec. `Input input_type="password" reveal=true` permet d'afficher la valeur saisie. Aucun secret enregistré n'est chargé pour cette action. Avec une désactivation individuelle, fournir aussi `disabled=Signal::derive(...)` avant `{..}` pour désactiver les actions.
- `Combobox` filtre des `Choice { value, label }`. La saisie seule ne change jamais la valeur enregistrée. Une sélection par clic ou Entrée valide la valeur. Flèches, Début, Fin, Échap et Tabulation sont pris en charge.

```rust
view! {
    <crate::form::Field id="name" label=Signal::derive(move || i18n.text("message.project_name"))>
        <crate::form::Input {..} id="name" required=true prop:value=move || name.get()
            on:input=move |event| name.set(event_target_value(&event))/>
    </crate::form::Field>
}
```

## Boutons

`button::Button` est le composant commun pour tous les boutons des vues et des modales, y compris les actions des champs. Il rend un bouton HTML natif. Son type par défaut est `button` pour éviter une soumission accidentelle. Fournir `type="submit"` après `{..}` pour soumettre un formulaire. Les événements, identifiants, titres, rôles, attributs ARIA et attributs de formulaire restent natifs.

`ButtonVariant` propose `Primary`, `Secondary` par défaut, `Ghost` et `Danger`. Réserver `Primary` à l'action principale et `Danger` aux actions destructives. Les boutons d'icône doivent garder un nom accessible avec `aria-label`. Les styles de disposition des onglets, listes et menus restent dans leur vue.

`class`, `disabled` et `busy` se placent avant `{..}`. La classe fournie est ajoutée à `ui-button`. `disabled` et `busy` reçoivent des signaux booléens. `busy` désactive le bouton, renseigne `aria-busy` et affiche un indicateur qui respecte la préférence de réduction des animations. Conserver un libellé pendant l'attente. Le signal `busy` du formulaire affiche aussi l'indicateur sur son bouton de soumission. Un champ désactivé par son `fieldset` conserve le comportement natif.

Les styles de base précèdent les règles de contexte dans la couche CSS existante. Ils ne centrent pas les contenus avec Flexbox. La navigation garde ses icônes et libellés alignés à gauche. Les titres des cartes gardent leur disposition de texte cliquable. La barre de filtres aligne les contrôles en bas sur bureau et les répartit sur plusieurs lignes sur mobile. Ces alignements et l'absence de débordement ont été contrôlés à 1440, 768 et 390 pixels après correction.

```rust
view! {
    <crate::button::Button
        variant={crate::button::ButtonVariant::Primary}
        busy={Signal::derive(move || pending.get())}
        disabled={Signal::derive(move || !dirty.get())}
        {..}
        type="submit"
    >
        {move || i18n.text("message.save")}
    </crate::button::Button>
}
```

## Cartes

`card::Card` rend un article HTML avec la surface commune. `class` se place avant `{..}`. Les identifiants, attributs ARIA, états et événements restent sur l'article. La disposition des listes, des processus, des services et du suivi du temps reste définie par leur vue.

`CardHeader` aligne le libellé et les actions. `CardFooter` regroupe les métadonnées et les états secondaires. Le Kanban utilise ces trois composants. Son titre ouvre la tâche et nomme l'article avec `aria-labelledby`. Le menu garde un bouton de 44 pixels, disponible au clavier et sur écran tactile. Les cartes ne deviennent pas des boutons contenant d'autres boutons. Le déplacement, l'archivage et les confirmations gardent leurs gestionnaires existants.

Le menu natif `popover` utilise les ancres CSS pour rester près de son bouton, avec retournement si le bord de la fenêtre est proche. Les métadonnées de projet sont du texte et ne présentent pas de flèche de liste déroulante. Aucun paquet supplémentaire n'est requis.

Les choix de projet et de sauvegarde restent des boutons avec la classe `ui-card-choice`. Les messages de conversation gardent leur présentation de conversation. Les autres articles d'information et d'objet utilisent `Card`. Les styles sont limités à ces classes et ne changent pas la navigation ou les champs.

Vérification du 30 octobre : 16 usages de surface migrés et 105 états contrôlés sur trois largeurs. Le parcours ciblé couvre les titres longs, les descriptions vides, les noms accessibles, les cibles d'action de 44 pixels, les menus près du bouton et leur retournement en bas d'écran. L'ouverture utilise des événements clavier natifs du navigateur. Le changement de colonne vérifie les gestionnaires avec des événements DragEvent synthétiques et le statut enregistré. Il ne qualifie pas un geste physique de glisser-déposer sur chaque plateforme. Les 24 contrôles d'onglet sélectionné passent aussi.

Design Taste sur le Kanban : 0 échec et 3 avertissements. Les libellés de page et de pied de page existaient déjà. Les différences de hauteur et les zones vides correspondent aux listes Kanban de tailles différentes et à leurs zones de dépôt. Les cartes restent de hauteur adaptée à leur contenu. Les captures des trois largeurs et les planches du catalogue ont été relues. Les parcours métier complets restent à qualifier séparément.

## Comportement des modales

`Modal` porte le dialogue natif, le titre accessible, l'ouverture et Échap. Le parent garde la décision de fermer ou de confirmer la perte du brouillon. `busy` empêche Échap pendant une opération. `ModalHeader`, `ModalBody` et `ModalActions` donnent les espacements et les actions communs. Le bouton de fermeture doit utiliser la même demande de fermeture que la touche Échap.

Une modale montée seulement quand elle est ouverte utilise le comportement par défaut. Un dialogue persistant ouvert explicitement avec son `NodeRef` utilise `auto_open=false`. Ne pas ajouter un second appel `show_modal` au montage.

`Confirmation` fournit une modale persistante, un titre accessible, la fermeture par Échap et le bouton de fermeture. Le parent fournit le signal `open`, l'état `busy` et sa vraie annulation via `on_close`. Le bouton d'annulation porte `data-confirmation-cancel` pour rendre le focus au formulaire avant son gestionnaire. Les actions d'annulation et de confirmation utilisent `ModalActions`. Les informations de récupération ou de vérification qui doivent rester près de leur action utilisent `Notice`.

## Couverture du 30 octobre 2026

Les 331 boutons utilisent `button::Button`. La reprise du catalogue sur 105 états et trois largeurs contrôle aussi l'usage du composant commun et le nom accessible de chaque bouton visible. Le scénario ciblé d'enregistrement vérifie le type de soumission, le bouton secondaire sans soumission, le brouillon propre, le blocage pendant la requête, `aria-busy`, l'indicateur d'attente et la réduction des animations. Les 20 contrôles des actions de champ passent aussi sur chacune des trois largeurs. La compilation WASM avec toutes les cibles, la compilation de `xtask`, le formatage et la génération UI passent. Les parcours métier complets ci-dessous restent distincts de cette vérification ciblée.

L'inspection Design Taste des projets donne 0 échec et 2 avertissements. Ils concernent le petit libellé du pied de page et la longueur du catalogue synthétique, sans lien avec les boutons. Les captures des trois largeurs et les planches des 105 états ont été relues.

Les formulaires et dialogues des vues utilisent ces composants communs. Les contrôles HTML natifs restent dans leur implémentation. Les confirmations de suppression, d'abandon et d'opérations externes utilisent `Confirmation`. Les panneaux de récupération restent dans leur contexte avec `Notice`.

Le catalogue synthétique a contrôlé 105 états à 390, 768 et 1440 pixels. Il couvre les 11 routes principales, les onglets des projets, des paramètres et des tâches, ainsi que les dialogues principaux. Aucun débordement horizontal, identifiant dupliqué, champ sans libellé, description ARIA manquante ou dialogue sans titre n'a été détecté. Les captures sont des contrôles de présentation et d'accessibilité structurelle. Elles ne valident pas toutes les opérations métier.

Les parcours WebDriver complets ne sont pas validés. Le dernier passage a révélé une mauvaise cible de focus à l'ouverture d'une tâche, corrigée vers `editor-heading`. Le parcours Tempo a rencontré un refus temporaire d'admission. Le parcours de mise à jour a été refusé parce que le binaire de vérification n'avait pas le nom applicatif requis. Ces résultats ne doivent pas être comptés comme des réussites. Les tests Rust, Clippy et la CI n'ont pas été exécutés.
