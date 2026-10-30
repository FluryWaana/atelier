use leptos::prelude::*;

/// Card surfaces keep their native article semantics and their caller's interactions.
#[component]
pub fn Card(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! { <article class={format!("ui-card {class}")}>{children()}</article> }
}

#[component]
pub fn CardHeader(children: Children) -> impl IntoView {
    view! { <div class="ui-card-header">{children()}</div> }
}

#[component]
pub fn CardFooter(#[prop(optional, into)] class: String, children: Children) -> impl IntoView {
    view! { <div class={format!("ui-card-footer {class}")}>{children()}</div> }
}
