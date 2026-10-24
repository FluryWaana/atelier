use atelier_core::domain::{agent_options::*, checklist::*, projects::*, tasks::*, text};
use serde_json::{Value, json};

#[test]
fn historical_node_domain_results_and_errors_match() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-domain-reference.json"
    ))
    .unwrap();
    assert!(cases.len() > 150);
    for (index, case) in cases.iter().enumerate() {
        let args = &case["args"];
        let result = match case["operation"].as_str().unwrap() {
            "text" => args[0]
                .as_str()
                .ok_or("Texte invalide ou trop long.")
                .and_then(|value| text(value, args[1].as_u64().unwrap() as usize))
                .map(|value| json!(value)),
            "checklist" => checklist(&serde_json::value::to_raw_value(&args[0]).unwrap())
                .map(|value| json!(value)),
            "checklistText" => Ok(json!(checklist_text(
                &serde_json::from_str::<Vec<Criterion>>(&args[0].to_string()).unwrap()
            ))),
            "reviewRequirements" => Ok(review_requirements(&args[0])),
            "blockers" => Ok(json!(blockers(&args[0], args[1].as_array().unwrap()))),
            "dependencies" => dependencies(
                args[0].as_str().unwrap(),
                &args[1],
                args[2].as_array().unwrap(),
            )
            .map(|value| json!(value)),
            "agentProviders" => agent_providers(&args[0]).map(|value| json!(value)),
            "agentProvider" => agent_provider(&args[0], args[1].as_str().unwrap()),
            "agentOptions" => agent_options(args[0].as_str().unwrap(), &args[1], args.get(2)),
            "resolveAgentOptions" => resolve_agent_options(&args[0], &args[1], &args[2]),
            "providerSettings" => {
                provider_settings(&serde_json::value::to_raw_value(&args[0]).unwrap())
                    .map(|value| serde_json::to_value(value).unwrap())
            }
            "projectSettings" => project_settings(
                &serde_json::from_str(&args[0].to_string()).unwrap(),
                &serde_json::from_str(&args.get(1).unwrap_or(&json!({})).to_string()).unwrap(),
            )
            .map(|value| serde_json::to_value(value).unwrap()),
            "timeFields" => atelier_core::domain::time::time_fields(
                &serde_json::from_str(&args[0].to_string()).unwrap(),
            )
            .map(|value| serde_json::to_value(value).unwrap()),
            "settingsForProject" => Ok(settings_for_project(
                &args[0],
                args[1].as_array().unwrap(),
                args[2].as_str().unwrap(),
                cfg!(windows),
            )),
            "sameRepository" => Ok(json!(same_repository(
                args[0].as_str().unwrap(),
                args[1].as_str().unwrap(),
                args[2] == "win32"
            ))),
            operation => panic!("Unknown contract: {operation}"),
        };
        let actual = match result {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error}),
        };
        assert_eq!(
            actual, case["expected"],
            "Contract {index}: {}",
            case["operation"]
        );
    }
}

#[test]
fn dependency_walk_handles_long_chains_and_cycles_without_recursion() {
    let mut cards: Vec<Value> = (0..10_000)
        .map(|i| json!({"id": i.to_string(), "blockedBy": [(i + 1).to_string()]}))
        .collect();
    assert_eq!(dependencies("new", &json!(["0"]), &cards).unwrap(), ["0"]);
    cards.last_mut().unwrap()["blockedBy"] = json!(["new"]);
    assert_eq!(
        dependencies("new", &json!(["0"]), &cards),
        Err("Cette dépendance créerait un cycle.")
    );
}
