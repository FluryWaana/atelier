use crate::i18n::I18n;
use leptos::{attribute_interceptor::AttributeInterceptor, prelude::*};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{JsFuture, spawn_local};

/// Native attributes after the spreading marker reach the input, including with actions.
#[component]
pub fn Input(
    #[prop(default = "text")] input_type: &'static str,
    #[prop(optional)] copy: bool,
    #[prop(optional)] reveal: bool,
    #[prop(optional, into)] disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let input = NodeRef::<leptos::html::Input>::new();
    let visible = RwSignal::new(false);
    let feedback = RwSignal::new("");
    Effect::new(move |_| {
        if disabled.get() {
            visible.set(false);
        }
    });
    view! {
        <AttributeInterceptor let:attrs>
            {
                let control = view! {
                    <input
                        class="ui-input"
                        node_ref={input}
                        disabled={move || disabled.get()}
                        type={move || if reveal && visible.get() { "text" } else { input_type }}
                        {..attrs}
                    />
                };
                if copy || reveal {
                    view! {
                        <div class="ui-input-actions">
                            <div class="ui-input-frame">
                                {control}
                                <div class="ui-input-buttons">
                                    {copy
                                        .then(|| {
                                            view! {
                                                <crate::button::Button
                                                    variant={crate::button::ButtonVariant::Ghost}
                                                    disabled={Signal::derive(move || disabled.get())}
                                                    class="ui-control-action"
                                                    {..}
                                                    type="button"
                                                    data-input-action="copy"
                                                    aria-label={move || i18n.text("message.form_copy")}
                                                    title={move || i18n.text("message.form_copy")}
                                                    on:click={move |_| {
                                                        let Some(input) = input.get() else {
                                                            return;
                                                        };
                                                        if input.matches(":disabled").unwrap_or(true) {
                                                            return;
                                                        }
                                                        let promise = window()
                                                            .navigator()
                                                            .clipboard()
                                                            .write_text(&input.value());
                                                        spawn_local(async move {
                                                            let message = if JsFuture::from(promise).await.is_ok() {
                                                                "message.form_copied"
                                                            } else {
                                                                "message.form_copy_failed"
                                                            };
                                                            let _ = feedback.try_set(message);
                                                            set_timeout(
                                                                move || {
                                                                    let _ = feedback.try_set("");
                                                                },
                                                                std::time::Duration::from_secs(3),
                                                            );
                                                        });
                                                    }}
                                                >
                                                    <svg
                                                        width="16"
                                                        height="16"
                                                        viewBox="0 0 24 24"
                                                        fill="none"
                                                        stroke="currentColor"
                                                        stroke-width="1.7"
                                                        aria-hidden="true"
                                                    >
                                                        <rect x="8" y="8" width="12" height="12" rx="2" />
                                                        <path d="M16 8V4H4v12h4" />
                                                    </svg>
                                                </crate::button::Button>
                                            }
                                        })}
                                    {reveal
                                        .then(|| {
                                            view! {
                                                <crate::button::Button
                                                    variant={crate::button::ButtonVariant::Ghost}
                                                    disabled={Signal::derive(move || disabled.get())}
                                                    class="ui-control-action"
                                                    {..}
                                                    type="button"
                                                    data-input-action="reveal"
                                                    aria-pressed={move || visible.get().to_string()}
                                                    aria-label={move || {
                                                        i18n.text(
                                                            if visible.get() {
                                                                "message.form_hide_value"
                                                            } else {
                                                                "message.form_show_value"
                                                            },
                                                        )
                                                    }}
                                                    title={move || {
                                                        i18n.text(
                                                            if visible.get() {
                                                                "message.form_hide_value"
                                                            } else {
                                                                "message.form_show_value"
                                                            },
                                                        )
                                                    }}
                                                    on:click={move |_| {
                                                        if let Some(input) = input.get()
                                                            && !input.matches(":disabled").unwrap_or(true) {
                                                                visible.update(|visible| *visible = !*visible);
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
                                                        aria-hidden="true"
                                                    >
                                                        <path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z" />
                                                        <circle cx="12" cy="12" r="3" />
                                                        {move || {
                                                            visible.get().then(|| view! { <path d="m3 3 18 18" /> })
                                                        }}
                                                    </svg>
                                                </crate::button::Button>
                                            }
                                        })}
                                </div>
                            </div>
                            <p
                                class="ui-control-feedback"
                                role="status"
                                hidden={move || feedback.get().is_empty()}
                            >
                                {move || i18n.text(feedback.get())}
                            </p>
                        </div>
                    }
                        .into_any()
                } else {
                    control.into_any()
                }
            }
        </AttributeInterceptor>
    }
}

#[component]
pub fn Textarea(#[prop(optional)] children: Option<Children>) -> impl IntoView {
    view! { <textarea class="ui-textarea">{children.map(|children| children())}</textarea> }
}

#[component]
pub fn Select(children: Children) -> impl IntoView {
    view! { <select class="ui-select">{children()}</select> }
}

#[component]
pub fn Checkbox() -> impl IntoView {
    view! { <input class="ui-checkbox" type="checkbox" /> }
}

#[component]
pub fn Switch() -> impl IntoView {
    view! { <input class="ui-switch" type="checkbox" role="switch" /> }
}

fn validation_message(element: &web_sys::Element, i18n: I18n) -> String {
    let validity = if let Some(input) = element.dyn_ref::<web_sys::HtmlInputElement>() {
        input.validity()
    } else if let Some(select) = element.dyn_ref::<web_sys::HtmlSelectElement>() {
        select.validity()
    } else if let Some(textarea) = element.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        textarea.validity()
    } else {
        return String::new();
    };
    if validity.valid() {
        return String::new();
    }
    if validity.value_missing() {
        return i18n.text("message.form_required");
    }
    for (invalid, key, attribute) in [
        (validity.range_underflow(), "message.form_min", "min"),
        (validity.range_overflow(), "message.form_max", "max"),
        (validity.too_short(), "message.form_min_length", "minlength"),
        (validity.too_long(), "message.form_max_length", "maxlength"),
    ] {
        if invalid {
            return i18n
                .text(key)
                .replace("{0}", &element.get_attribute(attribute).unwrap_or_default());
        }
    }
    if validity.type_mismatch() {
        return i18n.text(
            if element.get_attribute("type").as_deref() == Some("email") {
                "message.form_email"
            } else {
                "message.form_url"
            },
        );
    }
    if validity.bad_input() || validity.step_mismatch() {
        return i18n.text("message.form_number");
    }
    i18n.text("message.form_format")
}

/// Labels, help and native validation stay connected around controls with or without actions.
#[component]
pub fn Field(
    #[prop(optional, into)] id: String,
    #[prop(into)] label: Signal<String>,
    #[prop(optional, into)] help: Signal<String>,
    #[prop(optional, into)] error: Signal<String>,
    #[prop(optional)] label_class: &'static str,
    #[prop(optional)] class: &'static str,
    children: Children,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_FIELD: AtomicUsize = AtomicUsize::new(0);
    let id = if id.is_empty() {
        format!("ui-field-{}", NEXT_FIELD.fetch_add(1, Ordering::Relaxed))
    } else {
        id
    };
    let id = StoredValue::new(id);
    let root = NodeRef::<leptos::html::Div>::new();
    let local_error = RwSignal::new(String::new());
    let shown_error = Memo::new(move |_| {
        let external = error.get();
        if external.is_empty() {
            local_error.get()
        } else {
            external
        }
    });
    let validate = move |target: Option<web_sys::EventTarget>| {
        if let Some(element) = target.and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            && matches!(element.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA")
        {
            local_error.set(validation_message(&element, i18n));
        }
    };
    Effect::new(move |_| {
        let invalid = !shown_error.get().is_empty();
        if let Some(root) = root.get()
            && let Ok(Some(control)) = root.query_selector("input,select,textarea")
        {
            let _ = control.set_attribute("id", &id.get_value());
            let mut descriptions = control
                .get_attribute("aria-describedby")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for suffix in ["field-help", "field-error"] {
                let related = format!("{}-{suffix}", id.get_value());
                if !descriptions.contains(&related) {
                    descriptions.push(related);
                }
            }
            let _ = control.set_attribute("aria-describedby", &descriptions.join(" "));
            let _ = control.set_attribute("aria-invalid", if invalid { "true" } else { "false" });
        }
    });
    view! {
        <div
            class=format!("ui-field {class}")
            node_ref={root}
            on:invalid:capture={move |event: web_sys::Event| {
                event.prevent_default();
                validate(event.target());
            }}
            on:focusout={move |event: web_sys::FocusEvent| validate(event.target())}
            on:input={move |event: web_sys::Event| {
                if !local_error.get_untracked().is_empty() {
                    validate(event.target());
                }
            }}
            on:change={move |event: web_sys::Event| {
                if !local_error.get_untracked().is_empty() {
                    validate(event.target());
                }
            }}
        >
            <label class=label_class for={id.get_value()}>{move || label.get()}</label>
            {children()}
            <p
                id={format!("{}-field-help", id.get_value())}
                class="field-help"
                hidden={move || help.get().is_empty()}
            >
                {move || help.get()}
            </p>
            <p
                id={format!("{}-field-error", id.get_value())}
                class="error"
                role="alert"
                hidden={move || shown_error.get().is_empty()}
            >
                {move || shown_error.get()}
            </p>
        </div>
    }
}

/// Validate once, reveal a hidden panel if needed, then focus the first invalid control.
#[component]
pub fn Form(
    on_submit: Callback<leptos::ev::SubmitEvent>,
    #[prop(optional)] on_invalid: Option<Callback<web_sys::Element>>,
    #[prop(optional, into)] busy: Signal<bool>,
    children: Children,
) -> impl IntoView {
    let form = NodeRef::<leptos::html::Form>::new();
    view! {
        <form
            node_ref={form}
            novalidate
            aria-busy={move || busy.get().to_string()}
            on:submit={move |event: leptos::ev::SubmitEvent| {
                event.prevent_default();
                if busy.get_untracked() {
                    return;
                }
                let Some(form) = form.get() else {
                    return;
                };
                if !form.check_validity() {
                    if let Ok(Some(invalid)) = form
                        .query_selector("input:invalid,select:invalid,textarea:invalid")
                    {
                        if let Some(on_invalid) = on_invalid {
                            on_invalid.run(invalid.clone());
                        }
                        let mut ancestor=invalid.parent_element();
                        while let Some(element)=ancestor {
                            if element.tag_name()=="DETAILS" {let _=element.set_attribute("open","");}
                            ancestor=element.parent_element();
                        }

                        request_animation_frame(move || {
                            if let Some(element) = invalid.dyn_ref::<web_sys::HtmlElement>() {
                                let _ = element.focus();
                            }
                        });
                    }
                    return;
                }
                on_submit.run(event);
            }}
        >
            {children()}
        </form>
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// Searchable selection. Typing filters options and only a chosen option changes the value.
#[component]
pub fn Combobox(
    #[prop(into)] id: String,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] options: Signal<Vec<Choice>>,
    on_change: Callback<String>,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] describedby: String,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(id);
    let list_id = StoredValue::new(format!("{}-listbox", id.get_value()));
    let query = RwSignal::new(String::new());
    let opened = RwSignal::new(false);
    let active = RwSignal::new(0usize);
    let input = NodeRef::<leptos::html::Input>::new();
    let selected = Memo::new(move |_| {
        options
            .get()
            .into_iter()
            .find(|item| item.value == value.get())
            .map(|item| item.label)
            .unwrap_or_default()
    });
    let filtered = Memo::new(move |_| {
        let query = query.get().trim().to_lowercase();
        options
            .get()
            .into_iter()
            .filter(|item| item.label.to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    });
    let close = move || {
        opened.set(false);
        query.set(String::new());
        active.set(0);
    };
    let choose = Callback::new(move |position: usize| {
        if disabled.get_untracked() {
            return;
        }
        if let Some(item) = filtered.get_untracked().get(position) {
            on_change.run(item.value.clone());
            close();
        }
    });
    Effect::new(move |_| {
        if disabled.get() {
            close();
        }
    });
    Effect::new(move |_| {
        let index = active.get();
        let visible = opened.get();
        let count = filtered.get().len();
        if visible && count > 0 {
            request_animation_frame(move || {
                if let Some(option) = document().get_element_by_id(&format!(
                    "{}-option-{}",
                    id.get_value(),
                    index.min(count - 1)
                )) {
                    let options = web_sys::ScrollIntoViewOptions::new();
                    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    option.scroll_into_view_with_scroll_into_view_options(&options);
                }
            });
        }
    });
    view! {
        <div class="ui-combobox">
            <div class="ui-combobox-control">
                <input
                    class="ui-input"
                    id={id.get_value()}
                    node_ref={input}
                    type="text"
                    role="combobox"
                    autocomplete="off"
                    disabled={move || disabled.get()}
                    aria-autocomplete="list"
                    aria-expanded={move || opened.get().to_string()}
                    aria-controls={list_id.get_value()}
                    aria-describedby={describedby}
                    data-selected-value={move || value.get()}
                    aria-activedescendant={move || {
                        if opened.get() && !filtered.get().is_empty() {
                            Some(
                                format!(
                                    "{}-option-{}",
                                    id.get_value(),
                                    active.get().min(filtered.get().len() - 1),
                                ),
                            )
                        } else {
                            None
                        }
                    }}
                    prop:value={move || if opened.get() { query.get() } else { selected.get() }}
                    on:focus={move |_| {
                        if !disabled.get_untracked() {
                            opened.set(true);
                            query.set(String::new());
                            active.set(0);
                        }
                    }}
                    on:click={move |_| {
                        if !disabled.get_untracked() && !opened.get_untracked() {
                            query.set(String::new());
                            active.set(0);
                            opened.set(true);
                        }
                    }}
                    on:input={move |event| {
                        query.set(event_target_value(&event));
                        active.set(0);
                        opened.set(true);
                    }}
                    on:blur={move |_| close()}
                    on:keydown={move |event: web_sys::KeyboardEvent| {
                        if disabled.get_untracked() || event.is_composing() {
                            return;
                        }
                        let count = filtered.get_untracked().len();
                        match event.key().as_str() {
                            "ArrowDown" => {
                                event.prevent_default();
                                if !opened.get_untracked() {
                                    opened.set(true);
                                    query.set(String::new());
                                    active.set(0);
                                } else if count > 0 {
                                    active.update(|index| *index = (*index + 1) % count);
                                }
                            }
                            "ArrowUp" => {
                                event.prevent_default();
                                opened.set(true);
                                if count > 0 {
                                    active.update(|index| *index = (*index + count - 1) % count);
                                }
                            }
                            "Home" if opened.get_untracked() => {
                                event.prevent_default();
                                active.set(0);
                            }
                            "End" if opened.get_untracked() => {
                                event.prevent_default();
                                active.set(count.saturating_sub(1));
                            }
                            "Enter" if opened.get_untracked() => {
                                event.prevent_default();
                                choose.run(active.get_untracked());
                            }
                            "Escape" if opened.get_untracked() => {
                                event.prevent_default();
                                event.stop_propagation();
                                close();
                            }
                            "Tab" => close(),
                            _ => {}
                        }
                    }}
                />
                <crate::button::Button
                    variant={crate::button::ButtonVariant::Ghost}
                    disabled={Signal::derive(move || disabled.get())}
                    class="ui-combobox-toggle"
                    {..}
                    type="button"
                    tabindex="-1"
                    aria-label={move || i18n.text("message.form_show_options")}
                    aria-controls={list_id.get_value()}
                    aria-expanded={move || opened.get().to_string()}
                    on:mousedown={move |event| event.prevent_default()}
                    on:click={move |_| {
                        if opened.get_untracked() {
                            close();
                        } else {
                            if let Some(input) = input.get() {
                                let _ = input.focus();
                            }
                            query.set(String::new());
                            active.set(0);
                            opened.set(true);
                        }
                    }}
                >
                    <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.7"
                        aria-hidden="true"
                    >
                        <path d="m6 9 6 6 6-6" />
                    </svg>
                </crate::button::Button>
            </div>
            <div class="ui-combobox-popup" hidden={move || !opened.get()}>
                <ul id={list_id.get_value()} role="listbox">
                    <For
                        each={move || { filtered.get().into_iter().enumerate().collect::<Vec<_>>() }}
                        key={|(index, item)| (*index, item.value.clone())}
                        children={move |(index, item)| {
                            view! {
                                <li
                                    id={format!("{}-option-{index}", id.get_value())}
                                    role="option"
                                    aria-selected={move || (value.get() == item.value).to_string()}
                                    class:active={move || active.get() == index}
                                    on:mousedown={move |event| event.prevent_default()}
                                    on:click={move |_| choose.run(index)}
                                >
                                    {item.label}
                                </li>
                            }
                        }}
                    />
                </ul>
                <p class="field-help" role="status" hidden={move || !filtered.get().is_empty()}>
                    {move || i18n.text("message.form_no_options")}
                </p>
            </div>
        </div>
    }
}
