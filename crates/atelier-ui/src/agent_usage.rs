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
            return i18n.text("Indisponible");
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
    view! {<details id="agent-usage-report"><summary>{move||i18n.text("Usage enregistré par agent et modèle")}</summary>
        <p class="field-help">{move||i18n.text("Compteurs bruts déclarés par les CLI, regroupés par type, profil et modèle. Les entrées et caches dépendent du fournisseur ; ne les additionne pas pour calculer une facture. Aucune estimation de coût ni quota distant vérifié.")}</p>
        <p class="field-help">{move||i18n.text("Chaque compteur indique les sessions qui l’ont fourni sur le total sélectionné. Une valeur absente reste indisponible ; les sessions actives peuvent encore augmenter. Les filtres de projet et de date s’appliquent, à la date de début UTC.")}</p>
        <p class="error" role="alert">{move||report.with(|report|report.as_ref().err().map(|message|i18n.text(message)).unwrap_or_default())}</p>
        <p hidden=move||report.with(|report|report.as_ref().map_or(true,|groups|!groups.is_empty()))>{move||i18n.text("Aucune session d’agent dans cette sélection.")}</p>
        <div class="table-scroll" role="region" tabindex="0" aria-label=move||i18n.text("Usage enregistré par agent et modèle")><table><thead><tr><th scope="col">{move||i18n.text("Agent / profil / modèle")}</th><th scope="col">{move||i18n.text("Sessions")}</th><th scope="col">{move||i18n.text("Entrée déclarée")}</th><th scope="col">{move||i18n.text("Cache lu déclaré")}</th><th scope="col">{move||i18n.text("Cache créé déclaré")}</th><th scope="col">{move||i18n.text("Sortie déclarée")}</th><th scope="col">{move||i18n.text("Durée active")}</th></tr></thead><tbody><For each=move||report.with(|report|report.as_ref().map(|groups|groups.iter().skip(page.get()*100).take(100).cloned().collect::<Vec<_>>()).unwrap_or_default()) key=|group|group.clone() children=move|group|view!{<tr><th scope="row">{move||format!("{} / {} / {}",if group.agent_type.is_empty(){i18n.text("Non renseigné")}else{group.agent_type.clone()},if group.agent.is_empty(){i18n.text("Non renseigné")}else{group.agent.clone()},if group.model.is_empty(){i18n.text("Non renseigné")}else{group.model.clone()})}</th><td>{group.runs}</td><td>{move||coverage(&group.input,group.runs,false)}</td><td>{move||coverage(&group.cache_read,group.runs,false)}</td><td>{move||coverage(&group.cache_creation,group.runs,false)}</td><td>{move||coverage(&group.output,group.runs,false)}</td><td>{move||coverage(&group.active_milliseconds,group.runs,true)}</td></tr>}/></tbody></table></div>
        <div class="editor-actions" hidden=move||report.with(|report|report.as_ref().map_or(true,|groups|groups.len()<=100))><button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled=move||report.with(|report|report.as_ref().map_or(true,|groups|(page.get()+1)*100>=groups.len())) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button></div>
    </details>}
}
