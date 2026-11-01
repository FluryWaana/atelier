# UI components

Common Leptos components live in `crates/atelier-ui/src/form.rs`, `button.rs`, `card.rs` and `modal.rs`. Reuse them for consistent keyboard behavior, accessible names and state handling. The UI has no additional JavaScript component kit or Sass build.

## Fields

`Input` retains native HTML types such as text, number, date, time, email and URL. `Textarea`, `Select`, `Checkbox` and `Switch` cover other common controls.

Place component properties before `{..}` and native attributes after it. Native booleans need explicit values such as `required=true`.

`Field` associates labels, help and errors through `aria-describedby` and `aria-invalid`. Explicit IDs must be unique. Without one, the component generates an ID per instance. Validation errors appear on blur and update during input. The `error` property accepts translated business or server errors.

`Form` checks HTML constraints and focuses the first invalid control. `on_invalid` can open its tab. Collapsed sections containing invalid fields open before focus. `on_submit` receives the event after validation. `busy` blocks repeated submission. Server validation remains required.

```rust
view! {
    <crate::form::Field id="name" label=Signal::derive(move || i18n.text("message.project_name"))>
        <crate::form::Input {..} id="name" required=true prop:value=move || name.get()
            on:input=move |event| name.set(event_target_value(&event))/>
    </crate::form::Field>
}
```

`Input copy=true` supplies a copy action with accessible success or failure feedback. `Input input_type="password" reveal=true` reveals only the entered value and never loads a stored secret. For individual disabling, also pass `disabled=Signal::derive(...)` before `{..}` so field actions are disabled.

`Combobox` filters `Choice { value, label }`. Typing does not change the saved value. Clicking or pressing Enter selects a choice. Arrow keys, Home, End, Escape and Tab retain keyboard support.

## Buttons

`button::Button` renders a native HTML button. Its default type is `button`. Set `type="submit"` after `{..}` for form submission. Native events, IDs, titles, roles, ARIA and form attributes remain available.

`ButtonVariant` provides `Primary`, `Secondary`, `Ghost` and `Danger`. The default is `Secondary`. Reserve `Primary` for the main action and `Danger` for destructive actions. Icon buttons require an accessible name.

Place `class`, `disabled` and `busy` before `{..}`. The supplied class is added to `ui-button`. Boolean signals control `disabled` and `busy`. Busy state disables the button, sets `aria-busy` and shows an indicator that respects reduced motion. Retain a label while waiting. A busy form also marks its submit button. Native fieldset disabling remains effective.

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

## Cards

`card::Card` renders an article with the common surface. Place `class` before `{..}`. Native IDs, ARIA, states and events stay on the article. Each view owns layout.

`CardHeader` groups the label and actions. `CardFooter` groups metadata and secondary states. Kanban titles open the task and name the article through `aria-labelledby`. Keep the card menu's 44 pixel action target usable by keyboard and touch. Do not nest buttons inside a card that is itself a button.

The native `popover` menu uses CSS anchors near its trigger and flips near the viewport edge. Project and backup choices remain buttons with `ui-card-choice`. Conversation messages retain their own presentation.

## Dialogs

`Modal` provides the native dialog, accessible title, opening and Escape handling. The parent decides whether closing requires draft confirmation. `busy` blocks Escape during an operation. `ModalHeader`, `ModalBody` and `ModalActions` provide common spacing. The close button and Escape must use the same close request.

A dialog mounted only while open uses automatic opening. A persistent dialog opened through its `NodeRef` uses `auto_open=false`. Do not call `show_modal` a second time on mount.

`Confirmation` provides a persistent confirmation dialog. The parent supplies `open`, `busy` and the actual cancellation handler through `on_close`. Mark cancellation with `data-confirmation-cancel` to return focus before its handler. Use `ModalActions` for choices. Keep recovery messages near their action with `Notice`.

## Styling and checks

The final blocks of `style.css` define common controls and palette. Field tokens use `--field-*`. Common rules apply to `form-control`, `form-select` and native fields. Each screen owns widths and spacing.

Simple selects use `appearance: base-select` and `::picker(select)` when supported. Other browsers retain their native picker. Multiple selects and visible list selects retain native rendering.

Preserve English and French translations, draft protection, loading and error states, focus restoration and reduced motion. Check representative states at 1440, 768 and 390 pixels, including long labels, empty content, menu edges and invalid fields in hidden tabs. Presentation checks complement the business journeys in [verification](rust-final-qualification.md).
