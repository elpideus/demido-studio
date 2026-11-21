//! Where Demido keeps its files.
//!
//! One folder holds everything: the system's data folder for the user (`~/.local/share/demido` on Linux),
//! or whatever `DEMIDO_HOME` names. Nothing is written anywhere else.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    /// `DEMIDO_HOME` if set, otherwise the system's data folder for this user.
    pub fn resolve() -> Result<Self> {
        if let Some(home) = std::env::var_os("DEMIDO_HOME").filter(|h| !h.is_empty()) {
            return Ok(Self::at(PathBuf::from(home)));
        }
        directories::ProjectDirs::from("", "", "demido")
            .map(|d| Self::at(d.data_dir().to_path_buf()))
            .ok_or_else(|| Error::Other("Could not find your home folder. Set DEMIDO_HOME to a folder Demido may use.".into()))
    }

    pub fn at(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// Makes the folder if it is not there yet.
    pub fn ensure(&self) -> Result<&Self> {
        std::fs::create_dir_all(&self.home)?;
        Ok(self)
    }

    pub fn db(&self) -> PathBuf {
        self.home.join("demido.db")
    }
    /// The key every request needs. Readable by the user only.
    pub fn key(&self) -> PathBuf {
        self.home.join("key")
    }
    /// Written while a server runs: where it listens. The command line reads it to find the server.
    pub fn running(&self) -> PathBuf {
        self.home.join("running.json")
    }
    pub fn home(&self) -> &Path {
        &self.home
    }
}
