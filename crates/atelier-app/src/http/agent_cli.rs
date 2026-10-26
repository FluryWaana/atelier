use super::{Shared, failure, responses::response};
use crate::infrastructure::{agent_cli as workflow, agent_providers};
use atelier_core::{
    application::agent_cli::{Operation, Phase, UpdateRequest},
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
pub(super) async fn update(
    State(state): State<Shared>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    let input: UpdateRequest = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme la version et l’installation CLI affichées.",
        );
    }
    let expected = input.expected;
    let info = input.info;
    let selected = provider.clone();
    let vault = state.vault.clone();
    let snapshot = info.clone();
    let (profile, operation) = match state
        .database
        .call(move |store| {
            let mut body = Document::default();
            body.set("expected", &expected)?;
            let profile = agent_providers::diagnostic_profile(
                store,
                &body,
                &selected,
                &mut |value, decrypt| vault.protect(value, decrypt),
            )?;
            let operation = workflow::reserve(store, &selected, snapshot)?;
            Ok((profile, operation))
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return response::<Operation>(Err(error)),
    };
    let _ = state.changed.send(());
    let id = operation.id;
    let mut stopped = state.shutdown.subscribe();
    let result=async {
        let (mut command,pin,_lock)=workflow::prepare(&profile,&info).await?;
        if *stopped.borrow(){return Err("Atelier est en cours d’arrêt.".into());}
        let selected=id.clone();state.database.call(move|store|workflow::change(store,&selected,Phase::Running,None,"Commande de mise à jour CLI lancée ; résultat non encore observé.")).await?;
        let _=state.changed.send(());
        let guard=async {let stopping=*stopped.borrow();if !stopping{let _=stopped.changed().await;}crate::Error::from("Mise à jour CLI interrompue par l’arrêt d’Atelier.")};
        let output=crate::infrastructure::process_tree::capture_pinned(&mut command,pin,std::time::Duration::from_secs(180),1024*1024,guard).await?;
        if !output.status.success(){return Err("La commande de mise à jour CLI a échoué. Relis l’installation ; aucune nouvelle tentative automatique.".into());}
        let version=workflow::observed_version(&profile,&info).await?;
        let selected=id.clone();state.database.call(move|store|workflow::change(store,&selected,Phase::Observed,Some(version),"Version CLI relue après la commande. Consulte la cible et la version réellement observée.")).await
    }.await;
    let result = match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let selected = id;
            match state.database.call(move|store|{
            if let Some(operation)=workflow::read(store)?.filter(|operation|operation.id==selected){match operation.phase {Phase::Preparing=>{workflow::change(store,&selected,Phase::Failed,None,"Préflight CLI refusé ; commande non envoyée.")?;},Phase::Running=>{workflow::change(store,&selected,Phase::Uncertain,None,"Résultat de mise à jour CLI incertain. Relis le diagnostic avant toute nouvelle confirmation.")?;},_=>{}}}Ok(())
        }).await {Ok(())=>Err(error),Err(recording)=>Err(recording)}
        }
    };
    let _ = state.changed.send(());
    response(result)
}
