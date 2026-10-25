use crate::{contracts::Card, document::Document};
fn cell(out: &mut String, value: &str) -> Result<(), &'static str> {
    if value.len() > 64 * 1024 {
        return Err("Champ CSV trop volumineux.");
    }
    let formula = value.starts_with(['\t', '\r', '\n'])
        || value
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '\u{feff}' | '\u{200b}'))
            .starts_with(['=', '+', '-', '@']);
    out.push('"');
    if formula {
        out.push('\'');
    }
    out.push_str(&value.replace('"', "\"\""));
    out.push('"');
    if out.len() > 8 * 1024 * 1024 {
        return Err("Export CSV trop volumineux. Réduis la période.");
    }
    Ok(())
}
pub fn csv(
    headers: &[String; 8],
    entries: &[Document],
    cards: &[Card],
) -> Result<String, &'static str> {
    if entries.len() > 10_000 {
        return Err("Export CSV limité à 10 000 saisies. Réduis la période.");
    }
    let by_id = cards
        .iter()
        .map(|card| (card.id.as_str(), card))
        .collect::<std::collections::HashMap<_, _>>();
    let mut out = String::from("\u{feff}");
    for (index, header) in headers.iter().enumerate() {
        if index != 0 {
            out.push(';');
        }
        cell(&mut out, header)?;
    }
    out.push_str("\r\n");
    for entry in entries {
        if entry
            .truthy("runningSince")
            .map_err(|_| "Saisie de temps invalide.")?
        {
            continue;
        }
        let text = |key: &str| {
            entry
                .get::<String>(key)
                .map_err(|_| "Saisie de temps invalide.")?
                .ok_or("Saisie de temps incomplète.")
        };
        let card_id = text("cardId")?;
        let card = by_id.get(card_id.as_str()).copied();
        let title = card
            .map(|card| card.title.utf8().map_err(|_| "Titre de tâche invalide."))
            .transpose()?
            .unwrap_or_default();
        let ticket = card
            .map(|card| card.fields.get::<Document>("ticket"))
            .transpose()
            .map_err(|_| "Ticket invalide.")?
            .flatten()
            .map(|ticket| ticket.get::<String>("key"))
            .transpose()
            .map_err(|_| "Ticket invalide.")?
            .flatten()
            .unwrap_or_default();
        let seconds = entry
            .get::<f64>("seconds")
            .map_err(|_| "Durée invalide.")?
            .filter(|seconds| {
                seconds.is_finite() && seconds.fract() == 0.0 && (0.0..=86400.0).contains(seconds)
            })
            .ok_or("Durée invalide.")? as u64;
        let tempo = entry
            .get::<String>("tempoId")
            .map_err(|_| "Identifiant Tempo invalide.")?
            .unwrap_or_default();
        let values = [
            text("date")?,
            text("startTime")?,
            ticket,
            title,
            seconds.to_string(),
            text("description")?,
            text("exportState")?,
            tempo,
        ];
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                out.push(';');
            }
            cell(&mut out, value)?;
        }
        out.push_str("\r\n");
    }
    if out.len() > 8 * 1024 * 1024 {
        return Err("Export CSV trop volumineux. Réduis la période.");
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn entry() -> Document {
        serde_json::from_str(&json!({"id":"time","cardId":"card","seconds":90,"date":"2026-10-25","startTime":"10:00:00","description":"line1;\n\"line2\" é😀","exportState":"draft"}).to_string()).unwrap()
    }
    #[test]
    fn csv_preserves_unicode_and_quotes_and_neutralizes_spreadsheet_formulas() {
        let card: Card = serde_json::from_str(
            &json!({"id":"card","title":" =SUM(A1:A2)","ticket":{"key":"SYN-1"}}).to_string(),
        )
        .unwrap();
        let output = csv(
            &std::array::from_fn(|index| format!("H{index}")),
            &[entry()],
            &[card],
        )
        .unwrap();
        assert!(output.starts_with('\u{feff}'));
        assert!(output.contains("\"' =SUM(A1:A2)\""));
        assert!(output.contains("\"line1;\n\"\"line2\"\" é😀\""));
        assert!(output.ends_with("\r\n"));
        for dangerous in [
            "=1",
            "+1",
            "-1",
            "@formula",
            "\ttext",
            "\rtext",
            " \t=1",
            "\u{feff}=1",
            "\u{200b}@formula",
        ] {
            let mut out = String::new();
            cell(&mut out, dangerous).unwrap();
            assert!(out.starts_with("\"'"));
        }
    }
    #[test]
    fn live_timers_are_excluded_and_invalid_durations_fail_without_partial_success() {
        let mut live = entry();
        live.set("runningSince", &"2026-10-25T10:00:00Z").unwrap();
        let headers = std::array::from_fn(|index| format!("H{index}"));
        assert!(!csv(&headers, &[live], &[]).unwrap().contains("line1"));
        for value in [json!(-1), json!(86401), json!(1.5), json!("90")] {
            let mut invalid = entry();
            invalid.set("seconds", &value).unwrap();
            assert!(csv(&headers, &[invalid], &[]).is_err());
        }
        assert!(csv(&headers, &vec![entry(); 10001], &[]).is_err());
        let mut wide = entry();
        wide.set("description", &"x".repeat(64 * 1024 + 1)).unwrap();
        assert!(csv(&headers, &[wide.clone()], &[]).is_err());
        wide.set("description", &"x".repeat(64000)).unwrap();
        assert!(csv(&headers, &vec![wide; 140], &[]).is_err());
    }
}
