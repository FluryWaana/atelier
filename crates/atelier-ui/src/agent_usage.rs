use crate::i18n::I18n;
use atelier_core::{
    application::agent_usage::{Counter, summarize},
    document::Document,
    domain::time::duration_text,
};
use leptos::prelude::*;
#[component]
pub fn AgentUsage(runs: Signal<Vec<Document>>) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let page = RwSignal::new(0usize);
    let report = Memo::new(move |_| summarize(&runs.get()));
    let coverage = move |counter: &Counter, total: u64, duration: bool| {
        if counter.samples == 0 {
            return i18n.text("message.unavailable");
        }
        let value = if duration {
            duration_text(counter.total / 1000)
        } else {
            counter.total.to_string()
        };
        format!("{value} ({}/{total})", counter.samples)
    };
    Effect::new(move |_| {
        let last = report.with(|report| {
            report
                .as_ref()
                .map(|groups| groups.len().saturating_sub(1) / 100)
                .unwrap_or(0)
        });
        if page.get() > last {
            page.set(last);
        }
    });
    view! {
        <details id="agent-usage-report">
            <summary>{move || i18n.text("message.recorded_usage_by_agent_and_model")}</summary>
            <p class="field-help">
                {move || i18n.text("message.raw_counters_reported_by_clis_grouped_by_type_profile_and_model")}
            </p>
            <p class="field-help">
                {move || {
                    i18n.text("message.each_counter_shows_how_many_selected_sessions_provided_it_missin")
                }}
            </p>
            <p class="error" role="alert">
                {move || {
                    report
                        .with(|report| {
                            report.as_ref().err().map(|message| i18n.text(message)).unwrap_or_default()
                        })
                }}
            </p>
            <p hidden={move || {
                report.with(|report| report.as_ref().map_or(true, |groups| !groups.is_empty()))
            }}>{move || i18n.text("message.no_agent_sessions_in_this_selection")}</p>
            <div
                class="table-scroll"
                role="region"
                tabindex="0"
                aria-label={move || i18n.text("message.recorded_usage_by_agent_and_model")}
            >
                <table>
                    <thead>
                        <tr>
                            <th scope="col">{move || i18n.text("message.agent_profile_model")}</th>
                            <th scope="col">{move || i18n.text("message.sessions")}</th>
                            <th scope="col">{move || i18n.text("message.reported_input")}</th>
                            <th scope="col">{move || i18n.text("message.reported_cache_reads")}</th>
                            <th scope="col">{move || i18n.text("message.reported_cache_creation")}</th>
                            <th scope="col">{move || i18n.text("message.reported_output")}</th>
                            <th scope="col">{move || i18n.text("message.active_duration_91cdaea5")}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <For
                            each={move || {
                                report
                                    .with(|report| {
                                        report
                                            .as_ref()
                                            .map(|groups| {
                                                groups
                                                    .iter()
                                                    .skip(page.get() * 100)
                                                    .take(100)
                                                    .cloned()
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default()
                                    })
                            }}
                            key={|group| group.clone()}
                            children={move |group| {
                                view! {
                                    <tr>
                                        <th scope="row">
                                            {move || {
                                                format!(
                                                    "{} / {} / {}",
                                                    if group.agent_type.is_empty() {
                                                        i18n.text("message.not_specified")
                                                    } else {
                                                        group.agent_type.clone()
                                                    },
                                                    if group.agent.is_empty() {
                                                        i18n.text("message.not_specified")
                                                    } else {
                                                        group.agent.clone()
                                                    },
                                                    if group.model.is_empty() {
                                                        i18n.text("message.not_specified")
                                                    } else {
                                                        group.model.clone()
                                                    },
                                                )
                                            }}
                                        </th>
                                        <td>{group.runs}</td>
                                        <td>{move || coverage(&group.input, group.runs, false)}</td>
                                        <td>{move || coverage(&group.cache_read, group.runs, false)}</td>
                                        <td>{move || coverage(&group.cache_creation, group.runs, false)}</td>
                                        <td>{move || coverage(&group.output, group.runs, false)}</td>
                                        <td>
                                            {move || coverage(&group.active_milliseconds, group.runs, true)}
                                        </td>
                                    </tr>
                                }
                            }}
                        />
                    </tbody>
                </table>
            </div>
            <div
                class="editor-actions"
                hidden={move || {
                    report.with(|report| report.as_ref().map_or(true, |groups| groups.len() <= 100))
                }}
            >
                <button
                    type="button"
                    disabled={move || page.get() == 0}
                    on:click={move |_| page.update(|page| *page = page.saturating_sub(1))}
                >
                    {move || i18n.text("message.previous")}
                </button>
                <button
                    type="button"
                    disabled={move || {
                        report
                            .with(|report| {
                                report.as_ref().map_or(true, |groups| (page.get() + 1) * 100 >= groups.len())
                            })
                    }}
                    on:click={move |_| page.update(|page| *page += 1)}
                >
                    {move || i18n.text("message.next")}
                </button>
            </div>
        </details>
    }
}
