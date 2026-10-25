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

fn scope(profile: &Installed) -> Result<String> {
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
        Installed {manifest:serde_json::from_value::<Manifest>(json!({"id":"synthetic-tickets","name":"Synthetic","description":"No remote access","sourceLocale":"en-US","methods":["check","ticketsSearch"],"fields":[{"id":"tenant","label":"Tenant","kind":"text","credentialScope":true}],"tabs":[],"translations":{}})).unwrap(),binary:Binary{path:"C:/synthetic/module.exe".into(),sha256:"ab".repeat(32)},revision:1,values:BTreeMap::from([("tenant".into(),json!("first"))]),report:Some(report())}
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
        assert!(import(&store, "synthetic-tickets", &input).is_err());
        input.expected_revision = 2;
        let second = import(&store, "synthetic-tickets", &input).unwrap();
        assert!(second[0].created);
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
}
