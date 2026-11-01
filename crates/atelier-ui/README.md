# Atelier UI

The browser interface for Atelier Kanban uses Leptos CSR and compiles to WebAssembly. The native application serves its assets over authenticated loopback HTTP.

## Build

Run from the repository root with the committed Rust toolchain, Trunk 0.22.1 and wasm-bindgen CLI 0.2.130:

```sh
cargo fetch --locked
cargo xtask build-ui
```

Release assets are written to `.artifacts/rust-ui`. For the Windows development preview, `cargo xtask build-ui --dev` writes `.artifacts/rust-ui-dev`.

Trunk bundles `style.css` directly. The build externalizes the module loader for the content security policy and enforces asset size limits. No Node.js or Sass build is required.

## Components and styling

Use shared fields, buttons, cards and dialogs described in [UI components](../../docs/ui-components.md). The final CSS blocks define common field tokens and the neutral dark palette. Activity and success use green, verification uses amber, and errors and destructive actions use red.

Simple selects use the browser's customizable picker where available and retain native behavior otherwise. Keyboard operation, accessible labels and disabled states must work in both cases.

## First use

The welcome screen offers local task creation and optional project setup. It disappears when any task exists, including an archived task. Brainstorm discussions alone do not count as a first task. Closing creation restores focus to the visible creation control.

## Testing

```sh
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
```

Use synthetic browser journeys for interaction changes. See [testing](../../docs/testing.md) and [contributing](../../CONTRIBUTING.md).
