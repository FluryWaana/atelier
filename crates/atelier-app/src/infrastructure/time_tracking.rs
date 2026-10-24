use super::store::{Documents, Store};
use atelier_core::{
    application::time_tracking::{Result, TimeClock, TimeRepository},
    document::{Document, JsonString},
};
use rusqlite::{OptionalExtension, types::Value};
use serde_json::value::RawValue;

impl TimeRepository for Store<'_> {
    fn card(&self, id: Option<&RawValue>) -> Result<Option<Document>> {
        // node:sqlite binds JS numbers as REAL, booleans as INTEGER and objects as named parameters.
        let id = id.ok_or("Provided value cannot be bound to SQLite parameter 1.")?;
        let parameter = match id.get().as_bytes()[0] {
            b'"' => Value::Text(String::from_utf16_lossy(
                &serde_json::from_str::<JsonString>(id.get())?.0,
            )),
            b'n' => Value::Null,
            b't' => Value::Integer(1),
            b'f' => Value::Integer(0),
            b'{' => {
                let object: Document = serde_json::from_str(id.get())?;
                if let Some(key) = object.keys().next() {
                    return Err(format!(
                        "Unknown named parameter '{}'",
                        String::from_utf16_lossy(&key.0)
                    )
                    .into());
                }
                Value::Null
            }
            b'[' => {
                if !serde_json::from_str::<Vec<Box<RawValue>>>(id.get())?.is_empty() {
                    return Err("Unknown named parameter '0'".into());
                }
                Value::Null
            }
            _ => Value::Real(serde_json::from_str(id.get())?),
        };
        let data: Option<String> = self
            .connection
            .query_row("SELECT data FROM cards WHERE id=?", [parameter], |row| {
                row.get(0)
            })
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    fn entry(&self, id: &str) -> Result<Option<Document>> {
        self.document(Documents::Times, id)
    }
    fn entries(&self) -> Result<Vec<Document>> {
        self.documents(Documents::Times)
    }
    fn save(&self, entry: &Document) -> Result<()> {
        self.put(Documents::Times, entry)
    }
    fn remove(&self, id: &str) -> Result<()> {
        self.delete(Documents::Times, id)
    }
}

pub struct Clock;

impl TimeClock for Clock {
    fn now(&self) -> i64 {
        (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
    }
    fn parse(&self, timestamp: &str) -> Option<i64> {
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(|value| (value.unix_timestamp_nanos() / 1_000_000) as i64)
    }
    fn format(&self, milliseconds: i64) -> Result<String> {
        Ok(
            time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(milliseconds) * 1_000_000)?
                .format(time::macros::format_description!(
                    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
                ))?,
        )
    }
}
