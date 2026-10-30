use leptos::prelude::*;

#[derive(Clone, Copy, Default)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

impl ButtonVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Ghost => "ghost",
            Self::Danger => "danger",
        }
    }
}

/// Native attributes and events are forwarded to the button after the spreading marker.
#[component]
pub fn Button(
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional, into)] class: String,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] busy: Signal<bool>,
    children: Children,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class={format!("ui-button {class}")}
            data-variant={variant.name()}
            disabled={move || disabled.get() || busy.get()}
            aria-busy={move || busy.get().to_string()}
        >
            {children()}
        </button>
    }
}
