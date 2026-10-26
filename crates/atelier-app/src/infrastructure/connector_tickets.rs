use super::{
    connectors,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{
    application::{
        extensions::{
            self as protocol, FieldKind, ImportedTicket, Installed, Report, TicketImport,
            TicketPage,
        },
        task_creation,
    },
    contracts::Card,
    document::Document,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn scope(profile: &Installed) -> Result<String> {
    let fields: BTreeMap<_, _> = profile
        .manifest
        .fields
        .iter()
        .filter(|field| field.kind == FieldKind::Url || field.credential_scope)
        .filter(|field| field.kind != FieldKind::Password)
        .map(|field| (&field.id, profile.values.get(&field.id)))
        .collect();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            "atelier-ticket-scope/1",
            &profile.manifest.id,
            fields
        ))?)
    ))
}

pub fn page(report: &Report) -> Result<TicketPage> {
    let page = protocol::ticket_page(report.method, &report.output)?;
    for ticket in &page.items {
        if ticket.instance.chars().any(char::is_control)
            || ticket.instance.contains(['\\', '%'])
            || super::jira::normalize_url(&ticket.instance)? != ticket.instance
        {
            return Err("Portée HTTPS du ticket de connecteur invalide.".into());
        }
        let instance = reqwest::Url::parse(&ticket.instance)?;
        let url = reqwest::Url::parse(&ticket.url)?;
        let boundary = instance.path().trim_end_matches('/');
        if ticket.url.chars().any(char::is_control)
            || ticket.url.contains('\\')
            || url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || instance.origin() != url.origin()
            || !(url.path() == boundary
                || url
                    .path()
                    .strip_prefix(boundary)
                    .is_some_and(|path| path.starts_with('/')))
        {
            return Err("Adresse du ticket hors de la portée du connecteur.".into());
        }
    }
    Ok(page)
}

pub(super) fn linked(profile: &Installed, card: &Card) -> Result<Document> {
    let ticket = card
        .fields
        .get::<Document>("ticket")?
        .ok_or("Ticket de connecteur absent.")?;
    if !ticket.string_equals("connector", &format!("extension:{}", profile.manifest.id))?
        || !ticket.string_equals("scope", &scope(profile)?)?
    {
        return Err("La portée du connecteur a changé. Rétablis sa configuration avant de synchroniser cette carte.".into());
    }
    Ok(ticket)
}

pub fn imported_count(store: &Store<'_>, profile: &Installed) -> Result<usize> {
    let scope = scope(profile)?;
    let connector = format!("extension:{}", profile.manifest.id);
    let mut count = 0;
    for card in store.documents::<Card>(Documents::Cards)? {
        if let Some(ticket) = card.fields.get::<Document>("ticket")?
            && ticket.string_equals("connector", &connector)?
            && ticket.string_equals("scope", &scope)?
        {
            count += 1;
        }
    }
    Ok(count)
}

