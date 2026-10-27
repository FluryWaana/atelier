//! A reset changes local scope flags only after a complete bounded provider traversal.
use super::{
    connector_tickets, connectors,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{
    application::{
        extensions::{
            self as protocol, Installed, TicketIdentity, TicketResetPreview, TicketResetRow,
        },
        jira::card_snapshot,
    },
    contracts::Card,
    document::Document,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const KEY: &str = "connectorTicketResetPreview";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    card_id: String,
    digest: String,
    title: String,
    key: String,
    identity: TicketIdentity,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cached {
    preview: TicketResetPreview,
    source: Vec<Source>,
    applied: bool,
}

fn digest(card: &Card) -> Result<String> {
    Ok(crate::infrastructure::session::hexadecimal(
        &Sha256::digest(serde_json::to_vec(&card_snapshot(card))?),
    ))
}

pub fn source(store: &Store<'_>, profile: &Installed) -> Result<Vec<Source>> {
    let connector = format!("extension:{}", profile.manifest.id);
    let scope = connector_tickets::scope(profile)?;
    let mut source = Vec::new();
    for card in store.documents::<Card>(Documents::Cards)? {
        let Some(ticket) = card.fields.get::<Document>("ticket")? else {
            continue;
        };
        if !ticket.string_equals("connector", &connector)?
            || !ticket.string_equals("scope", &scope)?
        {
            continue;
        }
        source.push(Source {
            digest: digest(&card)?,
            title: card.title.utf8()?,
            card_id: card.id,
            key: ticket
                .get::<String>("key")?
                .ok_or("Clé du ticket absente.")?,
            identity: TicketIdentity {
                instance: ticket
                    .get::<String>("instance")?
                    .ok_or("Instance absente.")?,
                id: ticket.get::<String>("id")?.ok_or("Identifiant absent.")?,
            },
        });
        if source.len() > 1000 {
            return Err("Plus de 1 000 cartes liées : resynchronisation complète refusée sans modification.".into());
        }
    }
    source.sort_by(|a, b| a.card_id.cmp(&b.card_id));
    Ok(source)
}

pub fn same_source(store: &Store<'_>, profile: &Installed, expected: &[Source]) -> Result<()> {
    if serde_json::to_value(source(store, profile)?)? != serde_json::to_value(expected)? {
        return Err(
            "Les cartes liées ont changé. Relis entièrement le périmètre du connecteur.".into(),
        );
    }
    Ok(())
}

/// This collector cannot produce a preview until a terminal page is observed.
#[derive(Default)]
pub struct Traversal {
    identities: BTreeSet<(String, String)>,
    cursors: BTreeSet<String>,
    pages: usize,
    bytes: usize,
    complete: bool,
    failed: bool,
}
impl Traversal {
    pub fn page(&mut self, report: &protocol::Report) -> Result<Option<String>> {
        if self.failed {
            return Err("Lecture complète invalidée par une erreur précédente.".into());
        }
        let result = self.page_inner(report);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn page_inner(&mut self, report: &protocol::Report) -> Result<Option<String>> {
        if report.method != protocol::Method::TicketsSearch {
            return Err("La lecture du périmètre exige une page de recherche de tickets.".into());
        }
        if self.complete {
            return Err("Lecture complète déjà terminée.".into());
        }
        let page = connector_tickets::page(report)?;
        self.pages += 1;
        self.bytes += serde_json::to_vec(&report.output)?.len();
        if self.pages > 100 || self.bytes > 8 * 1024 * 1024 {
            return Err(
                "Le périmètre dépasse la limite de lecture complète ; aucune carte modifiée."
                    .into(),
            );
        }
        for ticket in page.items {
            if !self.identities.insert((ticket.instance, ticket.id))
                || self.identities.len() > 10_000
            {
                return Err("Périmètre trop grand ou ticket répété entre les pages ; aucune carte modifiée.".into());
            }
        }
        if let Some(cursor) = &page.next {
            if !self.cursors.insert(cursor.clone()) {
                return Err("Boucle de pagination du connecteur ; aucune carte modifiée.".into());
            }
        } else {
            self.complete = true;
        }
        Ok(page.next)
    }
    pub fn save(
        self,
        store: &Store<'_>,
        profile: &Installed,
        source: Vec<Source>,
    ) -> Result<TicketResetPreview> {
        if !self.complete || self.failed {
            return Err(
                "Un résultat partiel ne permet pas de déterminer les tickets hors périmètre."
                    .into(),
            );
        }
        connectors::profile(store, &profile.manifest.id, profile.revision)?;
        same_source(store, profile, &source)?;
        let preview = TicketResetPreview {
            id: uuid::Uuid::new_v4().to_string(),
            module: profile.manifest.id.clone(),
            revision: profile.revision,
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
            pages: self.pages,
            tickets: self.identities.len(),
            rows: source
                .iter()
                .map(|row| TicketResetRow {
                    card_id: row.card_id.clone(),
                    title: row.title.clone(),
                    key: row.key.clone(),
                    present: self
                        .identities
                        .contains(&(row.identity.instance.clone(), row.identity.id.clone())),
                })
                .collect(),
        };
        let mut settings = store.settings::<Document>()?;
        settings.set(
            KEY,
            &Cached {
                preview: preview.clone(),
                source,
                applied: false,
            },
        )?;
        store.save_settings(&settings)?;
        Ok(preview)
    }
}

pub fn apply(
    store: &Store<'_>,
    module: &str,
    input: &protocol::TicketResetApply,
) -> Result<TicketResetPreview> {
    if !input.confirmed {
        return Err("Confirme la mise à jour locale des indicateurs de périmètre.".into());
    }
    store.transaction(|store| {
        let profile=connectors::profile(store,module,input.expected_revision)?;
        if super::connector_effects::read(&store.settings::<Document>()?)?.iter().any(super::connector_effects::pending) {
            return Err("Termine ou rapproche les écritures de connecteurs avant de resynchroniser le périmètre.".into());
        }
        let mut settings=store.settings::<Document>()?;
        let mut cached=settings.get::<Cached>(KEY)?.ok_or("Relis entièrement le périmètre avant de confirmer.")?;
        if cached.preview.id!=input.preview_id || cached.preview.module!=module || cached.preview.revision!=profile.revision {
            return Err("L’aperçu du périmètre a changé. Relance sa lecture complète.".into());
        }
        if cached.applied {return Ok(cached.preview);}
        let observed=time::OffsetDateTime::parse(&cached.preview.observed_at,&time::format_description::well_known::Rfc3339)?;
        if !(0..=600).contains(&(time::OffsetDateTime::now_utc()-observed).whole_seconds()) {return Err("Cet aperçu de périmètre a expiré. Relis le connecteur.".into());}
        same_source(store,&profile,&cached.source)?;
        let flags:BTreeMap<_,_>=cached.preview.rows.iter().map(|row|(row.card_id.as_str(),!row.present)).collect();
        let runs=store.documents::<Document>(Documents::Runs)?;
        let entries=store.documents::<Document>(Documents::Times)?;
        for row in &cached.source {
            let card=store.document::<Card>(Documents::Cards,&row.card_id)?.ok_or("Carte introuvable.")?;
            let mut card=card;
            if atelier_core::application::jira::unresolved(&card.fields)? || atelier_core::application::forge::unresolved(&card.fields)?
                || card.fields.get::<Document>("automation")?.is_some_and(|flow|flow.string_equals("state","running").unwrap_or(true)) {
                return Err("Termine les opérations de la tâche avant de resynchroniser son périmètre.".into());
            }
            for run in &runs {
                if run.string_equals("cardId",&card.id)? && super::runs::active(run)? {return Err("Arrête l’exécution avant de resynchroniser le périmètre des tickets.".into());}
            }
            for entry in &entries {
                if entry.string_equals("cardId",&card.id)? && (entry.string_equals("exportState","sending")? || entry.string_equals("exportState","uncertain")?) {return Err("Rapproche l’export de temps avant de resynchroniser le périmètre.".into());}
            }
            card.fields.set("ticketOutOfScope",&flags[card.id.as_str()])?;
            store.put(Documents::Cards,&card)?;
        }
        cached.applied=true;
        settings.set(KEY,&cached)?;
        store.save_settings(&settings)?;
        Ok(cached.preview)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile() -> Installed {
        serde_json::from_value(json!({"manifest":{"id":"synthetic-reset","name":"Synthetic reset","description":"No network","sourceLocale":"en-US","methods":["check","ticketsSearch","ticketsReset"],"fields":[],"tabs":[],"translations":{}},"binary":{"path":"C:/synthetic/module.exe","sha256":"ab".repeat(32)},"revision":1,"values":{},"report":null})).unwrap()
    }
    fn card(profile: &Installed, id: &str) -> Card {
        serde_json::from_str(&json!({"id":id,"title":"Human title","status":"todo","description":"Human description","instructions":"Human instruction","checklist":[],"ticket":{"connector":"extension:synthetic-reset","scope":connector_tickets::scope(profile).unwrap(),"instance":"https://synthetic.example.test/tenant","id":id,"key":format!("SYN-{id}"),"url":format!("https://synthetic.example.test/tenant/tickets/{id}"),"status":"Open"}}).to_string()).unwrap()
    }
    fn page(id: Option<&str>, next: Option<&str>) -> protocol::Report {
        protocol::Report {
            id: uuid::Uuid::new_v4().to_string(),
            method: protocol::Method::TicketsSearch,
            action: None,
            message: String::new(),
            observed_at: "2026-10-27T00:00:00Z".into(),
            output: json!({"items":id.map(|id|json!({"instance":"https://synthetic.example.test/tenant","id":id,"key":format!("SYN-{id}"),"title":"Remote title","description":"Remote text","url":format!("https://synthetic.example.test/tenant/tickets/{id}"),"status":"Closed"})).into_iter().collect::<Vec<_>>(),"next":next}),
        }
    }
    #[test]
    fn incomplete_duplicate_and_cyclic_scans_never_produce_a_scope_preview() {
        let connection = super::super::store::open(std::path::Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let profile = profile();
        let mut scan = Traversal::default();
        scan.page(&page(Some("one"), Some("second"))).unwrap();
        assert!(scan.save(&store, &profile, Vec::new()).is_err());
        assert!(store.settings::<Document>().unwrap().raw(KEY).is_none());
        let mut duplicate = Traversal::default();
        duplicate.page(&page(Some("one"), Some("second"))).unwrap();
        assert!(duplicate.page(&page(Some("one"), None)).is_err());
        let mut cycle = Traversal::default();
        cycle.page(&page(None, Some("same"))).unwrap();
        assert!(cycle.page(&page(None, Some("same"))).is_err());
        let mut bound = Traversal::default();
        for index in 0..100 {
            bound.page(&page(None, Some(&index.to_string()))).unwrap();
        }
        assert!(bound.page(&page(None, None)).is_err());
    }
    #[test]
    fn full_scan_confirms_only_membership_and_retries_never_overwrite_human_edits() {
        let connection = super::super::store::open(std::path::Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let profile = profile();
        store
            .save_settings(
                &json!({"agent":"codex","connectorRevision":1,"processConnectors":[profile]}),
            )
            .unwrap();
        let present = card(&profile, "one");
        let absent = card(&profile, "two");
        store.put(Documents::Cards, &present).unwrap();
        store.put(Documents::Cards, &absent).unwrap();
        let expected = source(&store, &profile).unwrap();
        let mut scan = Traversal::default();
        scan.page(&page(None, Some("next"))).unwrap();
        scan.page(&page(Some("one"), None)).unwrap();
        let preview = scan.save(&store, &profile, expected).unwrap();
        assert_eq!(preview.pages, 2);
        assert_eq!(preview.tickets, 1);
        let input = protocol::TicketResetApply {
            preview_id: preview.id,
            expected_revision: 1,
            confirmed: true,
        };
        let mut human = present.clone();
        human.title = "Changed after preview".into();
        store.put(Documents::Cards, &human).unwrap();
        assert!(apply(&store, "synthetic-reset", &input).is_err());
        assert!(
            store
                .document::<Card>(Documents::Cards, "two")
                .unwrap()
                .unwrap()
                .fields
                .raw("ticketOutOfScope")
                .is_none()
        );
        store.put(Documents::Cards, &present).unwrap();
        let run: Document =
            serde_json::from_str(r#"{"id":"synthetic-run","cardId":"two","status":"running"}"#)
                .unwrap();
        store.put(Documents::Runs, &run).unwrap();
        assert!(apply(&store, "synthetic-reset", &input).is_err());
        assert!(
            store
                .document::<Card>(Documents::Cards, "one")
                .unwrap()
                .unwrap()
                .fields
                .raw("ticketOutOfScope")
                .is_none()
        );
        store.delete(Documents::Runs, "synthetic-run").unwrap();
        apply(&store, "synthetic-reset", &input).unwrap();
        for (original, out) in [(present.clone(), false), (absent.clone(), true)] {
            let mut saved = store
                .document::<Card>(Documents::Cards, &original.id)
                .unwrap()
                .unwrap();
            assert_eq!(
                saved.fields.get::<bool>("ticketOutOfScope").unwrap(),
                Some(out)
            );
            saved.fields.remove("ticketOutOfScope");
            assert_eq!(
                serde_json::to_value(saved).unwrap(),
                serde_json::to_value(original).unwrap()
            );
        }
        store.put(Documents::Cards, &human).unwrap();
        apply(&store, "synthetic-reset", &input).unwrap();
        assert_eq!(
            store
                .document::<Card>(Documents::Cards, "one")
                .unwrap()
                .unwrap()
                .title,
            atelier_core::document::JsonString::from("Changed after preview")
        );
    }
}
