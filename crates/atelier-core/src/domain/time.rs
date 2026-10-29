use crate::{
    document::{Document, JsonString},
    domain::text_units,
};
use serde::Serialize;
use serde_json::{Value, value::RawValue};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeFields {
    pub seconds: u32,
    pub date: String,
    pub start_time: String,
    pub description: JsonString,
}

// Number(array) uses Array.toString; JSON objects cannot supply callable conversion methods.
fn array_text(raw: &RawValue) -> Result<String, &'static str> {
    match raw.get().as_bytes()[0] {
        b'[' => serde_json::from_str::<Vec<Box<RawValue>>>(raw.get())
            .map_err(|_| "message.duration_must_be_between_1_second_and_24_hours")?
            .iter()
            .map(|item| array_text(item))
            .collect::<Result<Vec<_>, _>>()
            .map(|items| items.join(",")),
        b'{' => {
            let object: Document = serde_json::from_str(raw.get())
                .map_err(|_| "message.duration_must_be_between_1_second_and_24_hours")?;
            if object.raw("toString").is_some() {
                Err("message.invalid_duration_value")
            } else {
                Ok("[object Object]".into())
            }
        }
        b'"' => Ok(serde_json::from_str::<JsonString>(raw.get())
            .ok()
            .and_then(|value| value.utf8().ok())
            .unwrap_or_else(|| "NaN".into())),
        b'n' => Ok(String::new()),
        _ => Ok(raw.get().into()),
    }
}

fn seconds(raw: Option<&RawValue>) -> Result<u32, &'static str> {
    const ERROR: &str = "message.duration_must_be_between_1_second_and_24_hours";
    let raw = raw.ok_or(ERROR)?;
    let value = match raw.get().as_bytes()[0] {
        b't' => 1.0,
        b'f' | b'n' => 0.0,
        b'"' | b'[' | b'{' => {
            let value = array_text(raw)?;
            let value = crate::domain::text(&value, usize::MAX).map_err(|_| ERROR)?;
            let radix = if value.starts_with("0x") || value.starts_with("0X") {
                Some(16)
            } else if value.starts_with("0b") || value.starts_with("0B") {
                Some(2)
            } else if value.starts_with("0o") || value.starts_with("0O") {
                Some(8)
            } else {
                None
            };
            if let Some(radix) = radix {
                if value.len() == 2 {
                    return Err(ERROR);
                }
                value[2..].chars().try_fold(0.0, |n, c| {
                    c.to_digit(radix)
                        .map(|digit| n * f64::from(radix) + f64::from(digit))
                        .ok_or(ERROR)
                })?
            } else {
                value.parse::<f64>().map_err(|_| ERROR)?
            }
        }
        _ => serde_json::from_str::<Value>(raw.get())
            .ok()
            .and_then(|v| v.as_f64())
            .ok_or(ERROR)?,
    };
    if !value.is_finite() || value.fract() != 0.0 || !(1.0..=86400.0).contains(&value) {
        return Err(ERROR);
    }
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
        .get::<JsonString>("description")
        .ok()
        .flatten()
        .ok_or(DESCRIPTION)?;
    let description = text_units(&description, 4000).map_err(|_| DESCRIPTION)?;
    if description.0.is_empty() {
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
    fn time_descriptions_preserve_isolated_utf16_units() {
        let body: Document = serde_json::from_str(
            r#"{"seconds":60,"date":"2026-10-24","startTime":"09:00:00","description":" \ud83d "}"#,
        )
        .unwrap();
        let fields = time_fields(&body).unwrap();
        assert_eq!(fields.description, JsonString(vec![0xd83d]));
        assert!(
            serde_json::to_string(&fields)
                .unwrap()
                .contains(r#""description":"\ud83d""#)
        );
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
