use crate::{document::Document, domain::text};
use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeFields {
    pub seconds: u32,
    pub date: String,
    pub start_time: String,
    pub description: String,
}

fn seconds(raw: Option<&RawValue>) -> Result<u32, &'static str> {
    const ERROR: &str = "message.duration_must_be_between_1_second_and_24_hours";
    let number: serde_json::Number =
        serde_json::from_str(raw.ok_or(ERROR)?.get()).map_err(|_| ERROR)?;
    let value = number
        .as_f64()
        .filter(|value| {
            value.is_finite() && value.fract() == 0.0 && (1.0..=86400.0).contains(value)
        })
        .ok_or(ERROR)?;
    Ok(value as u32)
}
pub fn time_fields(body: &Document) -> Result<TimeFields, &'static str> {
    let seconds = seconds(body.raw("seconds"))?;
    let date = body
        .get::<String>("date")
        .ok()
        .flatten()
        .ok_or("message.invalid_date")?;
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return Err("message.invalid_date");
    }
    let year = date[..4]
        .parse::<u32>()
        .map_err(|_| "message.invalid_date")?;
    let month = date[5..7]
        .parse::<usize>()
        .map_err(|_| "message.invalid_date")?;
    let day = date[8..]
        .parse::<u32>()
        .map_err(|_| "message.invalid_date")?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err("message.invalid_time_value");
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day > days[month - 1] {
        return Err("message.invalid_date");
    }
    const DESCRIPTION: &str = "message.a_description_of_1_to_4_000_characters_is_required";
    let description = body
        .get::<String>("description")
        .ok()
        .flatten()
        .ok_or(DESCRIPTION)?;
    let description = text(&description, 4000).map_err(|_| DESCRIPTION)?;
    if description.is_empty() {
        return Err(DESCRIPTION);
    }
    let start_time = body
        .get::<String>("startTime")
        .ok()
        .flatten()
        .ok_or("message.invalid_time")?;
    let bytes = start_time.as_bytes();
    if bytes.len() != 8
        || bytes[2] != b':'
        || bytes[5] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, c)| i == 2 || i == 5 || c.is_ascii_digit())
        || &bytes[..2] > b"23"
        || &bytes[3..5] > b"59"
        || &bytes[6..] > b"59"
    {
        return Err("message.invalid_time");
    }
    Ok(TimeFields {
        seconds,
        date,
        start_time,
        description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_input_is_numeric_and_preserves_valid_unicode() {
        let mut body: Document = serde_json::from_str(r#"{"seconds":60.0,"date":"2026-10-24","startTime":"09:00:00","description":" Travail 🔧 "}"#).unwrap();
        assert_eq!(time_fields(&body).unwrap().description, "Travail 🔧");
        for raw in [
            "true", "false", "null", "[]", "[60]", "{}", "\"60\"", "60.5", "0", "86401",
        ] {
            body.set("seconds", &RawValue::from_string(raw.into()).unwrap())
                .unwrap();
            assert!(time_fields(&body).is_err(), "{raw}");
        }
    }
}
/// Human duration input is explicit; no floating-point minutes or implicit coercion.
pub fn parse_duration(value: &str) -> Option<u32> {
    let parts = value.split(':').collect::<Vec<_>>();
    let [hours, minutes, seconds] = parts.as_slice() else {
        return None;
    };
    if parts
        .iter()
        .any(|part| part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let hours = hours.parse::<u32>().ok()?;
    let minutes = minutes.parse::<u32>().ok()?;
    let seconds = seconds.parse::<u32>().ok()?;
    if hours > 24 || minutes > 59 || seconds > 59 {
        return None;
    }
    let total = hours * 3600 + minutes * 60 + seconds;
    (1..=86400).contains(&total).then_some(total)
}
pub fn duration_text(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
#[cfg(test)]
mod duration_tests {
    use super::*;
    #[test]
    fn durations_have_exact_seconds_and_explicit_day_bounds() {
        for (text, seconds) in [
            ("00:00:01", 1),
            ("00:30:00", 1800),
            ("23:59:59", 86399),
            ("24:00:00", 86400),
        ] {
            assert_eq!(parse_duration(text), Some(seconds));
            assert_eq!(duration_text(u64::from(seconds)), text);
        }
        for text in [
            "00:00:00",
            "24:00:01",
            "01:60:00",
            "1:00:00",
            "-1:00:00",
            "00:00:1.5",
            "30",
            "00:00:01:00",
            "００:００:０１",
        ] {
            assert!(parse_duration(text).is_none());
        }
    }
}
