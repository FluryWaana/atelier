use super::task_creation::{CreationContext, Result, text};
use crate::{
    document::{Document, JsonString},
    domain::projects::same_repository_units,
};

#[derive(Debug)]
pub enum ProjectFailure {
    Missing,
    Invalid(&'static str),
    Conflict(&'static str),
}
impl std::fmt::Display for ProjectFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "Projet introuvable.",
            Self::Invalid(message) | Self::Conflict(message) => message,
        })
    }
}
impl std::error::Error for ProjectFailure {}

pub trait ProjectRepository: CreationContext {
    fn projects(&self) -> Result<Vec<Document>>;
    fn cards(&self) -> Result<Vec<Document>>;
    fn times(&self) -> Result<Vec<Document>>;
    fn save_project(&self, project: &Document) -> Result<()>;
    fn save_card(&self, card: &Document) -> Result<()>;
    fn delete_project(&self, id: &str) -> Result<()>;
}

fn same_repo(document: &Document, repo: &JsonString, windows: bool) -> Result<bool> {
    let value = if document.truthy("repo")? {
        document.get::<JsonString>("repo")?.unwrap_or_default()
    } else {
        JsonString::default()
    };
    Ok(same_repository_units(&value, repo, windows))
}

pub fn create(
    repository: &impl ProjectRepository,
    body: &Document,
    windows: bool,
) -> Result<Document> {
    let name = text(body.raw("name"), 100)?;
    if name.0.is_empty() {
        return Err(ProjectFailure::Invalid("Un nom de projet est nécessaire.").into());
    }
    let repo = repository.repository(
        body.raw("repo")
            .ok_or(ProjectFailure::Invalid("Texte invalide ou trop long."))?,
    )?;
    for project in repository.projects()? {
        if same_repo(&project, &repo, windows)? {
            return Err(
                ProjectFailure::Conflict("Ce répertoire figure déjà dans les projets.").into(),
            );
        }
    }
    let mut project = Document::default();
    project.set("id", &repository.new_id())?;
    project.set("name", &name)?;
    project.set("repo", &repo)?;
    repository.save_project(&project)?;
    Ok(project)
}

fn archive_cards(
    repository: &impl ProjectRepository,
    repo: &JsonString,
    windows: bool,
    busy: &impl Fn(&str) -> bool,
) -> Result<usize> {
    let mut selected = Vec::new();
    let mut ids = Vec::new();
    for card in repository.cards()? {
        if same_repo(&card, repo, windows)? {
            ids.push(
                card.get::<String>("id")?
                    .ok_or("Identifiant de carte absent.")?,
            );
            selected.push(card);
        }
    }
    let conflict = || {
        ProjectFailure::Conflict(
            "Arrête les agents et chronomètres et attends la fin des envois avant de retirer ce projet.",
        )
    };
    if ids.iter().any(|id| busy(id)) {
        return Err(conflict().into());
    }
    for entry in repository.times()? {
        let id = entry.get::<String>("cardId").ok().flatten();
        if id.as_ref().is_some_and(|id| ids.contains(id))
            && (entry.truthy("runningSince")? || entry.string_equals("exportState", "sending")?)
        {
            return Err(conflict().into());
        }
    }
    let count = selected.len();
    for mut card in selected {
        card.set("archived", &true)?;
        card.set("updatedAt", &repository.timestamp()?)?;
        repository.save_card(&card)?;
    }
    Ok(count)
}

pub fn archive_tasks(
    repository: &impl ProjectRepository,
    body: &Document,
    windows: bool,
    busy: impl Fn(&str) -> bool,
) -> Result<usize> {
    let repo = text(body.raw("repo"), 2000)?;
    if repo.0.is_empty() {
        return Err(ProjectFailure::Invalid("Répertoire requis.").into());
    }
    archive_cards(repository, &repo, windows, &busy)
}

pub fn remove(
    repository: &impl ProjectRepository,
    id: &str,
    body: &Document,
    windows: bool,
    busy: impl Fn(&str) -> bool,
    servers_running: bool,
) -> Result<usize> {
    let mut found = None;
    for project in repository.projects()? {
        if project.string_equals("id", id)? {
            found = Some(project);
            break;
        }
    }
    let project = found.ok_or(ProjectFailure::Missing)?;
    let repo = project.get::<JsonString>("repo")?.unwrap_or_default();
    for card in repository.cards()? {
        if same_repo(&card, &repo, windows)?
            && busy(
                &card
                    .get::<String>("id")?
                    .ok_or("Identifiant de carte absent.")?,
            )
        {
            return Err(
                ProjectFailure::Conflict("Attends la fin des opérations du projet.").into(),
            );
        }
    }
    if servers_running {
        return Err(
            ProjectFailure::Conflict("Arrête les serveurs Front / Back de ce projet.").into(),
        );
    }
    if body.raw("archiveTasks").is_some() && body.get::<bool>("archiveTasks").is_err() {
        return Err(ProjectFailure::Invalid("Option d’archivage invalide.").into());
    }
    let archived = if body.truthy("archiveTasks")? {
        archive_cards(repository, &repo, windows, &busy)?
    } else {
        0
    };
    repository.delete_project(id)?;
    Ok(archived)
}
