//! Synthetic process connector: no files, network, credentials or paid agents.
use atelier_core::application::extensions::{
    self, Manifest, Method, PROTOCOL, Reply, Request, Status,
};
use serde_json::{Value, json};
use std::io::{Read, Write};

fn manifest() -> Manifest {
    serde_json::from_value(json!({
        "id":"demo","name":"Connecteur de démonstration","description":"Exemple Rust synthétique, sans compte ni réseau.","sourceLocale":"fr-FR",
        "methods":["check","ticketsSearch","ticketsRead","action"],
        "fields":[{"id":"title","label":"Titre du ticket de démonstration","kind":"text","options":[],"credentialScope":false}],
        "tabs":[{"id":"diagnostic","label":"Diagnostic démo","description":"Cet onglet est déclaré par le module.","actions":[{"id":"count","label":"Compter les tickets importés","effect":"read"}]}],
        "translations":{"en-US":{"Connecteur de démonstration":"Demo connector","Exemple Rust synthétique, sans compte ni réseau.":"Synthetic Rust example, without an account or network.","Titre du ticket de démonstration":"Demo ticket title","Diagnostic démo":"Demo diagnostics","Cet onglet est déclaré par le module.":"This tab is declared by the module.","Compter les tickets importés":"Count imported tickets"}}
    })).expect("static synthetic manifest")
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((extensions::INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > extensions::INPUT_LIMIT {
        return Err("Synthetic request exceeds the protocol bound".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let manifest = manifest();
    extensions::validate_manifest(&manifest)?;
    extensions::effect(&manifest, &request)?;
    if !request.secrets.is_empty() {
        return Err("The synthetic demo accepts no credentials".into());
    }
    let title = request
        .settings
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Mon premier connecteur");
    if title.encode_utf16().count() > 300 {
        return Err("Synthetic ticket title exceeds 300 characters".into());
    }
    let ticket = json!({"instance":"https://example.org","id":"first-ticket","key":"DEMO-1","title":title,"description":"Ticket de démonstration synthétique fourni par un processus Rust.","url":"https://example.org/tickets/1","status":"Ouvert"});
    let (message, output) = match request.method {
        Method::Describe => (
            "Synthetic connector manifest",
            serde_json::to_value(manifest)?,
        ),
        Method::Check => (
            "Démonstration locale uniquement ; aucun accès distant vérifié.",
            json!({"available":true,"message":"Aucun appel réseau ni écriture distante."}),
        ),
        Method::TicketsSearch => (
            "Un ticket synthétique",
            json!({"items":[ticket],"next":null}),
        ),
        Method::TicketsRead
            if request.input.get("id").and_then(Value::as_str) == Some("first-ticket") =>
        {
            ("Ticket synthétique", ticket)
        }
        Method::Action => (
            "Comptage local synthétique",
            json!({"message":"Aucun accès distant.","items":[{"label":"Tickets importés","value":request.input.get("importedCount").and_then(Value::as_u64).unwrap_or(0).to_string()}]}),
        ),
        _ => return Err("Unsupported synthetic method".into()),
    };
    let reply = Reply {
        protocol: PROTOCOL.into(),
        request_id: request.request_id,
        module: request.module,
        method: request.method,
        status: Status::Ok,
        message: message.into(),
        output,
    };
    let bytes = serde_json::to_vec(&reply)?;
    if bytes.len() > extensions::OUTPUT_LIMIT {
        return Err("Synthetic reply exceeds the protocol bound".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
