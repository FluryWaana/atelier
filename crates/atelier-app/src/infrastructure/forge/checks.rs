use super::{Duration, Forge, ForgeKind, Result, Url, Value, same_pull_request};
use atelier_core::contracts::{
    ForgeCheck, ForgeCheckScope as Scope, ForgeCheckState as State, ForgeChecks,
    ForgeReviewDecision as Decision, ForgeReviewer, PullRequest,
};
use std::collections::{BTreeMap, BTreeSet};
fn id(row: &Value) -> Result<u64> {
    row["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "message.invalid_remote_check_identifier".into())
}
impl Forge {
    pub async fn checks(&self, repository: &str, number: u64) -> Result<ForgeChecks> {
        tokio::time::timeout(
            Duration::from_secs(180),
            self.read_checks(repository, number),
        )
        .await
        .map_err(|_| "message.checks_read_timed_out_no_partial_results_displayed")?
    }
    async fn read_checks(&self, repository: &str, number: u64) -> Result<ForgeChecks> {
        let (pr, _, raw) = self
            .read_pull_request_content(repository, number, false)
            .await?;
        let source = pr
            .source_commit
            .as_deref()
            .ok_or("message.source_commit_unavailable_for_reading_checks")?;
        let mut report = match self.configuration.kind {
            ForgeKind::Github => self.github_checks(&pr, source).await?,
            ForgeKind::Gitlab => self.gitlab_checks(&pr, source).await?,
            ForgeKind::Azure => self.azure_checks(&pr, source, &raw).await?,
        };
        let refreshed = self.pull_request(repository, number).await?;
        if !same_pull_request(&pr, &refreshed) {
            return Err(
                "message.the_pr_mr_changed_while_reading_checks_read_its_link_again".into(),
            );
        }
        report.pull_request = refreshed;
        if serde_json::to_vec(&report)?.len() > 4 * 1024 * 1024 {
            return Err("message.forge_checks_too_large_no_partial_results_displayed".into());
        }
        Ok(report)
    }
    async fn check_pages(&self, endpoint: Url, key: Option<&str>) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        let mut ids = BTreeSet::new();
        let mut bytes = 0usize;
        for page in 1..=100 {
            let mut url = endpoint.clone();
            url.query_pairs_mut()
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
            let value = self.get(url).await?;
            bytes = bytes.saturating_add(serde_json::to_vec(&value)?.len());
            if bytes > 8 * 1024 * 1024 {
                return Err(
                    "message.check_pagination_too_large_no_partial_results_displayed".into(),
                );
            }
            let page = if let Some(key) = key {
                value[key].as_array()
            } else {
                value.as_array()
            }
            .ok_or("message.invalid_check_pagination")?;
            if page.len() > 100 {
                return Err("message.invalid_check_pagination".into());
            }
            if page.is_empty() {
                return Ok(rows);
            }
            for row in page {
                if !ids.insert(id(row)?) {
                    return Err(
                        "message.check_pagination_repeated_or_changed_no_partial_results_displaye"
                            .into(),
                    );
                }
                rows.push(row.clone());
            }
        }
        Err("message.check_read_limited_to_100_pages_no_partial_results_displayed".into())
    }
    fn check_text(&self, value: &Value, limit: usize) -> Result<String> {
        if value.is_null() {
            return Ok(String::new());
        }
        let text = value.as_str().ok_or("message.invalid_remote_check_text")?;
        if text.len() > limit
            || text.contains(&self.token)
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err("message.invalid_or_confidential_remote_check_text".into());
        }
        Ok(text.into())
    }
    async fn github_checks(&self, pr: &PullRequest, source: &str) -> Result<ForgeChecks> {
        let parts =
            atelier_core::application::forge::repository(pr.kind, &pr.repository, pr.number)?;
        let statuses = self
            .check_pages(
                self.endpoint(
                    true,
                    &["repos", parts[0], parts[1], "commits", source, "statuses"],
                )?,
                None,
            )
            .await?;
        let mut latest = BTreeMap::<String, (u64, ForgeCheck)>::new();
        for row in statuses {
            let id = id(&row)?;
            let name = self.text(&row["context"], 1000)?.to_owned();
            let state = match self.text(&row["state"], 100)? {
                "success" => State::Succeeded,
                "failure" | "error" => State::Failed,
                "pending" => State::Pending,
                _ => State::Unknown,
            };
            if latest.get(&name).is_none_or(|(known, _)| id > *known) {
                latest.insert(
                    name.clone(),
                    (
                        id,
                        ForgeCheck {
                            id: format!("status:{id}"),
                            name,
                            state,
                            scope: Scope::SourceCommit,
                            commit: Some(source.into()),
                            description: self.check_text(&row["description"], 8000)?,
                            description_key: None,
                        },
                    ),
                );
            }
        }
        let mut checks = latest
            .into_values()
            .map(|(_, check)| check)
            .collect::<Vec<_>>();
        let mut url = self.endpoint(
            true,
            &["repos", parts[0], parts[1], "commits", source, "check-runs"],
        )?;
        url.query_pairs_mut().append_pair("filter", "latest");
        for row in self.check_pages(url, Some("check_runs")).await? {
            let id = id(&row)?;
            if self.commit(&row["head_sha"])?.as_deref() != Some(source) {
                return Err("message.a_check_does_not_refer_to_the_requested_source_commit".into());
            }
            let status = self.text(&row["status"], 100)?;
            let conclusion = if row["conclusion"].is_null() {
                None
            } else {
                Some(self.text(&row["conclusion"], 100)?)
            };
            let state = if status == "completed" {
                match conclusion {
                    Some("success") => State::Succeeded,
                    Some("failure" | "timed_out" | "action_required" | "startup_failure") => {
                        State::Failed
                    }
                    Some("cancelled") => State::Cancelled,
                    Some("skipped" | "neutral") => State::Skipped,
                    _ => State::Unknown,
                }
            } else {
                match status {
                    "in_progress" => State::Running,
                    "queued" | "pending" | "waiting" | "requested" => State::Pending,
                    _ => State::Unknown,
                }
            };
            checks.push(ForgeCheck {
                id: format!("check:{id}"),
                name: self.text(&row["name"], 1000)?.into(),
                state,
                scope: Scope::SourceCommit,
                commit: Some(source.into()),
                description: self.check_text(&row["output"]["title"], 8000)?,
                description_key: None,
            });
        }
        let reviews = self
            .check_pages(
                self.endpoint(
                    true,
                    &[
                        "repos",
                        parts[0],
                        parts[1],
                        "pulls",
                        &pr.number.to_string(),
                        "reviews",
                    ],
                )?,
                None,
            )
            .await;
        let mut reviewers = BTreeMap::new();
        let available = reviews.is_ok();
        if let Ok(rows) = reviews {
            for row in rows {
                let decision = match self.text(&row["state"], 100)? {
                    "APPROVED" => Decision::Approved,
                    "CHANGES_REQUESTED" => Decision::ChangesRequested,
                    "DISMISSED" => Decision::Dismissed,
                    _ => continue,
                };
                let id = id(&row)?;
                let submitted = time::OffsetDateTime::parse(
                    self.text(&row["submitted_at"], 100)?,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|_| "message.invalid_github_review_date")?
                .unix_timestamp_nanos();
                let order = (submitted, id);
                let name = self.text(&row["user"]["login"], 1000)?.to_owned();
                let commit = self.commit(&row["commit_id"])?;
                let scope = match commit.as_deref() {
                    Some(commit) if commit == source => Scope::SourceCommit,
                    Some(_) => Scope::OtherSnapshot,
                    None => Scope::Unscoped,
                };
                if reviewers.get(&name).is_none_or(|(known, _)| order > *known) {
                    reviewers.insert(
                        name.clone(),
                        (
                            order,
                            ForgeReviewer {
                                name,
                                decision,
                                scope,
                                commit,
                            },
                        ),
                    );
                }
            }
        }
        Ok(ForgeChecks {
            pull_request: pr.clone(),
            checks,
            reviewers: reviewers
                .into_values()
                .map(|(_, reviewer)| reviewer)
                .collect(),
            reviewers_available: available,
            other_snapshot_count: 0,
            limits: vec![
                "message.github_limits_this_endpoint_to_the_1_000_most_recent_check_suite".into(),
            ],
        })
    }
    async fn gitlab_checks(&self, pr: &PullRequest, source: &str) -> Result<ForgeChecks> {
        let rows = self
            .check_pages(
                self.endpoint(
                    true,
                    &[
                        "projects",
                        &pr.repository,
                        "merge_requests",
                        &pr.number.to_string(),
                        "pipelines",
                    ],
                )?,
                None,
            )
            .await?;
        let mut latest = None::<(u64, Value)>;
        let mut other = 0;
        for row in rows {
            let id = id(&row)?;
            let commit = self
                .commit(&row["sha"])?
                .ok_or("message.pipeline_commit_missing")?;
            if commit != source {
                other += 1;
                continue;
            }
            if latest.as_ref().is_none_or(|(known, _)| id > *known) {
                latest = Some((id, row));
            }
        }
        let mut checks = Vec::new();
        if let Some((id, row)) = latest {
            let state = match self.text(&row["status"], 100)? {
                "success" => State::Succeeded,
                "failed" => State::Failed,
                "canceled" => State::Cancelled,
                "skipped" => State::Skipped,
                "running" => State::Running,
                "manual"
                | "scheduled"
                | "created"
                | "waiting_for_resource"
                | "preparing"
                | "pending" => State::Pending,
                _ => State::Unknown,
            };
            checks.push(ForgeCheck {
                id: format!("pipeline:{id}"),
                name: format!("Pipeline #{id}"),
                state,
                scope: Scope::SourceCommit,
                commit: Some(source.into()),
                description: self.check_text(&row["ref"], 1000)?,
                description_key: None,
            });
        }
        let approvals = self
            .get(self.endpoint(
                true,
                &[
                    "projects",
                    &pr.repository,
                    "merge_requests",
                    &pr.number.to_string(),
                    "approvals",
                ],
            )?)
            .await;
        let mut reviewers = Vec::new();
        let available = approvals.is_ok();
        if let Ok(value) = approvals {
            let rows = value["approved_by"]
                .as_array()
                .ok_or("message.invalid_gitlab_approval_list")?;
            if rows.len() > 10000 {
                return Err("message.gitlab_approval_list_too_large".into());
            }
            let mut ids = BTreeSet::new();
            for row in rows {
                let id = id(&row["user"])?;
                if !ids.insert(id) {
                    return Err("message.repeated_gitlab_approval".into());
                }
                let name = self.text(&row["user"]["username"], 1000)?.to_owned();
                reviewers.push(ForgeReviewer {
                    name,
                    decision: Decision::Approved,
                    scope: Scope::Unscoped,
                    commit: None,
                });
            }
        }
        Ok(ForgeChecks {
            pull_request: pr.clone(),
            checks,
            reviewers,
            reviewers_available: available,
            other_snapshot_count: other,
            limits: vec![
                "message.the_selected_pipeline_is_the_latest_for_the_source_sha_gitlab_ap".into(),
            ],
        })
    }
    async fn azure_checks(
        &self,
        pr: &PullRequest,
        source: &str,
        raw: &Value,
    ) -> Result<ForgeChecks> {
        let parts =
            atelier_core::application::forge::repository(pr.kind, &pr.repository, pr.number)?;
        let base = [
            parts[0],
            "_apis",
            "git",
            "repositories",
            parts[1],
            "pullrequests",
        ];
        let number = pr.number.to_string();
        let mut path = base.to_vec();
        path.extend([number.as_str(), "iterations"]);
        let iterations = self.get_unpaged(self.endpoint(true, &path)?).await?;
        let rows = azure_rows(&iterations)?;
        let mut latest = None::<(u64, String)>;
        let mut seen = BTreeSet::new();
        for row in rows {
            let id = id(row)?;
            if !seen.insert(id) {
                return Err("message.repeated_azure_iteration".into());
            }
            let commit = self
                .commit(&row["sourceRefCommit"]["commitId"])?
                .ok_or("message.azure_iteration_commit_missing")?;
            if latest.as_ref().is_none_or(|(known, _)| id > *known) {
                latest = Some((id, commit));
            }
        }
        let latest = latest
            .filter(|(_, commit)| commit == source)
            .map(|(id, _)| id)
            .ok_or("message.the_azure_iteration_does_not_match_the_source_commit_read_the_pr")?;
        *path.last_mut().ok_or("message.invalid_azure_check_path")? = "statuses";
        let statuses = self.get_unpaged(self.endpoint(true, &path)?).await?;
        let mut seen = BTreeSet::new();
        let mut other = 0;
        let mut selected = BTreeMap::new();
        for row in azure_rows(&statuses)? {
            let id = id(row)?;
            if !seen.insert(id) {
                return Err("message.repeated_azure_status".into());
            }
            let iteration =
                if row["iterationId"].is_null() || row["iterationId"].as_u64() == Some(0) {
                    None
                } else {
                    Some(
                        row["iterationId"]
                            .as_u64()
                            .filter(|id| *id > 0)
                            .ok_or("message.invalid_azure_status_iteration")?,
                    )
                };
            if iteration.is_some_and(|id| id != latest) {
                other += 1;
                continue;
            }
            let genre = self.check_text(&row["context"]["genre"], 1000)?;
            let name = self.text(&row["context"]["name"], 1000)?.to_owned();
            let key = (genre, name.clone(), iteration);
            let state = match self.text(&row["state"], 100)? {
                "succeeded" => State::Succeeded,
                "failed" | "error" => State::Failed,
                "pending" => State::Pending,
                "notApplicable" => State::Skipped,
                _ => State::Unknown,
            };
            let (scope, commit) = if iteration == Some(latest) {
                (Scope::SourceCommit, Some(source.into()))
            } else {
                (Scope::Unscoped, None)
            };
            if selected.get(&key).is_none_or(|(known, _)| id > *known) {
                selected.insert(
                    key,
                    (
                        id,
                        ForgeCheck {
                            id: format!("status:{id}"),
                            name,
                            state,
                            scope,
                            commit,
                            description: self.check_text(&row["description"], 8000)?,
                            description_key: None,
                        },
                    ),
                );
            }
        }
        let mut reviewers = Vec::new();
        let available = raw["reviewers"].is_array();
        if let Some(rows) = raw["reviewers"].as_array() {
            if rows.len() > 10000 {
                return Err("message.azure_reviewer_list_too_large".into());
            }
            let mut seen = BTreeSet::new();
            for row in rows {
                let id = self.text(&row["id"], 100)?;
                if !seen.insert(id) {
                    return Err("message.repeated_azure_reviewer".into());
                }
                let decision = match row["vote"].as_i64().ok_or("message.invalid_azure_vote")? {
                    10 => Decision::Approved,
                    5 => Decision::ApprovedWithSuggestions,
                    -10 => Decision::ChangesRequested,
                    -5 => Decision::Waiting,
                    0 => Decision::None,
                    _ => Decision::Unknown,
                };
                reviewers.push(ForgeReviewer {
                    name: self.text(&row["displayName"], 1000)?.into(),
                    decision,
                    scope: Scope::Unscoped,
                    commit: None,
                });
            }
        }
        let mut checks = selected
            .into_values()
            .map(|(_, check)| check)
            .collect::<Vec<_>>();
        checks.extend(self.azure_policies(pr, raw).await?);
        Ok(ForgeChecks {
            pull_request: pr.clone(),
            checks,
            reviewers,
            reviewers_available: available,
            other_snapshot_count: other,
            limits: vec![
                "message.azure_statuses_without_an_iteration_votes_and_policies_are_not_a".into(),
            ],
        })
    }
    async fn azure_policies(&self, pr: &PullRequest, raw: &Value) -> Result<Vec<ForgeCheck>> {
        let project = self.text(&raw["repository"]["project"]["id"], 100)?;
        let project = uuid::Uuid::parse_str(project)
            .map_err(|_| "message.invalid_azure_policy_project")?
            .to_string();
        let artifact = format!("vstfs:///CodeReview/CodeReviewId/{project}/{}", pr.number);
        let mut checks = Vec::new();
        let mut seen = BTreeSet::new();
        let mut bytes = 0usize;
        for page in 0..100 {
            let mut url = self.endpoint(true, &[&project, "_apis", "policy", "evaluations"])?;
            url.query_pairs_mut()
                .append_pair("api-version", "7.1-preview.1")
                .append_pair("artifactId", &artifact)
                .append_pair("includeNotApplicable", "true")
                .append_pair("$top", "100")
                .append_pair("$skip", &(page * 100).to_string());
            let value = self.get_unpaged(url).await?;
            bytes = bytes.saturating_add(serde_json::to_vec(&value)?.len());
            if bytes > 4 * 1024 * 1024 {
                return Err("message.azure_policies_are_too_large".into());
            }
            let rows = azure_rows(&value)?;
            if rows.len() > 100 {
                return Err("message.invalid_azure_policy_pagination".into());
            }
            if rows.is_empty() {
                return Ok(checks);
            }
            for row in rows {
                let evaluation = self.text(&row["evaluationId"], 100)?;
                let evaluation = uuid::Uuid::parse_str(evaluation)
                    .map_err(|_| "message.invalid_azure_evaluation")?
                    .to_string();
                if !seen.insert(evaluation.clone())
                    || self.text(&row["artifactId"], 500)? != artifact
                {
                    return Err(
                        "message.azure_policies_do_not_match_this_pr_or_are_repeated".into(),
                    );
                }
                let enabled = row["configuration"]["isEnabled"]
                    .as_bool()
                    .ok_or("message.azure_policy_state_missing")?;
                let blocking = row["configuration"]["isBlocking"]
                    .as_bool()
                    .ok_or("message.azure_policy_scope_missing")?;
                let state = match self.text(&row["status"], 100)? {
                    "queued" | "running" => State::Pending,
                    "approved" => State::Succeeded,
                    "rejected" | "broken" => State::Failed,
                    "notApplicable" => State::Skipped,
                    _ => State::Unknown,
                };
                checks.push(ForgeCheck {
                    id: format!("policy:{evaluation}"),
                    name: self
                        .text(&row["configuration"]["type"]["displayName"], 1000)?
                        .into(),
                    state: if enabled { state } else { State::Skipped },
                    scope: Scope::Unscoped,
                    commit: None,
                    description: String::new(),
                    description_key: Some(
                        match (enabled, blocking) {
                            (false, _) => "message.disabled_policy_no_requirement_inferred",
                            (true, true) => {
                                "message.active_blocking_policy_pr_result_with_no_commit_provided"
                            }
                            (true, false) => {
                                "message.active_advisory_policy_pr_result_with_no_commit_provided"
                            }
                        }
                        .into(),
                    ),
                });
            }
        }
        Err("message.azure_policy_reading_is_limited_to_100_pages_no_partial_result_i".into())
    }
}
fn azure_rows(value: &Value) -> Result<&[Value]> {
    let rows = value["value"]
        .as_array()
        .ok_or("message.invalid_azure_list")?;
    if rows.len() > 10000 || value["count"].as_u64() != Some(rows.len() as u64) {
        return Err(
            "message.azure_list_incomplete_or_too_large_no_partial_results_displayed".into(),
        );
    }
    Ok(rows)
}
