use crate::Result;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub fn pending(root: &Path) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        native::pending(root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Ok(false)
    }
}

pub async fn command(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let input = Input::parse(args)?;
    #[cfg(target_os = "linux")]
    {
        // System package code is managed by the administrator, never through a user's database.
        if unsafe { libc::geteuid() } != 0 {
            return Err(
                "Cette opération de paquet système exige le gestionnaire administrateur.".into(),
            );
        }
        native::execute(input).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let Input {
            action,
            root,
            source,
        } = input;
        let _ = (action, root, source);
        Err("Cette commande concerne uniquement les paquets système Linux.".into())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum Action {
    Apply,
    Recover,
    Remove,
    Reserve,
}
struct Input {
    action: Action,
    root: PathBuf,
    source: Option<[PathBuf; 3]>,
}
impl Input {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        let action = match args.next().as_deref().and_then(|value| value.to_str()) {
            Some("apply") => Action::Apply,
            Some("recover") => Action::Recover,
            Some("remove") => Action::Remove,
            Some("reserve") => Action::Reserve,
            _ => return Err("Action de paquet système inconnue.".into()),
        };
        let mut values = std::collections::BTreeMap::new();
        let mut confirmed = false;
        while let Some(flag) = args.next() {
            if flag == "--confirm" && !confirmed {
                confirmed = true;
                continue;
            }
            let flag = flag.to_str().ok_or("Option système invalide.")?.to_owned();
            if !["--root", "--archive", "--signature", "--trust"].contains(&flag.as_str())
                || values.contains_key(&flag)
            {
                return Err("Option système inconnue ou répétée.".into());
            }
            let path = args
                .next()
                .map(PathBuf::from)
                .ok_or("Chemin système absent.")?;
            if !path.is_absolute()
                || path
                    .to_str()
                    .is_none_or(|path| path.len() > 32_000 || path.chars().any(char::is_control))
            {
                return Err("Chemin système absolu requis.".into());
            }
            values.insert(flag, path);
        }
        if !confirmed {
            return Err("Confirme explicitement l’opération sur le paquet système.".into());
        }
        let root = values.remove("--root").ok_or("Dossier système absent.")?;
        let source = if action == Action::Apply {
            Some([
                values
                    .remove("--archive")
                    .ok_or("Archive système absente.")?,
                values
                    .remove("--signature")
                    .ok_or("Signature système absente.")?,
                values
                    .remove("--trust")
                    .ok_or("Confiance système absente.")?,
            ])
        } else {
            None
        };
        if !values.is_empty() {
            return Err(
                "Les chemins source concernent uniquement l’application d’un paquet.".into(),
            );
        }
        Ok(Self {
            action,
            root,
            source,
        })
    }
}

#[cfg(target_os = "linux")]
mod native {
    use super::super::{
        checked_file::CheckedFile,
        installation::{self, InstallationMode},
        installed::{self, Current, Version},
        package, publish, update_package, updates,
    };
    use super::*;
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
    use cap_std::fs::{Dir, MetadataExt, OpenOptionsExt};
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeSet, os::fd::AsRawFd, os::unix::fs::PermissionsExt, time::Duration,
    };
    const FORMAT: &str = "atelier-system-package/1";
    const MARKER: &str = ".package-maintenance";
    #[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    enum Phase {
        Preparing,
        Selecting,
        Committed,
        Removing,
        Removed,
        Failed,
        Uncertain,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Owned {
        path: String,
        sha256: String,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Journal {
        format: String,
        id: String,
        action: Action,
        phase: Phase,
        root: PathBuf,
        root_identity: Option<(u64, u64)>,
        expected: Option<Current>,
        next: Option<Version>,
        key_id: String,
        stage: Option<String>,
        prepared: Option<(PathBuf, (u64, u64))>,
        receipt: Vec<u8>,
        rescue: Option<String>,
        files: Vec<Owned>,
        message: String,
    }
    impl Journal {
        fn pending(&self) -> bool {
            matches!(
                self.phase,
                Phase::Preparing | Phase::Selecting | Phase::Removing | Phase::Uncertain
            )
        }
    }
    struct Scope {
        root: PathBuf,
        parent: Dir,
        key: String,
        _external: std::fs::File,
        _running: Option<std::fs::File>,
    }
    impl Scope {
        fn open(root: &Path) -> Result<Self> {
            let parent_path = root.parent().ok_or("Parent système absent.")?;
            let parent = super::super::repository_access::absolute(parent_path)?;
            let metadata = parent.dir_metadata()?;
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
                return Err("Parent système privé à l’administrateur requis.".into());
            }
            let leaf = root
                .file_name()
                .filter(|name| !name.is_empty())
                .ok_or("Dossier système distinct requis.")?;
            let root = std::fs::canonicalize(parent_path)?.join(leaf);
            let key = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
            let mut options = cap_std::fs::OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create(true)
                .mode(0o600)
                .follow(FollowSymlinks::No)
                .nonblock(true);
            let file = parent
                .open_with(format!(".atelier-system-package-{key}.lock"), &options)?
                .into_std();
            CheckedFile::from_file(file.try_clone()?)?
                .single_link()?
                .private()?;
            if file.metadata()?.len() != 0 {
                return Err("Verrou système modifié ; contenu conservé.".into());
            }
            lock(&file)?;
            let running = if root.try_exists()? {
                let directory = super::super::repository_access::absolute(&root)?;
                match directory.symlink_metadata(".package-lock") {
                    Ok(_) => {
                        let file = CheckedFile::open_in(&directory, Path::new(".package-lock"))?
                            .single_link()?;
                        let mut options = cap_std::fs::OpenOptions::new();
                        options.read(true).follow(FollowSymlinks::No).nonblock(true);
                        let running = directory.open_with(".package-lock", &options)?.into_std();
                        file.open_same(&root.join(".package-lock"))?;
                        if running.metadata()?.len() != 0 {
                            return Err("Verrou système modifié.".into());
                        }
                        lock(&running)?;
                        Some(running)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                }
            } else {
                None
            };
            Ok(Self {
                root,
                parent,
                key,
                _external: file,
                _running: running,
            })
        }
        fn name(&self) -> String {
            format!(".atelier-system-package-{}.json", self.key)
        }
        fn read(&self) -> Result<Option<Journal>> {
            read_journal(&self.parent, &self.root, &self.key)
        }
        fn save(&self, journal: &Journal) -> Result<()> {
            let bytes = serde_json::to_vec(journal)?;
            if bytes.len() > 1024 * 1024 {
                return Err("Journal système trop volumineux.".into());
            }
            publish::write_private(&self.parent, Path::new(&self.name()), &bytes, false)?;
            // This ledger contains only public code/signature metadata; users need it for a fail-closed startup gate.
            readable_file(&self.parent, &self.name(), false)?;
            self.parent.try_clone()?.into_std_file().sync_all()?;
            Ok(())
        }
        fn directory(&self, journal: &Journal) -> Result<Dir> {
            let directory = super::super::repository_access::absolute(&self.root)?;
            let metadata = directory.dir_metadata()?;
            if journal
                .root_identity
                .is_some_and(|identity| identity != (metadata.dev(), metadata.ino()))
            {
                return Err("Le dossier système a été remplacé ; contenu conservé.".into());
            }
            Ok(directory)
        }
    }
    fn read_journal(parent: &Dir, root: &Path, key: &str) -> Result<Option<Journal>> {
        let name = format!(".atelier-system-package-{key}.json");
        match parent.symlink_metadata(&name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
            Ok(metadata) => {
                super::super::checked_file::require_plain(&metadata)?;
                if metadata.uid() != parent.dir_metadata()?.uid() || metadata.mode() & 0o022 != 0 {
                    return Err("Protection du journal système invalide ; démarrage refusé.".into());
                }
                let journal: Journal = serde_json::from_slice(
                    &CheckedFile::open_in(parent, Path::new(&name))?
                        .single_link()?
                        .read(1024 * 1024)?,
                )?;
                if journal.format != FORMAT
                    || journal.root != root
                    || uuid::Uuid::parse_str(&journal.id).is_err()
                    || journal.files.len() > 4096
                    || journal.receipt.len() > 16 * 1024
                    || journal.action == Action::Recover
                    || matches!(
                        (journal.action, journal.phase),
                        (
                            Action::Reserve,
                            Phase::Selecting | Phase::Committed | Phase::Removing | Phase::Removed
                        ) | (Action::Apply, Phase::Removing | Phase::Removed)
                            | (
                                Action::Remove,
                                Phase::Preparing | Phase::Selecting | Phase::Committed
                            )
                    )
                {
                    return Err("Journal système incohérent ; aucun changement appliqué.".into());
                }
                for file in &journal.files {
                    relative(&file.path)?;
                    if file.sha256.len() != 64
                        || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                    {
                        return Err("Inventaire système invalide.".into());
                    }
                }
                Ok(Some(journal))
            }
        }
    }
    pub(super) fn pending(root: &Path) -> Result<bool> {
        let parent_path = root.parent().ok_or("Parent système absent.")?;
        let parent = super::super::repository_access::absolute(parent_path)?;
        let root = std::fs::canonicalize(parent_path)?
            .join(root.file_name().ok_or("Dossier système absent.")?);
        let key = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
        Ok(read_journal(&parent, &root, &key)?.is_some_and(|journal| journal.pending()))
    }
    fn lock(file: &std::fs::File) -> Result<()> {
        // Both package operations and packaged servers retain these advisory locks until complete.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Arrête toutes les instances Atelier et les autres opérations du gestionnaire de paquets.".into());
        }
        Ok(())
    }
    fn relative(path: &str) -> Result<()> {
        if path.is_empty()
            || path.len() > 4096
            || path.chars().any(char::is_control)
            || Path::new(path)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("Chemin de fichier système invalide.".into());
        }
        Ok(())
    }
    fn record_once(directory: &Dir, name: &str, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        match directory.symlink_metadata(name) {
            Ok(_) => {
                if CheckedFile::open_in(directory, Path::new(name))?
                    .single_link()?
                    .read(1024 * 1024)?
                    != bytes
                {
                    return Err("Un reçu de conservation a été modifié ; contenu préservé.".into());
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let temporary = format!(".atelier-system-record-{}", uuid::Uuid::new_v4());
        let result = (|| -> Result<()> {
            let mut options = cap_std::fs::OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(0o600)
                .follow(FollowSymlinks::No);
            let mut file = directory.open_with(&temporary, &options)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            publish::rename_new(directory, Path::new(&temporary), Path::new(name))?;
            directory.try_clone()?.into_std_file().sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = directory.remove_file(&temporary);
        }
        result
    }
    fn parent_file(directory: &Dir, path: &str) -> Result<(Dir, OsString)> {
        relative(path)?;
        let path = Path::new(path);
        let mut parent = directory.try_clone()?;
        if let Some(parts) = path.parent() {
            for part in parts.components() {
                parent = parent.open_dir_nofollow(part.as_os_str())?;
            }
        }
        Ok((
            parent,
            path.file_name().ok_or("Nom système absent.")?.to_owned(),
        ))
    }
    fn digest(directory: &Dir, path: &str) -> Result<String> {
        let (parent, name) = parent_file(directory, path)?;
        let bytes = CheckedFile::open_in(&parent, Path::new(&name))?
            .single_link()?
            .read(128 * 1024 * 1024)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    fn marker(journal: &Journal) -> Vec<u8> {
        format!("atelier-system-package-maintenance/1 {}\n", journal.id).into_bytes()
    }
    fn check_marker(scope: &Scope, journal: Option<&Journal>) -> Result<()> {
        let directory = super::super::repository_access::absolute(&scope.root)?;
        match directory.symlink_metadata(MARKER) {
            Ok(_) => {
                let expected = journal.ok_or(
                    "Une maintenance existe déjà ; récupère son état avant de poursuivre.",
                )?;
                scope.directory(expected)?;
                if CheckedFile::open_in(&directory, Path::new(MARKER))?
                    .single_link()?
                    .read(128)?
                    != marker(expected)
                {
                    return Err("Marqueur de maintenance étranger ; contenu conservé.".into());
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && journal.is_none() => {
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err("La réservation système a disparu ; aucun changement appliqué.".into())
            }
            Err(error) => Err(error.into()),
        }
    }
    fn reserve_marker(scope: &Scope, journal: &Journal) -> Result<()> {
        use std::io::Write;
        let directory = scope.directory(journal)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .follow(FollowSymlinks::No);
        let mut file = directory.open_with(MARKER, &options)?;
        file.write_all(&marker(journal))?;
        file.sync_all()?;
        Ok(())
    }
    fn fresh_controls(directory: &Dir, journal: &Journal) -> Result<()> {
        use std::io::Write;
        match directory.symlink_metadata(".package-lock") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut options = cap_std::fs::OpenOptions::new();
                options
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .follow(FollowSymlinks::No);
                directory.open_with(".package-lock", &options)?.sync_all()?;
            }
            Ok(_) => {
                if !CheckedFile::open_in(directory, Path::new(".package-lock"))?
                    .single_link()?
                    .read(1)?
                    .is_empty()
                {
                    return Err("Verrou système initial modifié.".into());
                }
            }
            Err(error) => return Err(error.into()),
        }
        match directory.symlink_metadata(MARKER) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut options = cap_std::fs::OpenOptions::new();
                options
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .follow(FollowSymlinks::No);
                let mut file = directory.open_with(MARKER, &options)?;
                file.write_all(&marker(journal))?;
                file.sync_all()?;
            }
            Ok(_) => {
                if CheckedFile::open_in(directory, Path::new(MARKER))?
                    .single_link()?
                    .read(128)?
                    != marker(journal)
                {
                    return Err("Marqueur système initial modifié.".into());
                }
            }
            Err(error) => return Err(error.into()),
        }
        directory.try_clone()?.into_std_file().sync_all()?;
        Ok(())
    }
    fn retained_limit(scope: &Scope, prefix: &str) -> Result<()> {
        let mut retained = 0;
        for (index, entry) in scope.parent.entries()?.enumerate() {
            if index >= 10_000 {
                return Err("Dossier système parent trop volumineux.".into());
            }
            if entry?
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(prefix))
            {
                retained += 1;
                if retained >= 2 {
                    return Err("Deux préparations ou retraits sont déjà conservés ; examine leurs reçus avant de poursuivre.".into());
                }
            }
        }
        Ok(())
    }
    fn clear_marker(scope: &Scope, journal: &Journal) -> Result<()> {
        let directory = scope.directory(journal)?;
        let found = match CheckedFile::open_in(&directory, Path::new(MARKER)) {
            Ok(file) => file.single_link()?.read(128)?,
            Err(error)
                if !journal.pending()
                    && error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if found != marker(journal) {
            return Err("Marqueur de maintenance remplacé ; contenu conservé.".into());
        }
        directory.remove_file(MARKER)?;
        directory.try_clone()?.into_std_file().sync_all()?;
        Ok(())
    }
    fn identity(root: &Path) -> Result<(u64, u64)> {
        let meta = super::super::repository_access::absolute(root)?.dir_metadata()?;
        Ok((meta.dev(), meta.ino()))
    }
    async fn runtime(binary: &Path, target: &str, version: &str) -> Result<()> {
        let mut command = tokio::process::Command::new(binary);
        command
            .arg("inspect-runtime")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C");
        let output =
            super::super::process_tree::capture(&mut command, &[], Duration::from_secs(10), 4096)
                .await?;
        if !output.status.success() {
            return Err("La version native préparée ne peut pas être exécutée.".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if value["protocol"] != "atelier-runtime/1"
            || value["target"] != target
            || value["version"] != version
        {
            return Err(
                "La version réellement exécutée ne correspond pas au paquet système signé.".into(),
            );
        }
        Ok(())
    }
    fn readable_file(directory: &Dir, path: &str, executable: bool) -> Result<()> {
        let (parent, name) = parent_file(directory, path)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let file = parent.open_with(&name, &options)?.into_std();
        CheckedFile::from_file(file.try_clone()?)?.single_link()?;
        file.set_permissions(std::fs::Permissions::from_mode(if executable {
            0o755
        } else {
            0o644
        }))?;
        Ok(())
    }
    fn readable(installation: &installed::Installation, version: &Version) -> Result<()> {
        installation.verify_version(version)?;
        let directory = installation.directory();
        for path in ["", "versions", "receipts"] {
            let dir = if path.is_empty() {
                directory.try_clone()?
            } else {
                directory.open_dir_nofollow(path)?
            };
            dir.into_std_file()
                .set_permissions(std::fs::Permissions::from_mode(0o755))?;
        }
        for name in [
            "atelier",
            "installation.json",
            "trusted-update-key.json",
            "bootstrap-receipt.json",
            "bootstrap-manifest.json",
            "current.json",
            ".package-lock",
        ] {
            readable_file(directory, name, name == "atelier")?;
        }
        readable_file(directory, &format!("receipts/{}.json", version.id), false)?;
        let slot = directory
            .open_dir_nofollow("versions")?
            .open_dir_nofollow(&version.id)?;
        slot.try_clone()?
            .into_std_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
        slot.open_dir_nofollow("ui")?
            .into_std_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
        package::verify(&installation.slot_path(version)?)?;
        let manifest: serde_json::Value = serde_json::from_slice(
            &CheckedFile::open_in(&slot, Path::new("atelier-package.json"))?.read(1024 * 1024)?,
        )?;
        for file in manifest["files"]
            .as_array()
            .ok_or("Inventaire système absent.")?
        {
            let path = file["path"].as_str().ok_or("Chemin système absent.")?;
            readable_file(&slot, path, path == "atelier")?;
        }
        readable_file(&slot, "atelier-package.json", false)?;
        Ok(())
    }
    fn new_journal(scope: &Scope, action: Action) -> Journal {
        Journal {
            format: FORMAT.into(),
            id: uuid::Uuid::new_v4().to_string(),
            action,
            phase: Phase::Preparing,
            root: scope.root.clone(),
            root_identity: None,
            expected: None,
            next: None,
            key_id: String::new(),
            stage: None,
            prepared: None,
            receipt: vec![],
            rescue: None,
            files: vec![],
            message: "Opération de code système préparée ; aucune donnée ouverte.".into(),
        }
    }

    pub(super) async fn execute(input: Input) -> Result<()> {
        let scope = Scope::open(&input.root)?;
        let mut previous = scope.read()?;
        if input.action == Action::Recover {
            return match previous {
                Some(previous) => recover(&scope, previous).await,
                None => {
                    println!("Aucune opération système à récupérer.");
                    Ok(())
                }
            };
        }
        if input.action == Action::Remove
            && previous
                .as_ref()
                .is_some_and(|previous| previous.action == Action::Remove)
        {
            return resume_remove(
                &scope,
                &mut previous.take().ok_or("Journal système absent.")?,
            );
        }
        if previous
            .as_ref()
            .is_some_and(|previous| previous.pending() && previous.action == Action::Apply)
            && matches!(input.action, Action::Apply | Action::Reserve)
        {
            recover(&scope, previous.take().ok_or("Journal système absent.")?).await?;
            previous = scope.read()?;
        }
        if previous.as_ref().is_some_and(|previous| {
            previous.pending()
                && !(input.action == Action::Apply && previous.action == Action::Reserve)
        }) {
            return Err(
                "Récupère l’opération système interrompue avant une nouvelle action.".into(),
            );
        }
        match input.action {
            Action::Apply => {
                apply(
                    &scope,
                    input.source.ok_or("Source système absente.")?,
                    previous.filter(|previous| {
                        previous.pending() && previous.action == Action::Reserve
                    }),
                )
                .await
            }
            Action::Remove => remove(&scope).await,
            Action::Reserve => reserve(&scope),
            Action::Recover => unreachable!(),
        }
    }
    fn reserve(scope: &Scope) -> Result<()> {
        if !scope.root.try_exists()? {
            return Ok(());
        }
        check_marker(scope, None)?;
        if scope._running.is_none() || installation::mode(&scope.root)? != InstallationMode::System
        {
            return Err("Installation système existante étrangère ; contenu conservé.".into());
        }
        installed::verify_all(&scope.root, package::native_target()?)?;
        if updates::status(Some(&scope.root))?
            .operation
            .is_some_and(|operation| operation.state.pending())
        {
            return Err("Une autre mise à jour Rust reste non résolue.".into());
        }
        let installation = installed::open(&scope.root, package::native_target()?)?
            .ok_or("Installation système absente.")?;
        let mut journal = new_journal(scope, Action::Reserve);
        journal.expected = Some(installation.current().clone());
        journal.key_id = installation.trust().key_id().into();
        journal.root_identity = Some(identity(&scope.root)?);
        scope.save(&journal)?;
        reserve_marker(scope, &journal)?;
        println!("Maintenance système réservée avant dépaquetage ; aucune donnée ouverte.");
        Ok(())
    }
    async fn apply(
        scope: &Scope,
        source: [PathBuf; 3],
        mut reservation: Option<Journal>,
    ) -> Result<()> {
        retained_limit(scope, &format!(".atelier-system-failed-{}-", scope.key))?;
        let target = package::native_target()?;
        let trust = update_package::Trust::load(&source[2])?;
        let receipt = package::read(&source[1], 16 * 1024)?;
        let payload = update_package::authenticated_payload(&receipt, &trust, target)?;
        let mut current = if scope.root.try_exists()? {
            check_marker(scope, reservation.as_ref())?;
            if installation::mode(&scope.root)? != InstallationMode::System {
                return Err("Une installation étrangère ne peut pas devenir un paquet système implicitement.".into());
            }
            installed::verify_all(&scope.root, target)?;
            let installation = installed::open(&scope.root, target)?
                .ok_or("Installation système étrangère ; contenu conservé.")?;
            if installation.trust().key_id() != trust.key_id() {
                return Err("La clé du paquet diffère de la confiance installée.".into());
            }
            if updates::status(Some(&scope.root))?
                .operation
                .is_some_and(|operation| operation.state.pending())
            {
                return Err("Une autre mise à jour Rust est encore non résolue.".into());
            }
            if scope._running.is_none() {
                return Err("Verrou du paquet installé absent ; contenu conservé.".into());
            }
            if installation.current().active == Version::from_payload(&payload) {
                runtime(&installation.active_executable()?, target, &payload.version).await?;
                if let Some(mut reserved) = reservation.take() {
                    if reserved.expected.as_ref() != Some(installation.current())
                        || reserved.key_id != trust.key_id()
                    {
                        return Err(
                            "Sélection ou confiance système modifiée depuis la réservation.".into(),
                        );
                    }
                    reserved.action = Action::Apply;
                    reserved.phase = Phase::Committed;
                    reserved.next = Some(Version::from_payload(&payload));
                    reserved.receipt = receipt;
                    reserved.message =
                        "Paquet système exact déjà actif ; réservation clôturée après relecture."
                            .into();
                    scope.save(&reserved)?;
                    clear_marker(scope, &reserved)?;
                }
                println!("Le paquet système exact est déjà actif et vérifié.");
                return Ok(());
            }
            Some(installation)
        } else {
            None
        };
        let from = current
            .as_ref()
            .map(|current| current.current().active.version.clone())
            .unwrap_or_else(|| "0.0.0".into());
        if package::version_parts(&payload.version)? <= package::version_parts(&from)? {
            return Err("Le paquet système doit être une version strictement supérieure.".into());
        }
        let resumed = reservation.is_some();
        let mut journal = reservation.unwrap_or_else(|| new_journal(scope, Action::Apply));
        if resumed
            && (journal.expected.as_ref() != current.as_ref().map(|current| current.current())
                || journal.key_id != trust.key_id())
        {
            return Err("La sélection ou la confiance a changé après réservation système.".into());
        }
        journal.action = Action::Apply;
        journal.expected = current.as_ref().map(|current| current.current().clone());
        journal.next = Some(Version::from_payload(&payload));
        journal.key_id = trust.key_id().into();
        journal.receipt = receipt;
        journal.root_identity = if current.is_some() {
            Some(identity(&scope.root)?)
        } else {
            None
        };
        if current.is_none() {
            journal.stage = Some(format!(
                ".atelier-system-install-{}-{}",
                scope.key, journal.id
            ));
        }
        scope.save(&journal)?;
        if current.is_some() && !resumed {
            reserve_marker(scope, &journal)?;
        }
        let result: Result<()> = async {
            let prepared = update_package::prepare(
                &source[0],
                &source[1],
                &trust,
                target,
                &from,
                scope.root.parent().ok_or("Parent système absent.")?,
            )?;
            if Some(Version::from_payload(prepared.payload())) != journal.next
                || prepared.receipt() != journal.receipt
            {
                return Err("Le paquet système a changé pendant la préparation.".into());
            }
            journal.prepared = Some((
                prepared.package().path().to_owned(),
                identity(prepared.package().path())?,
            ));
            scope.save(&journal)?;
            runtime(
                &prepared
                    .package()
                    .path()
                    .join(package::binary_name(target)?),
                target,
                &payload.version,
            )
            .await?;
            if let Some(installation) = current.as_mut() {
                let next = installation.stage_system_update(prepared)?;
                readable(installation, &next)?;
                journal.phase = Phase::Selecting;
                scope.save(&journal)?;
                let expected = journal
                    .expected
                    .clone()
                    .ok_or("Ancienne sélection absente.")?;
                installation.write_current(
                    &expected,
                    Current {
                        format: expected.format.clone(),
                        active: next.clone(),
                        previous: Some(expected.active.clone()),
                    },
                )?;
                readable(installation, &next)?;
            } else {
                let stage = journal
                    .stage
                    .as_ref()
                    .ok_or("Préparation système absente.")?;
                let staged_root = scope
                    .root
                    .parent()
                    .ok_or("Parent système absent.")?
                    .join(stage);
                installed::install(
                    &source[0],
                    &source[1],
                    &source[2],
                    &staged_root,
                    InstallationMode::System,
                    target,
                )?;
                journal.root_identity = Some(identity(&staged_root)?);
                let directory = super::super::repository_access::absolute(&staged_root)?;
                fresh_controls(&directory, &journal)?;
                let installation = installed::open(&staged_root, target)?
                    .ok_or("Installation système absente.")?;
                readable(&installation, &installation.current().active)?;
                journal.phase = Phase::Selecting;
                scope.save(&journal)?;
                publish::rename_new(
                    &scope.parent,
                    Path::new(stage),
                    Path::new(scope.root.file_name().ok_or("Nom système absent.")?),
                )?;
            }
            installed::verify_all(&scope.root, target)?;
            let observed =
                installed::open(&scope.root, target)?.ok_or("Installation système absente.")?;
            if observed.current().active != Version::from_payload(&payload) {
                return Err("La sélection système a changé.".into());
            }
            runtime(&observed.active_executable()?, target, &payload.version).await?;
            // Persist the selected current.json name before committing the external ledger.
            observed
                .directory()
                .try_clone()?
                .into_std_file()
                .sync_all()?;
            journal.phase = Phase::Committed;
            journal.message =
                "Code système vérifié et sélectionné ; aucune base ouverte ou restaurée.".into();
            scope.save(&journal)?;
            clear_marker(scope, &journal)?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            journal.phase = Phase::Uncertain;
            journal.message="Opération système interrompue ; récupère-la hors ligne, sans modification des données.".into();
            scope.save(&journal)?;
            return Err(error);
        }
        println!("Paquet système vérifié : {}", payload.version);
        Ok(())
    }

    async fn recover(scope: &Scope, mut journal: Journal) -> Result<()> {
        if journal.action == Action::Remove {
            return resume_remove(scope, &mut journal);
        }
        if !scope.root.try_exists()? {
            if journal.expected.is_some() {
                return Err(
                    "Installation système disparue ; aucune reconstruction implicite.".into(),
                );
            }
            if journal.root_identity.is_some() && journal.stage.is_some() {
                let stage = journal
                    .stage
                    .as_ref()
                    .filter(|name| {
                        name.as_str()
                            == format!(".atelier-system-install-{}-{}", scope.key, journal.id)
                    })
                    .ok_or("Préparation système inconnue.")?;
                let path = scope
                    .root
                    .parent()
                    .ok_or("Parent système absent.")?
                    .join(stage);
                if Some(identity(&path)?) != journal.root_identity {
                    return Err("Préparation système remplacée ; aucun dossier adopté.".into());
                }
                installed::verify_all(&path, package::native_target()?)?;
                let staged = installed::open(&path, package::native_target()?)?
                    .ok_or("Préparation système absente.")?;
                if journal.next.as_ref() != Some(&staged.current().active)
                    || staged.trust().key_id() != journal.key_id
                {
                    return Err(
                        "La préparation a changé après réservation ; contenu conservé.".into(),
                    );
                }
                fresh_controls(staged.directory(), &journal)?;
                publish::rename_new(
                    &scope.parent,
                    Path::new(stage),
                    Path::new(scope.root.file_name().ok_or("Nom système absent.")?),
                )?;
            } else {
                journal.phase = Phase::Failed;
                journal.message =
                    "Préparation interrompue avant publication ; aucune donnée modifiée.".into();
                scope.save(&journal)?;
                return Ok(());
            }
        }
        scope.directory(&journal)?;
        let target = package::native_target()?;
        let mut installation = installed::open_recovery(&scope.root, target)?
            .ok_or("Installation système incomplète ; examen requis.")?;
        if installation.trust().key_id() != journal.key_id {
            return Err("Confiance système changée ; récupération refusée.".into());
        }
        if journal.action == Action::Reserve {
            if journal.expected.as_ref() != Some(installation.current()) {
                return Err(
                    "Sélection modifiée après réservation système ; contenu conservé.".into(),
                );
            }
            installed::verify_all(&scope.root, target)?;
            journal.phase = Phase::Failed;
            journal.message =
                "Réservation de maintenance abandonnée ; code précédent vérifié et conservé."
                    .into();
            scope.save(&journal)?;
            clear_marker(scope, &journal)?;
            return Ok(());
        }
        let next = journal
            .next
            .clone()
            .ok_or("Version système préparée absente.")?;
        if installation.current().active == next {
            if journal.expected.is_none() {
                fresh_controls(&scope.directory(&journal)?, &journal)?;
            }
            if installation.verify_version(&next).is_err()
                || runtime(&installation.active_executable()?, target, &next.version)
                    .await
                    .is_err()
            {
                let old = journal
                    .expected
                    .clone()
                    .ok_or("Premier paquet invalide ; aucun code précédent connu.")?;
                installation.verify_version(&old.active)?;
                let actual = installation.current().clone();
                installation.write_current(&actual, old.clone())?;
                readable(&installation, &old.active)?;
                journal.phase = Phase::Failed;
                journal.message =
                    "Version système précédente restaurée ; aucune base touchée.".into();
            } else {
                readable(&installation, &next)?;
                journal.phase = Phase::Committed;
                journal.message =
                    "Version système relue après interruption ; aucune commande distante rejouée."
                        .into();
            }
        } else if journal.expected.as_ref() == Some(installation.current()) {
            installation.verify_version(&installation.current().active)?;
            readable(&installation, &installation.current().active)?;
            if journal.pending() {
                preserve_preparation(scope, &mut journal, &installation, &next)?;
            }
            journal.phase = Phase::Failed;
            journal.message="Ancienne version conservée ; préparation inactive laissée pour examen, aucune donnée touchée.".into();
        } else {
            return Err(
                "La sélection système a été modifiée ; récupération automatique refusée.".into(),
            );
        }
        installation
            .directory()
            .try_clone()?
            .into_std_file()
            .sync_all()?;
        scope.save(&journal)?;
        clear_marker(scope, &journal)?;
        println!("{}", journal.message);
        Ok(())
    }

    fn preserve_preparation(
        scope: &Scope,
        journal: &mut Journal,
        installation: &installed::Installation,
        next: &Version,
    ) -> Result<()> {
        let versions = installation.directory().open_dir_nofollow("versions")?;
        if versions.symlink_metadata(&next.id).is_ok() {
            installation.verify_version(next)?;
            return Ok(());
        }
        let retained = format!(".atelier-system-failed-{}-{}", scope.key, journal.id);
        if journal.rescue.is_none() {
            scope.parent.create_dir(&retained)?;
            scope
                .parent
                .open_dir_nofollow(&retained)?
                .into_std_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))?;
            journal.rescue = Some(retained.clone());
            scope.save(journal)?;
        }
        if journal.rescue.as_deref() != Some(retained.as_str()) {
            return Err("Conservation système étrangère ; contenu préservé.".into());
        }
        let destination = scope.parent.open_dir_nofollow(&retained)?;
        let receipts = installation.directory().open_dir_nofollow("receipts")?;
        let name = format!("{}.json", next.id);
        match receipts.symlink_metadata(&name) {
            Ok(_) => {
                let bytes = CheckedFile::open_in(&receipts, Path::new(&name))?
                    .single_link()?
                    .read(16 * 1024)?;
                if bytes != journal.receipt
                    || Version::from_payload(&update_package::authenticated_payload(
                        &bytes,
                        installation.trust(),
                        &next.target,
                    )?) != *next
                {
                    return Err("Reçu préparatoire modifié ; aucun déplacement appliqué.".into());
                }
                publish::rename_new_between(
                    &receipts,
                    std::ffi::OsStr::new(&name),
                    &destination,
                    std::ffi::OsStr::new("signed-receipt.json"),
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some((path, expected)) = &journal.prepared {
            if path.parent() != scope.root.parent() {
                return Err("Préparation extérieure au périmètre système.".into());
            }
            if path.try_exists()? {
                if identity(path)? != *expected {
                    return Err("Préparation remplacée ; contenu préservé.".into());
                }
                let payload = update_package::verify_installed_payload(
                    path,
                    &journal.receipt,
                    installation.trust(),
                    &next.target,
                )?;
                if Version::from_payload(&payload) != *next {
                    return Err("Préparation d’une autre version ; contenu conservé.".into());
                }
                publish::rename_new_between(
                    &scope.parent,
                    path.file_name().ok_or("Préparation sans nom.")?,
                    &destination,
                    std::ffi::OsStr::new("payload"),
                )?;
            }
        }
        record_once(
            &destination,
            "preparation-receipt.json",
            &serde_json::to_vec(journal)?,
        )?;
        installed::verify_all(&scope.root, package::native_target()?)?;
        Ok(())
    }

    async fn remove(scope: &Scope) -> Result<()> {
        if let Some(mut journal) = reserve_removal(scope)? {
            resume_remove(scope, &mut journal)?;
        }
        Ok(())
    }
    fn reserve_removal(scope: &Scope) -> Result<Option<Journal>> {
        if !scope.root.try_exists()? {
            println!("Installation système déjà absente.");
            return Ok(None);
        }
        check_marker(scope, None)?;
        if scope._running.is_none() || installation::mode(&scope.root)? != InstallationMode::System
        {
            return Err("Installation système ou verrou absents ; contenu conservé.".into());
        }
        let target = package::native_target()?;
        installed::verify_all(&scope.root, target)?;
        if updates::status(Some(&scope.root))?
            .operation
            .is_some_and(|operation| operation.state.pending())
        {
            return Err("Une mise à jour Rust est encore non résolue.".into());
        }
        let mut journal = new_journal(scope, Action::Remove);
        let installation =
            installed::open(&scope.root, target)?.ok_or("Installation système absente.")?;
        journal.expected = Some(installation.current().clone());
        journal.key_id = installation.trust().key_id().into();
        journal.root_identity = Some(identity(&scope.root)?);
        let directory = scope.directory(&journal)?;
        let mut paths = vec![
            "atelier".to_owned(),
            "installation.json".into(),
            "trusted-update-key.json".into(),
            "bootstrap-receipt.json".into(),
            "bootstrap-manifest.json".into(),
            "current.json".into(),
            ".package-lock".into(),
        ];
        for entry in directory.open_dir_nofollow("versions")?.entries()? {
            let id = entry?
                .file_name()
                .into_string()
                .map_err(|_| "Version système invalide.")?;
            package::verify(&scope.root.join("versions").join(&id))?;
            let slot = directory
                .open_dir_nofollow("versions")?
                .open_dir_nofollow(&id)?;
            let manifest: serde_json::Value = serde_json::from_slice(
                &CheckedFile::open_in(&slot, Path::new("atelier-package.json"))?
                    .read(1024 * 1024)?,
            )?;
            paths.push(format!("receipts/{id}.json"));
            paths.push(format!("versions/{id}/atelier-package.json"));
            for file in manifest["files"]
                .as_array()
                .ok_or("Inventaire système absent.")?
            {
                paths.push(format!(
                    "versions/{id}/{}",
                    file["path"].as_str().ok_or("Chemin système absent.")?
                ));
            }
        }
        if paths.len() > 4096 {
            return Err("Inventaire système trop volumineux.".into());
        }
        for path in paths {
            journal.files.push(Owned {
                sha256: digest(&directory, &path)?,
                path,
            });
        }
        let prefix = format!(".atelier-system-removed-{}-", scope.key);
        retained_limit(scope, &prefix)?;
        let rescue = format!("{prefix}{}", journal.id);
        scope.parent.create_dir(&rescue)?;
        scope
            .parent
            .open_dir_nofollow(&rescue)?
            .into_std_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))?;
        journal.rescue = Some(rescue);
        journal.phase = Phase::Removing;
        scope.save(&journal)?;
        reserve_marker(scope, &journal)?;
        Ok(Some(journal))
    }
    fn resume_remove(scope: &Scope, journal: &mut Journal) -> Result<()> {
        if journal.phase == Phase::Removed {
            if scope.root.try_exists()? {
                let source = scope.directory(journal)?;
                for file in &journal.files {
                    match digest(&source,&file.path) {Ok(_)=>return Err("Un nouveau contenu système est apparu après retrait ; aucun fichier modifié.".into()),Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|error|error.kind()==std::io::ErrorKind::NotFound)=>{},Err(error)=>return Err(error)}
                }
                clear_marker(scope, journal)?;
            }
            println!("Retrait système déjà enregistré.");
            return Ok(());
        }
        if journal.action != Action::Remove || journal.phase != Phase::Removing {
            return Err("Aucun retrait système récupérable.".into());
        }
        let rescue = journal
            .rescue
            .as_ref()
            .filter(|name| {
                name.as_str() == format!(".atelier-system-removed-{}-{}", scope.key, journal.id)
            })
            .ok_or("Dossier de conservation système invalide.")?;
        let destination = scope.parent.open_dir_nofollow(rescue)?;
        let source = scope.directory(journal)?;
        for file in &journal.files {
            let old = digest(&source, &file.path);
            let saved = digest(&destination, &file.path);
            match (old,saved) {
                (Ok(hash),Err(error)) if error.downcast_ref::<std::io::Error>().is_some_and(|error|error.kind()==std::io::ErrorKind::NotFound) && hash==file.sha256=>{},
                (Err(error),Ok(hash)) if error.downcast_ref::<std::io::Error>().is_some_and(|error|error.kind()==std::io::ErrorKind::NotFound) && hash==file.sha256=>{},
                _=>return Err("Un fichier système a changé ou existe aux deux emplacements ; contenu conservé.".into()),
            }
        }
        for file in &journal.files {
            if digest(&destination, &file.path).is_ok() {
                continue;
            }
            scope.directory(journal)?;
            let (from, name) = parent_file(&source, &file.path)?;
            let parent = Path::new(&file.path)
                .parent()
                .ok_or("Parent système absent.")?;
            let mut created = destination.try_clone()?;
            for part in parent.components() {
                match created.symlink_metadata(part.as_os_str()) {
                    Ok(metadata) => super::super::checked_file::require_plain(&metadata)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        created.create_dir(part.as_os_str())?
                    }
                    Err(error) => return Err(error.into()),
                }
                created = created.open_dir_nofollow(part.as_os_str())?;
            }
            let (to, target) = parent_file(&destination, &file.path)?;
            if digest(&source, &file.path)? != file.sha256 {
                return Err(
                    "Fichier système modifié pendant le retrait ; contenu conservé.".into(),
                );
            }
            publish::rename_new_between(&from, &name, &to, &target)?;
            from.try_clone()?.into_std_file().sync_all()?;
            to.try_clone()?.into_std_file().sync_all()?;
            if digest(&destination, &file.path)? != file.sha256 {
                let _ = publish::rename_new_between(&to, &target, &from, &name);
                return Err("Fichier système modifié pendant son déplacement ; copie conservée pour examen.".into());
            }
        }
        let mut parents = BTreeSet::new();
        for file in &journal.files {
            let mut parent = Path::new(&file.path).parent();
            while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
                parents.insert(path.to_owned());
                parent = path.parent();
            }
        }
        for path in parents.iter().rev() {
            let mut destination_parent = destination.try_clone()?;
            for part in path.components() {
                destination_parent = destination_parent.open_dir_nofollow(part.as_os_str())?;
            }
            destination_parent.into_std_file().sync_all()?;
            let removal = parent_file(&source, path.to_str().ok_or("Chemin système non UTF-8.")?)
                .and_then(|(directory, name)| directory.remove_dir(name).map_err(Into::into));
            match removal {
                Ok(()) => {}
                Err(error)
                    if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                        )
                    }) => {}
                Err(error) => return Err(error),
            }
        }
        journal.phase = Phase::Removed;
        journal.message="Code système retiré et conservé dans un dossier privé ; données et fichiers non suivis laissés en place.".into();
        record_once(
            &destination,
            "removal-receipt.json",
            &serde_json::to_vec(journal)?,
        )?;
        scope.save(journal)?;
        clear_marker(scope, journal)?;
        if let Err(error) = scope
            .parent
            .remove_dir(scope.root.file_name().ok_or("Nom système absent.")?)
            && error.kind() != std::io::ErrorKind::DirectoryNotEmpty
        {
            return Err(error.into());
        }
        println!(
            "Retrait système terminé ; code conservé : {}",
            scope
                .root
                .parent()
                .ok_or("Parent système absent.")?
                .join(rescue)
                .display()
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn fixture() -> tempfile::TempDir {
            let (root, _) = update_package::tests::fixture();
            let target = package::native_target().unwrap();
            let payload = root.path().join("payload");
            if target.starts_with("aarch64") {
                let path = payload.join("atelier");
                let mut image = std::fs::read(&path).unwrap();
                image[18] = 183;
                std::fs::write(path, image).unwrap();
            }
            let manifest = package::manifest(&payload, target, "0.2.0").unwrap();
            std::fs::write(
                payload.join("atelier-package.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            super::super::super::package_archive::create(&payload, &root.path().join("native.zip"))
                .unwrap();
            update_package::sign(
                &root.path().join("native.zip"),
                target,
                &root.path().join("keys"),
                &root.path().join("native.json"),
            )
            .unwrap();
            let installed = root.path().join("installed");
            installed::install(
                &root.path().join("native.zip"),
                &root.path().join("native.json"),
                &root.path().join("keys/synthetic-update-public.json"),
                &installed,
                InstallationMode::System,
                target,
            )
            .unwrap();
            std::fs::write(installed.join(".package-lock"), []).unwrap();
            let installation = installed::open(&installed, target).unwrap().unwrap();
            readable(&installation, &installation.current().active).unwrap();
            root
        }
        #[tokio::test]
        async fn interrupted_reservation_blocks_new_servers_and_recovers_without_running_code_or_opening_data()
         {
            let root = fixture();
            let path = root.path().join("installed");
            std::fs::write(root.path().join("synthetic-data"), "preserve-user-data").unwrap();
            let scope = Scope::open(&path).unwrap();
            reserve(&scope).unwrap();
            let original_marker = marker(&scope.read().unwrap().unwrap());
            drop(scope);
            assert!(super::super::super::package_maintenance::acquire(Some(&path)).is_err());
            std::fs::remove_file(path.join(MARKER)).unwrap();
            assert!(super::super::super::package_maintenance::acquire(Some(&path)).is_err());
            std::fs::write(path.join(MARKER), b"atelier-package-maintenance/1\n").unwrap();
            assert!(
                super::super::super::package_maintenance::recover_command([
                    OsString::from("--root"),
                    path.clone().into_os_string(),
                    OsString::from("--confirm")
                ])
                .is_err()
            );
            assert_eq!(
                std::fs::read(path.join(MARKER)).unwrap(),
                b"atelier-package-maintenance/1\n"
            );
            std::fs::write(path.join(MARKER), original_marker).unwrap();
            let scope = Scope::open(&path).unwrap();
            recover(&scope, scope.read().unwrap().unwrap())
                .await
                .unwrap();
            drop(scope);
            assert!(super::super::super::package_maintenance::acquire(Some(&path)).is_ok());
            assert_eq!(
                std::fs::read_to_string(root.path().join("synthetic-data")).unwrap(),
                "preserve-user-data"
            );
            assert!(!path.join(MARKER).exists());
            let scope = Scope::open(&path).unwrap();
            std::fs::write(root.path().join(scope.name()), "malformed system journal").unwrap();
            drop(scope);
            assert!(super::super::super::package_maintenance::acquire(Some(&path)).is_err());
        }
        #[tokio::test]
        async fn interrupted_removal_preserves_human_changes_and_resumes_only_the_frozen_inventory()
        {
            let root = fixture();
            let path = root.path().join("installed");
            std::fs::write(path.join("human-note"), "preserve-note").unwrap();
            std::fs::write(root.path().join("synthetic-data"), "preserve-user-data").unwrap();
            let scope = Scope::open(&path).unwrap();
            let journal = reserve_removal(&scope).unwrap().unwrap();
            let destination = scope
                .parent
                .open_dir_nofollow(journal.rescue.as_ref().unwrap())
                .unwrap();
            let source = scope.directory(&journal).unwrap();
            let first = &journal.files[0];
            let (from, name) = parent_file(&source, &first.path).unwrap();
            from.rename(&name, &destination, &name).unwrap();
            let installation = installed::open_recovery(&path, package::native_target().unwrap());
            assert!(installation.is_err()); // The removed bootstrap must not be adopted as an uninstalled application.
            let readme = journal
                .files
                .iter()
                .find(|file| file.path.ends_with("/README.txt"))
                .unwrap()
                .path
                .clone();
            let original = std::fs::read(path.join(&readme)).unwrap();
            std::fs::write(path.join(&readme), "human edit after interruption").unwrap();
            drop(scope);
            let scope = Scope::open(&path).unwrap();
            let mut resumed = scope.read().unwrap().unwrap();
            assert!(resume_remove(&scope, &mut resumed).is_err());
            assert_eq!(
                std::fs::read_to_string(path.join(&readme)).unwrap(),
                "human edit after interruption"
            );
            assert!(path.join("installation.json").exists());
            std::fs::write(path.join(&readme), original).unwrap();
            resume_remove(&scope, &mut scope.read().unwrap().unwrap()).unwrap();
            assert!(!path.join("atelier").exists());
            assert_eq!(
                std::fs::read_to_string(path.join("human-note")).unwrap(),
                "preserve-note"
            );
            assert_eq!(
                std::fs::read_to_string(root.path().join("synthetic-data")).unwrap(),
                "preserve-user-data"
            );
            resume_remove(&scope, &mut scope.read().unwrap().unwrap()).unwrap();
        }
        #[test]
        fn archive_receipts_are_never_overwritten_and_symlinked_payloads_are_not_adopted() {
            let root = tempfile::tempdir().unwrap();
            let directory = super::super::super::repository_access::absolute(root.path()).unwrap();
            record_once(&directory, "receipt", b"first").unwrap();
            assert!(record_once(&directory, "receipt", b"second").is_err());
            assert_eq!(
                std::fs::read(root.path().join("receipt")).unwrap(),
                b"first"
            );
            std::os::unix::fs::symlink("receipt", root.path().join("linked")).unwrap();
            assert!(digest(&directory, "linked").is_err());
            assert!(parent_file(&directory, "../receipt").is_err());
            directory.create_dir("source").unwrap();
            directory.create_dir("destination").unwrap();
            std::fs::write(root.path().join("source/code"), b"signed-code").unwrap();
            std::fs::write(root.path().join("destination/code"), b"human-file").unwrap();
            let source = directory.open_dir_nofollow("source").unwrap();
            let destination = directory.open_dir_nofollow("destination").unwrap();
            assert!(
                publish::rename_new_between(
                    &source,
                    std::ffi::OsStr::new("code"),
                    &destination,
                    std::ffi::OsStr::new("code")
                )
                .is_err()
            );
            assert_eq!(
                std::fs::read(root.path().join("source/code")).unwrap(),
                b"signed-code"
            );
            assert_eq!(
                std::fs::read(root.path().join("destination/code")).unwrap(),
                b"human-file"
            );
        }
        #[test]
        fn atomic_system_staging_keeps_the_old_selection_and_recovers_an_orphan_receipt_offline() {
            let root = fixture();
            let path = root.path().join("installed");
            let target = package::native_target().unwrap();
            let installation = installed::open(&path, target).unwrap().unwrap();
            let before = installation.current().clone();
            let prepare = |version: &str, name: &str| {
                let payload = root.path().join("payload");
                let manifest = package::manifest(&payload, target, version).unwrap();
                std::fs::write(
                    payload.join("atelier-package.json"),
                    serde_json::to_vec(&manifest).unwrap(),
                )
                .unwrap();
                let archive = root.path().join(format!("{name}.zip"));
                let signature = root.path().join(format!("{name}.json"));
                super::super::super::package_archive::create(&payload, &archive).unwrap();
                update_package::sign(&archive, target, &root.path().join("keys"), &signature)
                    .unwrap();
                update_package::prepare(
                    &archive,
                    &signature,
                    installation.trust(),
                    target,
                    "0.2.0",
                    root.path(),
                )
                .unwrap()
            };
            let prepared = prepare("0.3.0", "future");
            let temporary = prepared.package().path().to_owned();
            let next = installation.stage_system_update(prepared).unwrap();
            assert!(!temporary.exists());
            assert!(installation.slot_path(&next).unwrap().exists());
            assert!(installed::open(&path, target).unwrap().unwrap().current() == &before);
            let interrupted = prepare("0.4.0", "interrupted");
            let next = Version::from_payload(interrupted.payload());
            let scope = Scope::open(&path).unwrap();
            let mut journal = new_journal(&scope, Action::Apply);
            journal.expected = Some(before.clone());
            journal.key_id = installation.trust().key_id().into();
            journal.root_identity = Some(identity(&path).unwrap());
            journal.next = Some(next.clone());
            journal.receipt = interrupted.receipt().to_owned();
            journal.prepared = Some((
                interrupted.package().path().to_owned(),
                identity(interrupted.package().path()).unwrap(),
            ));
            package::write(
                &path.join("receipts").join(format!("{}.json", next.id)),
                interrupted.receipt(),
            )
            .unwrap();
            let _retained = interrupted.keep_package();
            scope.save(&journal).unwrap();
            preserve_preparation(&scope, &mut journal, &installation, &next).unwrap();
            assert!(
                !path
                    .join("receipts")
                    .join(format!("{}.json", next.id))
                    .exists()
            );
            installed::verify_all(&path, target).unwrap();
            assert!(installed::open(&path, target).unwrap().unwrap().current() == &before);
            let rescue = root.path().join(journal.rescue.unwrap());
            assert!(rescue.join("signed-receipt.json").is_file());
            assert!(rescue.join("payload/atelier").is_file());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsing_requires_explicit_absolute_scope_and_confirmation_before_any_io() {
        for args in [
            vec![],
            vec!["apply"],
            vec!["remove", "--root", "relative", "--confirm"],
            vec!["recover", "--root", "relative"],
            vec!["remove", "--confirm", "--confirm"],
        ] {
            assert!(Input::parse(args.into_iter().map(OsString::from)).is_err());
        }
        let root = std::env::temp_dir();
        assert!(
            Input::parse([
                OsString::from("remove"),
                OsString::from("--root"),
                root.into_os_string(),
                OsString::from("--confirm")
            ])
            .is_ok()
        );
    }
}
