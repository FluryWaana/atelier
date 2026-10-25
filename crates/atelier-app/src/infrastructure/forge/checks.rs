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
        .ok_or_else(|| "Identifiant de contrôle distant invalide.".into())
}
impl Forge {
    pub async fn checks(&self, repository: &str, number: u64) -> Result<ForgeChecks> {
        tokio::time::timeout(
            Duration::from_secs(180),
            self.read_checks(repository, number),
        )
        .await
        .map_err(|_| "Lecture des contrôles trop longue. Aucun résultat partiel affiché.")?
    }
    async fn read_checks(&self, repository: &str, number: u64) -> Result<ForgeChecks> {
        let (pr, _, raw) = self
            .read_pull_request_content(repository, number, false)
            .await?;
        let source = pr
            .source_commit
            .as_deref()
            .ok_or("Commit source indisponible pour lire les contrôles.")?;
        let mut report = match self.configuration.kind {
            ForgeKind::Github => self.github_checks(&pr, source).await?,
            ForgeKind::Gitlab => self.gitlab_checks(&pr, source).await?,
            ForgeKind::Azure => self.azure_checks(&pr, source, &raw).await?,
        };
        let refreshed = self.pull_request(repository, number).await?;
        if !same_pull_request(&pr, &refreshed) {
            return Err(
                "La PR/MR a changé pendant la lecture des contrôles. Relis sa liaison.".into(),
            );
        }
        report.pull_request = refreshed;
        if serde_json::to_vec(&report)?.len() > 4 * 1024 * 1024 {
            return Err(
                "Contrôles de forge trop volumineux. Aucun résultat partiel affiché.".into(),
            );
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
                    "Pagination des contrôles trop volumineuse. Aucun résultat partiel affiché."
                        .into(),
                );
            }
            let page = if let Some(key) = key {
                value[key].as_array()
            } else {
                value.as_array()
            }
            .ok_or("Pagination des contrôles invalide.")?;
            if page.len() > 100 {
                return Err("Pagination des contrôles invalide.".into());
            }
            if page.is_empty() {
                return Ok(rows);
            }
            for row in page {
                if !ids.insert(id(row)?) {
                    return Err("Pagination des contrôles répétée ou modifiée. Aucun résultat partiel affiché.".into());
                }
                rows.push(row.clone());
            }
        }
        Err("Lecture des contrôles limitée à 100 pages. Aucun résultat partiel affiché.".into())
    }
    fn check_text(&self, value: &Value, limit: usize) -> Result<String> {
        if value.is_null() {
            return Ok(String::new());
        }
        let text = value
            .as_str()
            .ok_or("Texte de contrôle distant invalide.")?;
        if text.len() > limit
            || text.contains(&self.token)
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err("Texte de contrôle distant invalide ou confidentiel.".into());
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
                return Err("Un contrôle ne porte pas sur le commit source demandé.".into());
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
                .map_err(|_| "Date d’avis GitHub invalide.")?
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
        Ok(ForgeChecks{pull_request:pr.clone(),checks,reviewers:reviewers.into_values().map(|(_,reviewer)|reviewer).collect(),reviewers_available:available,other_snapshot_count:0,limits:vec!["GitHub limite cet endpoint aux 1 000 suites de checks les plus récentes. Les règles de fusion ne sont pas qualifiées.".into()]})
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
                .ok_or("Commit de pipeline absent.")?;
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
                .ok_or("Liste des approbations GitLab invalide.")?;
            if rows.len() > 10000 {
                return Err("Liste des approbations GitLab trop volumineuse.".into());
            }
            let mut ids = BTreeSet::new();
            for row in rows {
                let id = id(&row["user"])?;
                if !ids.insert(id) {
                    return Err("Approbation GitLab répétée.".into());
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
        Ok(ForgeChecks{pull_request:pr.clone(),checks,reviewers,reviewers_available:available,other_snapshot_count:other,limits:vec!["Le pipeline retenu est le plus récent pour le SHA source. Les approbations GitLab ne prouvent pas le commit examiné.".into()]})
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
                return Err("Itération Azure répétée.".into());
            }
            let commit = self
                .commit(&row["sourceRefCommit"]["commitId"])?
                .ok_or("Commit d’itération Azure absent.")?;
            if latest.as_ref().is_none_or(|(known, _)| id > *known) {
                latest = Some((id, commit));
            }
        }
        let latest = latest
            .filter(|(_, commit)| commit == source)
            .map(|(id, _)| id)
            .ok_or("L’itération Azure ne correspond pas au commit source. Relis la PR.")?;
        *path
            .last_mut()
            .ok_or("Chemin de contrôle Azure invalide.")? = "statuses";
        let statuses = self.get_unpaged(self.endpoint(true, &path)?).await?;
        let mut seen = BTreeSet::new();
        let mut other = 0;
        let mut selected = BTreeMap::new();
        for row in azure_rows(&statuses)? {
            let id = id(row)?;
            if !seen.insert(id) {
                return Err("Statut Azure répété.".into());
            }
            let iteration =
                if row["iterationId"].is_null() || row["iterationId"].as_u64() == Some(0) {
                    None
                } else {
                    Some(
                        row["iterationId"]
                            .as_u64()
                            .filter(|id| *id > 0)
                            .ok_or("Itération de statut Azure invalide.")?,
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
                        },
                    ),
                );
            }
        }
        let mut reviewers = Vec::new();
        let available = raw["reviewers"].is_array();
        if let Some(rows) = raw["reviewers"].as_array() {
            if rows.len() > 10000 {
                return Err("Liste des reviewers Azure trop volumineuse.".into());
            }
            let mut seen = BTreeSet::new();
            for row in rows {
                let id = self.text(&row["id"], 100)?;
                if !seen.insert(id) {
                    return Err("Reviewer Azure répété.".into());
                }
                let decision = match row["vote"].as_i64().ok_or("Vote Azure invalide.")? {
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
        Ok(ForgeChecks{pull_request:pr.clone(),checks:selected.into_values().map(|(_,check)|check).collect(),reviewers,reviewers_available:available,other_snapshot_count:other,limits:vec!["Les statuts Azure sans itération et les votes restent non attribués à un commit. Les politiques de branche ne sont pas qualifiées.".into()]})
    }
}
fn azure_rows(value: &Value) -> Result<&[Value]> {
    let rows = value["value"].as_array().ok_or("Liste Azure invalide.")?;
    if rows.len() > 10000 || value["count"].as_u64() != Some(rows.len() as u64) {
        return Err(
            "Liste Azure incomplète ou trop volumineuse. Aucun résultat partiel affiché.".into(),
        );
    }
    Ok(rows)
}
