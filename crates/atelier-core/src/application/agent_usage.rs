use crate::document::Document;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Counter {
    pub total: u64,
    pub samples: u64,
}
impl Counter {
    fn add(&mut self, value: Option<u64>) -> Result<(), &'static str> {
        if let Some(value) = value {
            self.total = self
                .total
                .checked_add(value)
                .ok_or("message.usage_counters_too_large")?;
            self.samples += 1;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Summary {
    pub agent_type: String,
    pub agent: String,
    pub model: String,
    pub runs: u64,
    pub input: Counter,
    pub cache_read: Counter,
    pub cache_creation: Counter,
    pub output: Counter,
    pub active_milliseconds: Counter,
}
pub fn summarize(runs: &[Document]) -> Result<Vec<Summary>, &'static str> {
    let mut groups = BTreeMap::<(String, String, String), Summary>::new();
    let mut ids = std::collections::BTreeSet::new();
    for run in runs {
        let text = |key| {
            run.get::<String>(key)
                .map_err(|_| "message.invalid_run_in_the_usage_summary")
        };
        let id = text("id")?.ok_or("message.run_without_an_identifier_in_the_usage_summary")?;
        if !ids.insert(id) {
            return Err("message.duplicate_runs_in_the_usage_summary");
        }
        if run
            .string_equals("purpose", "verification")
            .map_err(|_| "message.invalid_run_in_the_usage_summary")?
        {
            continue;
        }
        let agent_type = text("agentType")?.unwrap_or_default();
        let agent = text("agent")?.unwrap_or_default();
        let model = text("model")?.unwrap_or_default();
        let entry = groups
            .entry((agent_type.clone(), agent.clone(), model.clone()))
            .or_insert_with(|| Summary {
                agent_type,
                agent,
                model,
                ..Summary::default()
            });
        entry.runs += 1;
        let usage = run
            .get::<Option<Document>>("usage")
            .map_err(|_| "message.invalid_usage_counters")?
            .flatten()
            .unwrap_or_default();
        let metric = |key| {
            usage
                .get::<Option<u64>>(key)
                .map(|value| value.flatten())
                .map_err(|_| "message.invalid_usage_counters")
        };
        entry.input.add(metric("inputTokens")?)?;
        entry.cache_read.add(metric("cachedInputTokens")?)?;
        entry
            .cache_creation
            .add(metric("cacheCreationInputTokens")?)?;
        entry.output.add(metric("outputTokens")?)?;
        entry.active_milliseconds.add(
            run.get::<u64>("activeMilliseconds")
                .map_err(|_| "message.invalid_active_duration")?,
        )?;
        if groups.len() > 1000 {
            return Err("message.usage_summary_limited_to_1_000_groups_narrow_the_selection");
        }
    }
    Ok(groups.into_values().collect())
}
