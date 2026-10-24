use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::contracts::{Worktree, WorktreeState};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Workspace(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    on_change: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let current = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .and_then(|card| card.fields.get::<Worktree>("worktree").ok().flatten())
        })
    });
    let defaults = move || {
        let card = board
            .cards
            .get_untracked()
            .into_iter()
            .find(|card| card.id == id.get_value());
        let branch = card
            .as_ref()
            .map(|card| field(&card.fields, "branch"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("atelier/{}", id.get_value()));
        let base = card
            .as_ref()
            .map(|card| field(&card.fields, "baseBranch"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field(&board.settings.get_untracked(), "baseBranch"));
        (branch, if base.is_empty() { "HEAD".into() } else { base })
    };
    let initial = defaults();
    let branch = RwSignal::new(initial.0);
    let base = RwSignal::new(initial.1);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let confirming = RwSignal::new(false);
    let discard = RwSignal::new(false);
    let act = move |remove: bool| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/cards/{}/worktree{}",
            id.get_value(),
            if remove { "/remove" } else { "" }
        );
        let body = if remove {
            serde_json::json!({"confirmed":true,"discardChanges":discard.get_untracked()})
        } else {
            serde_json::json!({"branch":branch.get_untracked(),"base":base.get_untracked()})
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        on_change.run(());
        spawn_local(async move {
            let result = write(&token, Method::POST, &path, &body).await;
            let _ = board.refresh(&token).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(_) => {
                    dirty.set(false);
                    confirming.set(false);
                    discard.set(false);
                    notice.set(
                        if remove {
                            "Worktree retiré. La branche Git est conservée."
                        } else {
                            "Worktree prêt."
                        }
                        .into(),
                    );
                    request_animation_frame(move || {
                        if let Some(element) = web_sys::window()
                            .and_then(|window| window.document())
                            .and_then(|document| {
                                document.get_element_by_id(if remove {
                                    "create-worktree"
                                } else {
                                    "verify-worktree"
                                })
                            })
                            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
                        {
                            let _ = element.focus();
                        }
                    });
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    view! {
        <section class="task-files" aria-labelledby="workspace-heading" aria-busy=move || pending.get().to_string()>
            <h3 id="workspace-heading">{move || i18n.text("Espace de travail Git")}</h3>
            <p class="field-help">{move || i18n.text("Un worktree isole les fichiers de cette tâche dans un dossier et une branche dédiés.")}</p>
            <p class="field-help">{move || i18n.text("La préparation et la vérification actualisent le contexte local des agents. Les fichiers suivis par Git et tes modifications dans le worktree sont conservés ; les copies locales restent exclues des nouveaux fichiers Git.")}</p>
            <p class="field-help" hidden=move || !blocked.get()>{move || i18n.text("Enregistre ou abandonne les modifications de la tâche avant de gérer son worktree.")}</p>
            <form hidden=move || current.get().is_some() on:submit=move |event| { event.prevent_default(); act(false); }>
                <fieldset disabled=move || pending.get() || blocked.get()>
                    <label for="worktree-branch">{move || i18n.text("Branche de la tâche")}</label><input id="worktree-branch" required maxlength="180" prop:value=move || branch.get() on:input=move |event| { branch.set(event_target_value(&event)); dirty.set(true); } />
                    <label for="worktree-base">{move || i18n.text("Référence de départ")}</label><input id="worktree-base" required maxlength="180" prop:value=move || base.get() on:input=move |event| { base.set(event_target_value(&event)); dirty.set(true); } />
                    <p class="field-help">{move || i18n.text("Utilise une référence disponible localement, par exemple main ou origin/main. La préparation ne télécharge pas le dépôt distant.")}</p>
                    <div class="editor-actions"><button id="create-worktree" type="submit">{move || i18n.text("Préparer le worktree")}</button><button type="button" hidden=move || !dirty.get() on:click=move |_| { let initial = defaults(); branch.set(initial.0); base.set(initial.1); dirty.set(false); }>{move || i18n.text("Réinitialiser")}</button></div>
                </fieldset>
            </form>
            <div hidden=move || current.get().is_none()>
                <p class="workspace-path">{move || current.get().map(|worktree| format!("{} · {}", worktree.branch, worktree.path)).unwrap_or_default()}</p>
                <p id="worktree-state" role="status">{move || i18n.text(match current.get().map(|worktree| worktree.state) { Some(WorktreeState::Ready) => "Worktree prêt.", Some(WorktreeState::Creating | WorktreeState::Removing) => "Opération à terminer ou à vérifier. Les fichiers sont conservés jusqu’au retrait confirmé.", None => "" })}</p>
                <div class="editor-actions"><button id="verify-worktree" type="button" disabled=move || pending.get() || blocked.get() on:click=move |_| act(false)>{move || i18n.text("Vérifier et reprendre")}</button><button id="remove-worktree" type="button" class="danger" disabled=move || pending.get() || blocked.get() on:click=move |_| { discard.set(false); confirming.set(true); }>{move || i18n.text("Retirer le worktree…")}</button></div>
            </div>
            <div class="confirmation" hidden=move || !confirming.get()>
                <p>{move || i18n.text("Retirer ce dossier de travail ? La branche et ses commits restent dans Git.")}</p>
                <label class="check-filter"><input id="discard-worktree-files" type="checkbox" disabled=move || pending.get() prop:checked=move || discard.get() on:change=move |event| discard.set(event_target_checked(&event)) />{move || i18n.text("Abandonner aussi les fichiers modifiés, nouveaux et ignorés de ce worktree. Cette suppression est irréversible.")}</label>
                <button id="confirm-remove-worktree" class="danger" type="button" disabled=move || pending.get() || blocked.get() on:click=move |_| act(true)>{move || i18n.text("Confirmer le retrait")}</button>
                <button type="button" disabled=move || pending.get() on:click=move |_| confirming.set(false)>{move || i18n.text("Annuler")}</button>
            </div>
            <p id="worktree-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="worktree-notice" role="status">{move || i18n.text(&notice.get())}</p>
        </section>
    }
}
