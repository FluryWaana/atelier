use leptos::prelude::*;

/// Mounted by the caller while open; dismissal stays with its draft/navigation policy.
#[component]
pub fn Modal(
    id: &'static str,
    labelledby: &'static str,
    node_ref: NodeRef<leptos::html::Dialog>,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    node_ref.on_load(move |dialog| {
        let _ = dialog.show_modal();
    });
    view! {
        <dialog
            id={id}
            class="app-modal"
            node_ref={node_ref}
            aria-labelledby={labelledby}
            on:cancel={move |event: web_sys::Event| {
                event.prevent_default();
                on_close.run(());
            }}
        >
            {children()}
        </dialog>
    }
}
