use super::checked_file::CheckedFile;
use crate::Result;
use atelier_core::{
    document::{Document, JsonString},
    domain::text,
};
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use icu_collator::{Collator, CollatorBorrowed};
use regex::Regex;
use std::{ffi::OsStr, path::Path, sync::LazyLock};

const SPACE: &str =
    r"[\t-\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";
static FRAMEWORK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:vite(?:{SPACE}|$)|next{SPACE}+(?:dev|start)(?:{SPACE}|$)|ng{SPACE}+serve(?:{SPACE}|$))")).unwrap()
});
static CSPROJ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?iu)^[\p{L}\p{N}_. -]+\.csproj$").unwrap());
static PROFILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_. :/()-]+$").unwrap());
static WEB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?-u:\b)(?i-u:Sdk){SPACE}*={SPACE}*["'](?i-u:Microsoft\.NET\.Sdk\.Web)["']"#
    ))
    .unwrap()
});
static EXECUTABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"<(?i-u:OutputType)>{SPACE}*(?i-u:Exe|WinExe){SPACE}*</(?i-u:OutputType)>"
    ))
    .unwrap()
});

fn name_text(value: &OsStr) -> JsonString {
    #[cfg(windows)]
    let units: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().collect()
    };
    #[cfg(unix)]
    let units: Vec<u16> = value.to_string_lossy().encode_utf16().collect();
    JsonString(units)
}

fn path_text(value: &OsStr) -> JsonString {
    let mut units = name_text(value).0;
    for unit in &mut units {
        if *unit == u16::from(b'\\') {
            *unit = u16::from(b'/');
        }
    }
    JsonString(units)
}

fn message(prefix: &str, directory: &JsonString, suffix: &str) -> JsonString {
    let mut result = JsonString::from(prefix);
    result.0.extend(if directory.0.is_empty() {
        vec![u16::from(b'.')]
    } else {
        directory.0.clone()
    });
    result.0.extend(suffix.encode_utf16());
    result
}

fn read(directory: &Dir, file: &Path) -> Result<String> {
    let bytes = CheckedFile::open_in(directory, file)?.read(1024 * 1024)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn object(value: &str) -> Result<Document> {
    let raw: Box<serde_json::value::RawValue> = serde_json::from_str(value)?;
    if raw.get() == "null" {
        return Err("Manifeste nul.".into());
    }
    Ok(Document::spread(&raw)?)
}

fn default_locale() -> String {
    #[cfg(windows)]
    {
        let mut buffer = [0u16; 85];
        // Windows writes at most the supplied capacity, including the trailing NUL.
        let count = unsafe {
            windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
                buffer.as_mut_ptr(),
                buffer.len() as i32,
            )
        };
        if count > 1 {
            return String::from_utf16_lossy(&buffer[..count as usize - 1]);
        }
    }
    #[cfg(unix)]
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            let locale = value.split(['.', '@']).next().unwrap_or("");
            return if matches!(locale, "C" | "POSIX") {
                "en-US".into()
            } else {
                locale.replace('_', "-")
            };
        }
    }
    "en-US".into()
}

fn detect_opened(directory: &Dir) -> Result<Document> {
    let locale = default_locale()
        .parse::<icu_locale_core::Locale>()
        .unwrap_or(icu_locale_core::locale!("en-US"));
    let collator = Collator::try_new(locale.into(), Default::default())?;
    let mut scan = Scan {
        collator,
        visited: 0,
        candidates: Vec::new(),
        warnings: Vec::new(),
    };
    scan.directory(directory, Path::new(""), 0, "npm")?;
    if scan.visited > 300 {
        scan.warnings.push(JsonString::from(
            "Analyse limitée aux 300 premiers dossiers.",
        ));
    }
    let mut result = Document::default();
    result.set("candidates", &scan.candidates)?;
    result.set("warnings", &scan.warnings)?;
    Ok(result)
}

pub fn detect(repo: &Path) -> Result<Document> {
    detect_opened(&Dir::open_ambient_dir(
        std::fs::canonicalize(repo)?,
        ambient_authority(),
    )?)
}

