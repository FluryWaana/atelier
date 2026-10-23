use super::data_lock::DataLock;
use crate::Result;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

pub struct PrototypeDirectory {
    pub path: PathBuf,
    _lock: DataLock,
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
        let lock = DataLock::acquire(&path)?;
        Ok(Self { path, _lock: lock })
    }
}
