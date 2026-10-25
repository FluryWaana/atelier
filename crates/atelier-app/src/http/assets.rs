use super::{Result, Shared};
use crate::infrastructure::checked_file::CheckedFile;
use axum::{Router, body::Bytes, http::header, routing::get};
use cap_std::{ambient_authority, fs::Dir};
use std::path::Path;

// The temporary legacy build copies only assets already exposed by the Node server.
pub const LEGACY_ASSETS: &[&str] = &[
    "index.html",
    "app.js",
    "style.css",
    "atelier-kanban.png",
    "local-session.mjs",
    "agent-options.mjs",
    "agent-usage.mjs",
    "connector-availability.mjs",
    "i18n.mjs",
    "syntax-highlight.mjs",
    "ui-actions.mjs",
    "ui-helpers.mjs",
    "updates.mjs",
    "view-helpers.mjs",
    "workflow.mjs",
    "locales/index.mjs",
    "locales/fr-FR.mjs",
    "locales/en-US.mjs",
    "vendor/highlightjs/highlight.min.js",
    "views/agent-interactions.mjs",
    "views/agent-providers.mjs",
    "views/board.mjs",
    "views/brainstorm-questions.mjs",
    "views/brainstorm.mjs",
    "views/card.mjs",
    "views/overview.mjs",
    "views/processes.mjs",
    "views/project-settings.mjs",
    "views/projects.mjs",
    "views/settings.mjs",
    "views/task-editor.mjs",
    "views/task-workspace.mjs",
    "views/time.mjs",
    "views/workspace-workflows.mjs",
    "lib/domain/agent-options.mjs",
    "lib/domain/tasks.mjs",
    "lib/domain/paths.mjs",
    "lib/domain/validation.mjs",
];

pub fn wasm_asset_name(name: &str) -> bool {
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return false;
    }
    name == "index.html"
        || name == "atelier-loader.js"
        || (name.starts_with("atelier-ui-") && (name.ends_with(".js") || name.ends_with(".wasm")))
        || (name.starts_with("style-") && name.ends_with(".css"))
}

pub(super) fn router(path: &Path) -> Result<Router<Shared>> {
    let directory = Dir::open_ambient_dir(path, ambient_authority())?;
    let names: Vec<String> = if directory.symlink_metadata("app.js").is_ok() {
        LEGACY_ASSETS.iter().map(|name| (*name).into()).collect()
    } else {
        let mut names = Vec::new();
        for entry in directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if wasm_asset_name(name) {
                names.push(name.into());
            }
        }
        names
    };
    let mut router = Router::new();
    for name in names {
        // Capture checked bytes once; replacing the path later cannot publish another file.
        let bytes = Bytes::from(
            CheckedFile::open_in(&directory, Path::new(&name))?.read(64 * 1024 * 1024)?,
        );
        let mime = match Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("js" | "mjs") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("wasm") => "application/wasm",
            Some("png") => "image/png",
            _ => "text/html; charset=utf-8",
        };
        let route = if name == "index.html" {
            "/".into()
        } else {
            format!("/{name}")
        };
        router = router.route(
            &route,
            get(move || {
                let bytes = bytes.clone();
                async move { ([(header::CONTENT_TYPE, mime)], bytes) }
            }),
        );
    }
    Ok(router)
}