pub fn refresh(store: &Store<'_>, module: &str, input: &protocol::TicketRefresh) -> Result<Card> {
    if !input.confirmed {
        return Err("Confirme les champs du ticket à synchroniser.".into());
    }
    store.transaction(|store| {
        let profile = connectors::profile(store, module, input.expected_revision)?;
        if !profile
            .manifest
            .methods
            .contains(&protocol::Method::TicketsRead)
        {
            return Err("Ce connecteur ne déclare pas la lecture des tickets.".into());
        }
        let report = profile
            .report
            .as_ref()
            .filter(|report| {
                report.id == input.report_id && report.method == protocol::Method::TicketsRead
            })
            .ok_or("Le rapport du connecteur a changé. Relis le ticket avant de confirmer.")?;
        let observed = time::OffsetDateTime::parse(
            &report.observed_at,
            &time::format_description::well_known::Rfc3339,
        )?;
        if !(0..=600).contains(&(time::OffsetDateTime::now_utc() - observed).whole_seconds()) {
            return Err(
                "Cet aperçu de connecteur a expiré. Relis le ticket avant de confirmer.".into(),
            );
        }
        let page = page(report)?;
        let remote = &page.items[0];
        let mut card = super::forge_association::snapshot(store, &input.expected)?;
        let mut ticket = linked(&profile, &card)?;
        if !ticket.string_equals("instance", &remote.instance)?
            || !ticket.string_equals("id", &remote.id)?
        {
            return Err("Le rapport ne correspond pas au ticket lié à cette carte.".into());
        }
        let changed = (input.replace_title && card.title != remote.title.as_str().into())
            || (input.replace_description
                && card
                    .fields
                    .get::<String>("description")?
                    .unwrap_or_default()
                    != remote.description);
        if input.replace_title {
            card.title = remote.title.as_str().into();
        }
        if input.replace_description {
            card.fields.set("description", &remote.description)?;
        }
        ticket.set("key", &remote.key)?;
        ticket.set("url", &remote.url)?;
        ticket.set("status", &remote.status)?;
        ticket.set("reportId", &report.id)?;
        card.fields.set("ticket", &ticket)?;
        if changed {
            let invalidation = atelier_core::application::task_editing::invalidate_reviews(
                &card.id,
                &mut card.fields,
                store.documents(Documents::Runs)?,
                atelier_core::application::task_editing::REVIEW_CHANGE_REASON,
            )?;
            for run in invalidation.runs {
                store.put(Documents::Runs, &run)?;
            }
        }
        card.fields.set(
            "updatedAt",
            &task_creation::CreationContext::timestamp(store)?,
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}

pub fn import(
    store: &Store<'_>,
    module: &str,
    input: &TicketImport,
) -> Result<Vec<ImportedTicket>> {
    if !input.confirmed
        || input.tickets.is_empty()
        || input.tickets.len() > 20
        || input.tickets.iter().collect::<BTreeSet<_>>().len() != input.tickets.len()
    {
        return Err("Confirme une sélection de 1 à 20 tickets distincts du connecteur.".into());
    }
    store.transaction(|store|{
        let profile=connectors::profile(store,module,input.expected_revision)?;
        let report=profile.report.as_ref().filter(|report|report.id==input.report_id)
            .ok_or("Les résultats du connecteur ont changé. Relance la recherche.")?;
        let page=page(report)?;
        let project_id=input.project.get::<String>("id")?.ok_or("Projet introuvable.")?;
        let project=store.document::<Document>(Documents::Projects,&project_id)?.ok_or("Projet introuvable.")?;
        if serde_json::to_value(&project)?!=serde_json::to_value(&input.project)? {
            return Err("Le projet a changé. Sélectionne à nouveau sa version actuelle.".into());
        }
        let repository=project.get::<String>("repo")?.filter(|repo|!repo.is_empty()).ok_or("Dépôt du projet absent.")?;
        let scope=scope(&profile)?;
        let connector=format!("extension:{module}");
        let mut cards=store.documents::<Card>(Documents::Cards)?;
        let mut imported=Vec::new();
        for identity in &input.tickets {
            let ticket=page.items.iter().find(|ticket|ticket.identity()==*identity)
                .ok_or("Le ticket sélectionné est absent du rapport courant.")?;
            let mut existing=None;
            for card in &cards {
                if let Some(link)=card.fields.get::<Document>("ticket")?
                    && link.string_equals("connector",&connector)? && link.string_equals("scope",&scope)?
                    && link.string_equals("instance",&ticket.instance)? && link.string_equals("id",&ticket.id)? {
                    if existing.is_some(){return Err("Plusieurs cartes sont liées au même ticket de connecteur.".into());}
                    existing=Some(card.clone());
                }
            }
            if let Some(card)=existing {imported.push(ImportedTicket{card,created:false});continue;}
            let mut body=Document::default();
            body.set("title",&ticket.title)?;
            body.set("description",&ticket.description)?;
            body.set("repo",&repository)?;
            let mut card=task_creation::create(store,&body,false)?;
            card.fields.set("ticket",&serde_json::json!({"connector":connector,"scope":scope,"instance":ticket.instance,"id":ticket.id,"key":ticket.key,"url":ticket.url,"status":ticket.status,"reportId":report.id}))?;
            store.put(Documents::Cards,&card)?;
            cards.push(card.clone());
            imported.push(ImportedTicket{card,created:true});
        }
        Ok(imported)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Binary, Manifest, Method, TicketIdentity};
    use serde_json::json;

    fn report() -> Report {
        Report {
            id: "synthetic-report".into(),
            method: Method::TicketsSearch,
            action: None,
            message: "Synthetic page".into(),
            observed_at: "2026-10-25T00:00:00Z".into(),
            output: json!({"items":[{"instance":"https://synthetic.example.test/tenant","id":"1","key":"SYN-1","title":"Synthetic task","description":"No user data or account","url":"https://synthetic.example.test/tenant/tickets/1","status":"Open"}],"next":null}),
        }
    }
    fn profile() -> Installed {
        Installed {manifest:serde_json::from_value::<Manifest>(json!({"id":"synthetic-tickets","name":"Synthetic","description":"No remote access","sourceLocale":"en-US","methods":["check","ticketsSearch","ticketsRead"],"fields":[{"id":"tenant","label":"Tenant","kind":"text","credentialScope":true}],"tabs":[],"translations":{}})).unwrap(),binary:Binary{path:"C:/synthetic/module.exe".into(),sha256:"ab".repeat(32)},revision:1,values:BTreeMap::from([("tenant".into(),json!("first"))]),report:Some(report())}
    }
    fn settings(store: &Store<'_>, profile: &Installed) {
        store.save_settings(&json!({"agent":"codex","connectorRevision":profile.revision,"processConnectors":[profile]})).unwrap();
    }
    #[test]
    fn ticket_reports_refuse_invalid_scopes_duplicates_and_unbounded_content() {
        let valid = report();
        assert_eq!(page(&valid).unwrap().items.len(), 1);
        for url in [
            "https://synthetic.example.test/tenant-other/1",
            "https://other.example.test/tenant/1",
            "https://secret@synthetic.example.test/tenant/1",
            "https://synthetic.example.test/tenant/1?token=secret",
            "http://synthetic.example.test/tenant/1",
            "https://synthetic.example.test/tenant/../outside",
        ] {
            let mut bad = valid.clone();
            bad.output["items"][0]["url"] = json!(url);
            assert!(page(&bad).is_err(), "{url}");
        }
        for instance in [
            "https://synthetic.example.test/tenant/",
            "https://synthetic.example.test/%74enant",
            "https://synthetic.example.test/tenant\n",
        ] {
            let mut bad = valid.clone();
            bad.output["items"][0]["instance"] = json!(instance);
            assert!(page(&bad).is_err());
        }
        let mut bad = valid.clone();
        bad.output["items"]
            .as_array_mut()
            .unwrap()
            .push(valid.output["items"][0].clone());
        assert!(page(&bad).is_err());
        let mut bad = valid.clone();
        bad.output["items"][0]["title"] = json!("x".repeat(301));
        assert!(page(&bad).is_err());
        let mut bad = valid;
        bad.output["items"][0]["unexpected"] = json!(true);
        assert!(page(&bad).is_err());
    }
    #[test]
    fn imports_are_atomic_revision_scoped_and_preserve_every_existing_human_field() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let mut profile = profile();
        settings(&store, &profile);
        let project:Document=serde_json::from_str(&json!({"id":"synthetic-project","name":"Synthetic project","repo":home.path().to_string_lossy()}).to_string()).unwrap();
        store.put(Documents::Projects, &project).unwrap();
        let mut input = TicketImport {
            expected_revision: 1,
            report_id: "synthetic-report".into(),
            project: project.clone(),
            tickets: vec![TicketIdentity {
                instance: "https://synthetic.example.test/tenant".into(),
                id: "1".into(),
            }],
            confirmed: false,
        };
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        input.confirmed = true;
        input.tickets.push(TicketIdentity {
            instance: input.tickets[0].instance.clone(),
            id: "absent".into(),
        });
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        assert!(
            store
                .documents::<Card>(Documents::Cards)
                .unwrap()
                .is_empty()
        );
        input.tickets.pop();
        let first = import(&store, "synthetic-tickets", &input).unwrap();
        assert!(first[0].created);
        assert_eq!(imported_count(&store, &profile).unwrap(), 1);
        let mut human = first[0].card.clone();
        human.title = "Human title".into();
        human
            .fields
            .set("description", &"Human description")
            .unwrap();
        human.fields.set("archived", &true).unwrap();
        store.put(Documents::Cards, &human).unwrap();
        let repeated = import(&store, "synthetic-tickets", &input).unwrap();
        assert!(!repeated[0].created);
        assert_eq!(
            serde_json::to_value(&repeated[0].card).unwrap(),
            serde_json::to_value(&human).unwrap()
        );
        input.report_id = "obsolete".into();
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        input.report_id = "synthetic-report".into();
        input.project.set("name", &"Stale project").unwrap();
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        input.project = project;
        profile.revision = 2;
        profile.values.insert("tenant".into(), json!("second"));
        settings(&store, &profile);
        assert_eq!(imported_count(&store, &profile).unwrap(), 0);
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        input.expected_revision = 2;
        let second = import(&store, "synthetic-tickets", &input).unwrap();
        assert!(second[0].created);
        assert_eq!(imported_count(&store, &profile).unwrap(), 1);
        assert_ne!(first[0].card.id, second[0].card.id);
        profile.revision = 3;
        profile.binary.sha256 = "cd".repeat(32);
        settings(&store, &profile);
        input.expected_revision = 3;
        assert!(!import(&store, "synthetic-tickets", &input).unwrap()[0].created);
        assert_eq!(store.documents::<Card>(Documents::Cards).unwrap().len(), 2);
        assert_eq!(
            serde_json::to_value(
                store
                    .document::<Card>(Documents::Cards, &human.id)
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(human).unwrap()
        );
    }

    #[test]
    fn refresh_is_confirmed_scoped_selective_atomic_and_invalidates_reviews_only_for_changed_requirements()
     {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let mut profile = profile();
        settings(&store, &profile);
        let project: Document = serde_json::from_str(&json!({"id":"synthetic-project","name":"Synthetic","repo":home.path().to_string_lossy()}).to_string()).unwrap();
        store.put(Documents::Projects, &project).unwrap();
        let imported = import(
            &store,
            "synthetic-tickets",
            &TicketImport {
                expected_revision: 1,
                report_id: "synthetic-report".into(),
                project,
                tickets: vec![TicketIdentity {
                    instance: "https://synthetic.example.test/tenant".into(),
                    id: "1".into(),
                }],
                confirmed: true,
            },
        )
        .unwrap();
        let mut human = imported[0].card.clone();
        human.title = "Human title".into();
        human
            .fields
            .set("description", &"Human description")
            .unwrap();
        human.fields.set("status", &"review").unwrap();
        human
            .fields
            .set("instructions", &"Keep human instructions")
            .unwrap();
        human
            .fields
            .set(
                "checklist",
                &json!([{"text":"Human criterion","done":true}]),
            )
            .unwrap();
        store.put(Documents::Cards, &human).unwrap();
        let review: Document = serde_json::from_str(
            &json!({"id":"review","cardId":human.id,"kind":"pr-review","status":"completed"})
                .to_string(),
        )
        .unwrap();
        store.put(Documents::Runs, &review).unwrap();
        let mut report = report();
        report.method = Method::TicketsRead;
        report.output = report.output["items"][0].clone();
        report.output["title"] = json!("Remote title");
        report.output["status"] = json!("Done");
        report.observed_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        profile.report = Some(report.clone());
        settings(&store, &profile);
        let mut input = protocol::TicketRefresh {
            expected_revision: 1,
            report_id: report.id.clone(),
            expected: human.clone(),
            replace_title: false,
            replace_description: false,
            confirmed: false,
        };
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        input.confirmed = true;
        let active: Document = serde_json::from_str(
            &json!({"id":"active","cardId":human.id,"status":"running"}).to_string(),
        )
        .unwrap();
        store.put(Documents::Runs, &active).unwrap();
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        store.delete(Documents::Runs, "active").unwrap();
        let export: Document = serde_json::from_str(
            &json!({"id":"export","cardId":human.id,"exportState":"uncertain"}).to_string(),
        )
        .unwrap();
        store.put(Documents::Times, &export).unwrap();
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        store.delete(Documents::Times, "export").unwrap();
        profile.report.as_mut().unwrap().output["id"] = json!("other");
        settings(&store, &profile);
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        profile.report = Some(report.clone());
        settings(&store, &profile);
        let metadata = refresh(&store, "synthetic-tickets", &input).unwrap();
        assert_eq!(metadata.title, human.title);
        assert_eq!(
            metadata
                .fields
                .get::<String>("description")
                .unwrap()
                .unwrap(),
            "Human description"
        );
        assert!(
            store
                .document::<Document>(Documents::Runs, "review")
                .unwrap()
                .unwrap()
                .raw("reviewStaleReason")
                .is_none()
        );
        input.expected = metadata;
        input.replace_title = true;
        profile.values.insert("tenant".into(), json!("other"));
        settings(&store, &profile);
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        profile.values.insert("tenant".into(), json!("first"));
        profile.report.as_mut().unwrap().id = "new-report".into();
        settings(&store, &profile);
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        profile.report = Some(report.clone());
        profile.report.as_mut().unwrap().observed_at = "2000-01-01T00:00:00Z".into();
        settings(&store, &profile);
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        profile.report = Some(report);
        settings(&store, &profile);
        connection.execute_batch("CREATE TRIGGER refuse_refresh BEFORE UPDATE ON cards BEGIN SELECT RAISE(ABORT,'synthetic rollback'); END;").unwrap();
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        assert!(
            store
                .document::<Document>(Documents::Runs, "review")
                .unwrap()
                .unwrap()
                .raw("reviewStaleReason")
                .is_none()
        );
        connection
            .execute_batch("DROP TRIGGER refuse_refresh;")
            .unwrap();
        let result = refresh(&store, "synthetic-tickets", &input).unwrap();
        assert_eq!(result.title, "Remote title".into());
        for key in ["description", "status", "instructions", "checklist", "repo"] {
            assert_eq!(
                serde_json::to_value(result.fields.get::<serde_json::Value>(key).unwrap()).unwrap(),
                serde_json::to_value(human.fields.get::<serde_json::Value>(key).unwrap()).unwrap()
            );
        }
        assert!(
            store
                .document::<Document>(Documents::Runs, "review")
                .unwrap()
                .unwrap()
                .raw("reviewStaleReason")
                .is_some()
        );
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        input.expected = result.clone();
        input.expected.title = "Stale human edit".into();
        assert!(refresh(&store, "synthetic-tickets", &input).is_err());
        assert_eq!(
            store
                .document::<Card>(Documents::Cards, &result.id)
                .unwrap()
                .unwrap()
                .title,
            result.title
        );
    }
}