fn synthetic_relative(
    roots: &[std::path::PathBuf],
    repo: &JsonString,
) -> Result<std::path::PathBuf> {
    let path = std::path::PathBuf::from(String::from_utf16_lossy(&repo.0));
    let path = std::path::PathBuf::from(super::task_creation::path_string(&path).utf8()?);
    // Recognize only the selected root's canonical spelling and its original startup spelling.
    let relative = roots
        .iter()
        .find_map(|root| {
            let root =
                std::path::PathBuf::from(super::task_creation::path_string(root).utf8().ok()?)
                    .join("repositories");
            path.strip_prefix(root).ok()
        })
        .ok_or(
            "Le prototype analyse uniquement les dépôts synthétiques de son dossier repositories.",
        )?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Un sous-dossier synthétique sans traversée est requis.".into());
    }
    Ok(Path::new("repositories").join(relative))
}

pub fn detect_synthetic(
    root: &Dir,
    roots: &[std::path::PathBuf],
    repo: &JsonString,
) -> Result<Document> {
    // The HTTP prototype must never scan a caller-supplied directory outside its synthetic data root.
    detect_opened(&open_repository(root, roots, repo)?)
}

pub(super) fn open_repository(
    root: &Dir,
    roots: &[std::path::PathBuf],
    repo: &JsonString,
) -> Result<Dir> {
    match synthetic_relative(roots, repo) {
        Ok(relative) => Ok(root.open_dir_nofollow(relative)?),
        Err(_) => super::repository_access::open(root, Path::new(&repo.utf8()?)),
    }
}

pub fn synthetic_repository(
    root: &Dir,
    roots: &[std::path::PathBuf],
    value: &serde_json::value::RawValue,
) -> Result<JsonString> {
    let input: JsonString =
        serde_json::from_str(value.get()).map_err(|_| "message.invalid_or_overly_long_text")?;
    let repo = atelier_core::domain::text_units(&input, 2000)?;
    let relative = match synthetic_relative(roots, &repo) {
        Ok(relative) => relative,
        Err(_) => {
            let _ = super::repository_access::open(root, Path::new(&repo.utf8()?))?;
            return Ok(repo);
        }
    };
    root.open_dir_nofollow(&relative)?;
    Ok(super::task_creation::path_string(
        &roots
            .first()
            .ok_or("Racine synthétique absente.")?
            .join(relative),
    ))
}

struct Scan {
    collator: CollatorBorrowed<'static>,
    visited: usize,
    candidates: Vec<Document>,
    warnings: Vec<JsonString>,
}

