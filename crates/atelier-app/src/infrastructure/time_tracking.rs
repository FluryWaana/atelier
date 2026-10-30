use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::time_tracking::{TimeClock, TimeRepository},
    document::Document,
};

impl TimeRepository for Store<'_> {
    fn card(&self, id: &str) -> Result<Option<Document>> {
        self.document(Documents::Cards, id)
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
