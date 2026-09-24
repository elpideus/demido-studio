use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

impl Arch {
    pub fn current() -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            Arch::X86_64
        }
    }
}

/// Appends the platform's executable suffix (`.exe` on Windows) to a binary stem.
pub fn exe(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}
