use crate::i18n::I18n;
use leptos::prelude::*;

/// Mounted while open by default. The caller owns draft and dismissal decisions.
#[component]
pub fn Modal(
    #[prop(into)] id: String,
    #[prop(optional, into)] labelledby: String,
    #[prop(optional, into)] describedby: String,
    #[prop(optional)] class: &'static str,
    #[prop(default = true)] auto_open: bool,
    #[prop(optional)] node_ref: NodeRef<leptos::html::Dialog>,
    #[prop(optional, into)] busy: Signal<bool>,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    if auto_open {
        node_ref.on_load(move |dialog| {
            let _ = dialog.show_modal();
        });
    }
    view! {
        <dialog
            id={id}
            class={format!("app-modal {class}")}
            node_ref={node_ref}
            aria-labelledby={(!labelledby.is_empty()).then_some(labelledby)}
            aria-describedby={(!describedby.is_empty()).then_some(describedby)}
            aria-busy={move || busy.get().to_string()}
            on:keydown={move |event: web_sys::KeyboardEvent| {
                if event.key()=="Escape" && !event.default_prevented() && !event.is_composing() {
                    event.prevent_default();event.stop_propagation();
                    if !busy.get_untracked(){on_close.run(());}
                }
            }}
            on:cancel={move |event: web_sys::Event| {
                event.prevent_default();
                if !busy.get_untracked() {
                    on_close.run(());
                }
            }}
        >
            {children()}
        </dialog>
    }
}

#[component]
pub fn ModalHeader(
    #[prop(into)] title_id: String,
    #[prop(into)] title: Signal<String>,
    on_close: Callback<()>,
    #[prop(optional)] close_id: &'static str,
    #[prop(optional)] class: &'static str,
    #[prop(optional, into)] busy: Signal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        <header class={format!("ui-modal-header {class}")}>
            <h2 id={title_id}>{move || title.get()}</h2>
            <crate::button::Button
                variant={crate::button::ButtonVariant::Ghost}
                disabled={Signal::derive(move || busy.get())}
                class="modal-close"
                {..}
                id={(!close_id.is_empty()).then_some(close_id)}
                type="button"
                data-close=""
                aria-label={move || i18n.text("message.close")}
                on:click={move |_| {
                    if !busy.get_untracked() {
                        on_close.run(());
                    }
                }}
            >
                <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.7"
                    stroke-linecap="round"
                    aria-hidden="true"
                >
                    <path d="m5 5 14 14M19 5 5 19" />
                </svg>
            </crate::button::Button>
        </header>
    }
}

#[component]
pub fn ModalBody(#[prop(optional)] class: &'static str, children: Children) -> impl IntoView {
    view! { <div class={format!("ui-modal-body {class}")}>{children()}</div> }
}

#[component]
pub fn ModalActions(children: Children) -> impl IntoView {
    view! { <div class="ui-modal-actions form-actions">{children()}</div> }
}

/// Persistent confirmations reuse their caller's cancellation and draft decisions.
#[component]
pub fn Confirmation(
    #[prop(into)] open: Signal<bool>,
    #[prop(optional, into)] busy: Signal<bool>,
    #[prop(optional, into)] error: Signal<String>,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = format!("confirmation-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let title_id = format!("{id}-title");
    let description_id = format!("{id}-description");
    let i18n = expect_context::<I18n>();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let close = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        if let Some(dialog) = dialog.get_untracked() {
            dialog.close();
        }
        on_close.run(());
    });
    Effect::new(move |_| {
        let visible = open.get();
        if let Some(dialog) = dialog.get() {
            if visible && !dialog.open() {
                let _ = dialog.show_modal();
            } else if !visible && dialog.open() {
                dialog.close();
            }
        }
    });
    view! {
        <Modal id=id labelledby=title_id.clone() describedby=description_id.clone() class="ui-confirmation" auto_open=false node_ref=dialog busy on_close=close>
            <ModalHeader title_id title=Signal::derive(move || i18n.text("message.confirm_action")) busy on_close=close/>
            <div id=description_id class="ui-modal-body ui-confirmation-body" on:click:capture=move |event: web_sys::MouseEvent| {
                use wasm_bindgen::JsCast;
                if !busy.get_untracked() && event.target().and_then(|target|target.dyn_into::<web_sys::Element>().ok()).is_some_and(|target|target.closest("[data-confirmation-cancel]").ok().flatten().is_some())
                    && let Some(dialog)=dialog.get_untracked(){dialog.close();}
            }>{children()}<p class="error" role="alert" hidden=move||error.get().is_empty()>{move||error.get()}</p></div>
        </Modal>
    }
}

/// Persistent recovery and review information must remain available beside its action.
#[component]
pub fn Notice(children: Children) -> impl IntoView {
    view! { <div class="ui-notice" role="status">{children()}</div> }
}
