use atelier_app::infrastructure::{database::Database, store::Documents, time_tracking::Clock};
use atelier_core::{
    application::time_tracking::{self, TimeClock},
    document::{Document, JsonString},
};
use serde::Serialize;
use serde_json::{Value, value::RawValue};
use std::cell::Cell;

struct FixedClock(i64);
impl TimeClock for FixedClock {
    fn now(&self) -> i64 {
        self.0
    }
    fn parse(&self, timestamp: &str) -> Option<i64> {
        Clock.parse(timestamp)
    }
    fn format(&self, milliseconds: i64) -> time_tracking::Result<String> {
        Clock.format(milliseconds)
    }
}

fn equivalent(actual: &RawValue, expected: &RawValue) -> bool {
    match (actual.get().as_bytes()[0], expected.get().as_bytes()[0]) {
        (b'{', b'{') => {
            let actual: Document = serde_json::from_str(actual.get()).unwrap();
            let expected: Document = serde_json::from_str(expected.get()).unwrap();
            actual.keys().count() == expected.keys().count()
                && actual.keys().all(|key| {
                    let key = key.utf8().unwrap();
                    expected
                        .raw(&key)
                        .is_some_and(|expected| equivalent(actual.raw(&key).unwrap(), expected))
                })
        }
        (b'[', b'[') => {
            let actual: Vec<Box<RawValue>> = serde_json::from_str(actual.get()).unwrap();
            let expected: Vec<Box<RawValue>> = serde_json::from_str(expected.get()).unwrap();
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, e)| equivalent(a, e))
        }
        (b'"', b'"') => {
            serde_json::from_str::<JsonString>(actual.get()).unwrap()
                == serde_json::from_str::<JsonString>(expected.get()).unwrap()
        }
        _ => serde_json::from_str::<Value>(actual.get())
            .unwrap()
            .as_f64()
            .zip(
                serde_json::from_str::<Value>(expected.get())
                    .unwrap()
                    .as_f64(),
            )
            .map_or_else(|| actual.get() == expected.get(), |(a, e)| a == e),
    }
}

fn compare(actual: &impl Serialize, expected: &RawValue, context: &str) {
    let actual = serde_json::value::to_raw_value(actual).unwrap();
    assert!(
        equivalent(&actual, expected),
        "{context}\nactual: {actual}\nexpected: {expected}"
    );
}

#[tokio::test]
async fn a_failed_timer_tick_rolls_back_every_entry_in_the_same_job() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic.sqlite");
    let database = Database::open(&path).unwrap();
    let initial = database.call(|store| {
        for id in ["blocked", "first"] {
            store.put(Documents::Times, &serde_json::json!({"id":id, "seconds":10, "runningSince":"2026-10-22T09:00:00.000Z", "exportState":"draft"}))?;
        }
        store.documents::<Document>(Documents::Times)
    }).await.unwrap();
    {
        let injection = rusqlite::Connection::open(&path).unwrap();
        injection.execute_batch("CREATE TRIGGER refuse_tick BEFORE UPDATE ON time_entries WHEN NEW.id='blocked' BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;").unwrap();
    }
    assert!(
        database
            .call(|store| store
                .transaction(|store| time_tracking::tick(store, &FixedClock(1_792_659_665_000))))
            .await
            .is_err()
    );
    let unchanged = database
        .call(|store| store.documents::<Document>(Documents::Times))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&unchanged).unwrap(),
        serde_json::to_string(&initial).unwrap()
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn historical_time_transitions_match_node_on_sqlite() {
    let cases: Vec<Document> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-time-reference.json"
    ))
    .unwrap();
    let mut count = 0;
    for case in cases {
        let name = case.get::<String>("name").unwrap().unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let initial = case
            .get::<Vec<Document>>("initial")
            .unwrap()
            .unwrap_or_default();
        let initialized = database
            .call(move |store| {
                for card in [
                    serde_json::json!({"id":"card"}),
                    serde_json::json!({"id":"archived", "archived":true}),
                    serde_json::json!({"id":"1"}),
                    serde_json::json!({"id":"1.5"}),
                ] {
                    store.put(Documents::Cards, &card)?;
                }
                for entry in initial {
                    store.put(Documents::Times, &entry)?;
                }
                store.transaction(time_tracking::initialize)?;
                store.documents::<Document>(Documents::Times)
            })
            .await
            .unwrap();
        compare(&initialized, case.raw("initialized").unwrap(), &name);
        let mut now = 1_792_659_600_000i64;
        let mut sequence = 0;
        let mut busy = false;
        for (index, step) in case
            .get::<Vec<Document>>("steps")
            .unwrap()
            .unwrap()
            .into_iter()
            .enumerate()
        {
            count += 1;
            now = step.get::<i64>("now").unwrap().unwrap_or(now);
            busy = step.get::<bool>("busy").unwrap().unwrap_or(busy);
            let action = step.get::<String>("action").unwrap().unwrap();
            let id = step.get::<String>("id").unwrap().unwrap_or_default();
            let body = step.get::<Document>("body").unwrap().unwrap_or_default();
            let (result, times, next) = database
                .call(move |store| {
                    let counter = Cell::new(sequence);
                    let result = store.transaction(|store| match action.as_str() {
                        "put" => {
                            store.put(Documents::Times, &body)?;
                            Ok(None)
                        }
                        "tick" => {
                            time_tracking::tick(store, &FixedClock(now))?;
                            Ok(None)
                        }
                        "close" => {
                            time_tracking::close(store, &FixedClock(now))?;
                            Ok(None)
                        }
                        _ => time_tracking::execute(
                            store,
                            &FixedClock(now),
                            &action,
                            &id,
                            &body,
                            || {
                                counter.set(counter.get() + 1);
                                format!("entry-{}", counter.get())
                            },
                            |_| busy,
                        )
                        .map(Some),
                    });
                    let mut response = Document::default();
                    match result {
                        Ok(value) => response.set("value", &value)?,
                        Err(error) => response.set("error", &error.to_string())?,
                    }
                    Ok((
                        response,
                        store.documents::<Document>(Documents::Times)?,
                        counter.get(),
                    ))
                })
                .await
                .unwrap();
            sequence = next;
            let context = format!("{name}: {index}");
            compare(&result, step.raw("expected").unwrap(), &context);
            compare(&times, step.raw("times").unwrap(), &context);
        }
        database.close().await.unwrap();
    }
    assert_eq!(count, 46);
}
