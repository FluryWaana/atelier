use crate::Result;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub struct PrototypeDirectory {
    pub path: PathBuf,
    _lock: File,
}

impl PrototypeDirectory {
    pub fn open(path: &Path) -> Result<Self> {
        fs::create_dir_all(path)?;
        let path = fs::canonicalize(path)?;
        let marker = path.join(".atelier-rust-prototype");
        if !marker.exists() {
            if fs::read_dir(&path)?.next().is_some() {
                return Err("Le prototype exige un dossier vide ; les données Node sont interdites avant qualification M02.".into());
            }
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join(".instance-lock"))?;
        // shortcut: N0 does not honor this lock; qualify the shared N1/Rust protocol in M02.
        lock.try_lock()
            .map_err(|_| "Ce dossier est déjà ouvert par une instance Rust.")?;
        Ok(Self { path, _lock: lock })
    }
}