impl Scan {
    fn directory(
        &mut self,
        opened: &Dir,
        relative: &Path,
        depth: usize,
        inherited: &str,
    ) -> Result<()> {
        self.visited += 1;
        if self.visited > 300 {
            return Ok(());
        }
        let directory = path_text(relative.as_os_str());
        let entries = (|| -> std::io::Result<Vec<_>> {
            opened
                .entries()?
                .map(|entry| {
                    let entry = entry?;
                    Ok((entry.file_name(), entry.file_type()?))
                })
                .collect()
        })();
        let mut entries = match entries {
            Ok(entries) => entries,
            Err(_) => {
                self.warnings
                    .push(message("Dossier illisible : ", &directory, ""));
                return Ok(());
            }
        };
        // Node's stable locale sort retains libuv's lexical order for canonically equivalent names.
        entries.sort_by(|(left, _), (right, _)| {
            self.collator
                .compare_utf16(&name_text(left).0, &name_text(right).0)
                .then_with(|| left.to_string_lossy().cmp(&right.to_string_lossy()))
        });
        let has = |name: &str| {
            entries
                .iter()
                .any(|(file, kind)| kind.is_file() && file == name)
        };
        let mut manager = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "yarn"
        } else if has("package-lock.json") {
            "npm"
        } else {
            inherited
        };
        if has("package.json") {
            match self.package(opened, relative, &mut manager) {
                Ok(Some(candidate)) => self.candidates.push(candidate),
                Ok(None) => {}
                Err(_) => self.warnings.push(message(
                    "package.json illisible ou invalide : ",
                    &directory,
                    "",
                )),
            }
        }
        for (file, kind) in &entries {
            if kind.is_file() && file.to_str().is_some_and(|file| CSPROJ.is_match(file)) {
                match self.dotnet(opened, relative, Path::new(file)) {
                    Ok(candidate) => self.candidates.push(candidate),
                    Err(_) => {
                        let mut warning = JsonString::from("Projet .NET illisible : ");
                        warning.0.extend(directory.0.clone());
                        warning.0.push(u16::from(b'/'));
                        warning.0.extend(path_text(file).0);
                        self.warnings.push(warning);
                    }
                }
            }
        }
        if depth < 3 {
            for (file, kind) in entries {
                let name = file.to_string_lossy();
                if !kind.is_dir()
                    || name.starts_with('.')
                    || matches!(
                        name.as_ref(),
                        "node_modules"
                            | "vendor"
                            | "bin"
                            | "obj"
                            | "dist"
                            | "build"
                            | "coverage"
                            | "data"
                            | "runtime"
                    )
                {
                    continue;
                }
                let child = relative.join(&file);
                match opened.open_dir_nofollow(&file) {
                    Ok(opened) => self.directory(&opened, &child, depth + 1, manager)?,
                    Err(_) => {
                        self.visited += 1;
                        if self.visited <= 300 {
                            self.warnings.push(message(
                                "Dossier illisible : ",
                                &path_text(child.as_os_str()),
                                "",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn package(
        &self,
        opened: &Dir,
        relative: &Path,
        manager: &mut &str,
    ) -> Result<Option<Document>> {
        let manifest = object(&read(opened, Path::new("package.json"))?)?;
        if let Some(raw) = manifest
            .raw("packageManager")
            .filter(|value| value.get() != "null")
        {
            let declared: JsonString = serde_json::from_str(raw.get())?;
            let declared = String::from_utf16_lossy(&declared.0);
            match declared.split('@').next().unwrap_or("") {
                "npm" => *manager = "npm",
                "pnpm" => *manager = "pnpm",
                "yarn" => *manager = "yarn",
                _ => {}
            }
        }
        let scripts = manifest
            .raw("scripts")
            .map(Document::spread)
            .transpose()?
            .unwrap_or_default();
        let mut dependencies = manifest
            .raw("dependencies")
            .map(Document::spread)
            .transpose()?
            .unwrap_or_default();
        if let Some(value) = manifest.raw("devDependencies") {
            dependencies.overlay(&Document::spread(value)?);
        }
        let Some(script) = ["dev", "start:dev", "serve", "start"]
            .into_iter()
            .find(|name| {
                scripts
                    .raw(name)
                    .is_some_and(|value| value.get().starts_with('"'))
            })
        else {
            return Ok(None);
        };
        let source = scripts.get::<JsonString>(script)?.unwrap_or_default();
        let source = String::from_utf16_lossy(&source.0);
        let source = text(&source, usize::MAX)?;
        let framework =
            if !source.contains([';', '&', '|', '<', '>']) && FRAMEWORK.is_match(&source) {
                if source.starts_with("vite") {
                    "Vite"
                } else if source.starts_with("next") {
                    "Next.js"
                } else {
                    "Angular"
                }
            } else {
                ""
            };
        let mut technology = framework;
        if technology.is_empty() {
            technology = "Node.js";
            for (name, label) in [
                ("@nestjs/core", "NestJS"),
                ("express", "Express"),
                ("fastify", "Fastify"),
                ("react-scripts", "Create React App"),
            ] {
                if dependencies.truthy(name)? {
                    technology = label;
                    break;
                }
            }
        }
        let mut command = format!("{manager} run {script}");
        if !framework.is_empty() {
            command.push_str(&format!(
                "{} --port {{port}} {} 127.0.0.1{}",
                if *manager == "npm" { " --" } else { "" },
                if framework == "Next.js" {
                    "--hostname"
                } else {
                    "--host"
                },
                if framework == "Vite" {
                    " --strictPort"
                } else {
                    ""
                }
            ));
        }
        let mut result = candidate(relative, Path::new("package.json"))?;
        result.set("command", &command)?;
        result.set("technology", &technology)?;
        result.set(
            "role",
            &if !framework.is_empty() || technology == "Create React App" {
                "front"
            } else {
                "back"
            },
        )?;
        result.set("automaticPort", &!framework.is_empty())?;
        Ok(Some(result))
    }

    fn dotnet(&mut self, opened: &Dir, relative: &Path, file: &Path) -> Result<Document> {
        let source = read(opened, file)?;
        let profiles = match profiles(opened) {
            Ok(profiles) => profiles,
            Err(error) => {
                if !error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                {
                    self.warnings.push(message(
                        "Profils .NET illisibles : ",
                        &path_text(relative.as_os_str()),
                        "/Properties/launchSettings.json",
                    ));
                }
                Vec::new()
            }
        };
        let web = WEB.is_match(&source);
        let default_profile = profiles
            .iter()
            .find(|profile| profile.truthy("supported").unwrap_or(false))
            .and_then(|profile| profile.get::<JsonString>("name").ok().flatten())
            .unwrap_or_default();
        let runnable = web || EXECUTABLE.is_match(&source) || !default_profile.0.is_empty();
        let base = format!(
            "dotnet run --project \"{}\"",
            file.to_str().ok_or("Nom de projet invalide.")?
        );
        let command = if runnable {
            format!(
                "{base} {}{}",
                if default_profile.0.is_empty() {
                    "--no-launch-profile".into()
                } else {
                    format!("--launch-profile \"{}\"", default_profile.utf8()?)
                },
                if web {
                    " -- --urls http://127.0.0.1:{port}"
                } else {
                    ""
                }
            )
        } else {
            String::new()
        };
        let mut result = candidate(relative, file)?;
        result.set("command", &command)?;
        result.set("baseCommand", &base)?;
        result.set("defaultProfile", &default_profile)?;
        result.set("profiles", &profiles)?;
        result.set(
            "technology",
            &if web {
                "ASP.NET Core"
            } else if runnable {
                ".NET"
            } else {
                ".NET (bibliothèque)"
            },
        )?;
        result.set("role", &"back")?;
        result.set("automaticPort", &web)?;
        result.set("runnable", &runnable)?;
        Ok(result)
    }
}

fn candidate(relative: &Path, file: &Path) -> Result<Document> {
    let directory = path_text(relative.as_os_str());
    let mut id = directory.clone();
    id.0.push(u16::from(b'/'));
    id.0.extend(path_text(file.as_os_str()).0);
    let mut result = Document::default();
    result.set("id", &id)?;
    result.set("directory", &directory)?;
    result.set("file", &path_text(relative.join(file).as_os_str()))?;
    Ok(result)
}

fn profiles(opened: &Dir) -> Result<Vec<Document>> {
    let properties = opened.open_dir_nofollow("Properties")?;
    let settings = object(&read(&properties, Path::new("launchSettings.json"))?)?;
    let profiles = settings
        .raw("profiles")
        .map(Document::spread)
        .transpose()?
        .unwrap_or_default();
    let mut keys: Vec<_> = profiles.keys().collect();
    // Object.entries lists canonical array-index keys before insertion-ordered string keys.
    keys.sort_by_key(|name| {
        name.utf8()
            .ok()
            .and_then(|text| {
                text.parse::<u32>()
                    .ok()
                    .filter(|index| *index != u32::MAX && index.to_string() == text)
            })
            .map_or((1, 0), |index| (0, index))
    });
    let mut result = Vec::new();
    for name in keys {
        let raw = profiles.raw_key(name).ok_or("Profil absent.")?;
        if !raw.get().starts_with(['{', '[']) {
            continue;
        }
        let profile = Document::spread(raw)?;
        let supported = profile.string_equals("commandName", "Project")?
            && name.utf8().is_ok_and(|name| PROFILE.is_match(&name));
        let command_name = match profile
            .raw("commandName")
            .filter(|value| value.get().starts_with('"'))
        {
            Some(value) => serde_json::from_str::<JsonString>(value.get())?,
            None => JsonString::default(),
        };
        let mut urls = Vec::new();
        if let Some(value) = profile
            .raw("applicationUrl")
            .filter(|value| value.get().starts_with('"'))
        {
            let value: JsonString = serde_json::from_str(value.get())?;
            for value in String::from_utf16_lossy(&value.0).split(';') {
                if let Ok(url) = reqwest::Url::parse(&text(value, usize::MAX)?)
                    && matches!(url.scheme(), "http" | "https")
                {
                    urls.push(url.origin().ascii_serialization());
                }
            }
        }
        let mut entry = Document::default();
        entry.set("name", name)?;
        entry.set("commandName", &command_name)?;
        entry.set("urls", &urls)?;
        entry.set("supported", &supported)?;
        result.push(entry);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn prototype_scan_refuses_paths_outside_its_synthetic_repository_subtree() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(fixture.path()).unwrap();
        let roots = [path.clone(), fixture.path().to_owned()];
        let root = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
        let repo = path.join("repositories/synthetic");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
        let result = detect_synthetic(
            &root,
            &roots,
            &super::super::task_creation::path_string(&repo),
        )
        .unwrap();
        assert_eq!(
            result
                .get::<Vec<Document>>("candidates")
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        let original =
            serde_json::value::to_raw_value(&fixture.path().join("repositories/synthetic"))
                .unwrap();
        let selected = synthetic_repository(&root, &roots, &original).unwrap();
        assert_eq!(selected, super::super::task_creation::path_string(&repo));
        assert!(
            detect_synthetic(
                &root,
                &roots,
                &serde_json::from_str::<JsonString>(original.get()).unwrap()
            )
            .is_ok()
        );
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("package.json"),
            "synthetic outside sentinel",
        )
        .unwrap();
        for forbidden in [
            outside.path().to_owned(),
            path.clone(),
            path.join("repositories"),
            path.join("repositories/../outside"),
        ] {
            let value = super::super::task_creation::path_string(&forbidden);
            assert!(detect_synthetic(&root, &roots, &value).is_err());
            assert!(
                synthetic_repository(
                    &root,
                    &roots,
                    &serde_json::value::to_raw_value(&value).unwrap()
                )
                .is_err()
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), path.join("repositories/escape")).unwrap();
            assert!(
                detect_synthetic(
                    &root,
                    &roots,
                    &super::super::task_creation::path_string(&path.join("repositories/escape"))
                )
                .is_err()
            );
        }
        #[cfg(windows)]
        {
            use std::{os::windows::process::CommandExt, process::Command};
            let junction = path.join("repositories/escape");
            let output =
                Command::new(std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()))
                    .args(["/D", "/C"])
                    .raw_arg("mklink /J \"%ATELIER_TEST_JUNCTION%\" \"%ATELIER_TEST_TARGET%\"")
                    .env("ATELIER_TEST_JUNCTION", &junction)
                    .env("ATELIER_TEST_TARGET", outside.path())
                    .creation_flags(0x08000000)
                    .output()
                    .unwrap();
            assert!(output.status.success(), "Junction fixture creation failed");
            let escaped = super::super::task_creation::path_string(&junction);
            assert!(detect_synthetic(&root, &roots, &escaped).is_err());
            assert!(
                synthetic_repository(
                    &root,
                    &roots,
                    &serde_json::value::to_raw_value(&escaped).unwrap()
                )
                .is_err()
            );
        }
        assert_eq!(
            fs::read(outside.path().join("package.json")).unwrap(),
            b"synthetic outside sentinel"
        );
    }

    #[test]
    fn manifest_detection_preserves_port_commands_profiles_and_private_values() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::create_dir_all(root.path().join("app/Properties")).unwrap();
        fs::write(
            root.path().join("app/package.json"),
            r#"{"scripts":{"dev":"vite --port 99"}}"#,
        )
        .unwrap();
        fs::write(
            root.path().join("app/Api.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk.Web"/>"#,
        )
        .unwrap();
        fs::write(root.path().join("app/Properties/launchSettings.json"), r#"{"profiles":{"http":{"commandName":"Project","applicationUrl":"https://localhost:7000;http://localhost:5000/path","environmentVariables":{"secret":"do-not-expose"}},"bad%NAME%":{"commandName":"Project"},"\ud800":{"commandName":"Other"}}}"#).unwrap();
        let detected = detect(root.path()).unwrap();
        let candidates: Vec<Document> = detected.get("candidates").unwrap().unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates[0]
                .string_equals(
                    "command",
                    "pnpm run dev --port {port} --host 127.0.0.1 --strictPort"
                )
                .unwrap()
        );
        assert!(candidates[1].string_equals("command", "dotnet run --project \"Api.csproj\" --launch-profile \"http\" -- --urls http://127.0.0.1:{port}").unwrap());
        let profiles: Vec<Document> = candidates[1].get("profiles").unwrap().unwrap();
        assert_eq!(profiles.len(), 3);
        assert!(!profiles[1].truthy("supported").unwrap());
        assert_eq!(
            profiles[2].get::<JsonString>("name").unwrap().unwrap(),
            JsonString(vec![0xd800])
        );
        let encoded = serde_json::to_string(&detected).unwrap();
        assert!(!encoded.contains("do-not-expose"));
        assert!(!encoded.contains("environmentVariables"));
        fs::write(
            root.path().join("app/package.json"),
            r#"{"scripts":{"dev":"vite && echo forbidden"}}"#,
        )
        .unwrap();
        let candidates: Vec<Document> = detect(root.path())
            .unwrap()
            .get("candidates")
            .unwrap()
            .unwrap();
        assert!(
            candidates[0]
                .string_equals("command", "pnpm run dev")
                .unwrap()
        );
        assert!(!candidates[0].truthy("automaticPort").unwrap());
    }
}
