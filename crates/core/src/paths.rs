//! Well-known folders.
//!
//! Program files (the app, runtimes, Python, Node) live in the install folder. Everything a
//! person creates or downloads later (chats, models, skills) lives in the per-user data folder,
//! so a machine-wide install never needs write access to Program Files after setup.

use std::path::PathBuf;

use crate::brand::APP_NAME;
use crate::manifest::InstallScope;

/// Environment variable that relocates the user data folder (used for tests and dev profiles).
pub const DATA_DIR_ENV: &str = "DEMIDO_DATA_DIR";
/// Environment variable that points the app at an install folder other than its own
/// (used in development, where the binary lives in `target/`).
pub const INSTALL_DIR_ENV: &str = "DEMIDO_INSTALL_DIR";

/// Default install folder for a scope. Never in or around [`user_data_dir`]: setup refuses an
/// install folder that overlaps the person's data.
pub fn default_install_dir(scope: InstallScope) -> PathBuf {
    #[cfg(windows)]
    {
        match scope {
            InstallScope::User => dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public"))
                .join("Programs")
                .join(APP_NAME),
            InstallScope::Machine => std::env::var_os("ProgramW6432")
                .or_else(|| std::env::var_os("ProgramFiles"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("C:\\Program Files"))
                .join(APP_NAME),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match scope {
            InstallScope::User => home().join("Applications").join(APP_NAME),
            InstallScope::Machine => PathBuf::from("/Applications").join(APP_NAME),
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        match scope {
            // The per-user counterpart of /opt, beside ~/.local/share rather than in it.
            InstallScope::User => home().join(".local/opt").join(crate::brand::APP_SLUG),
            InstallScope::Machine => PathBuf::from("/opt").join(crate::brand::APP_SLUG),
        }
    }
}

/// Per-user data folder: chats, settings, skills, downloaded models.
pub fn user_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        dirs::data_local_dir().unwrap_or_else(home).join(APP_NAME)
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support").join(APP_NAME)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs::data_dir()
            .unwrap_or_else(|| home().join(".local/share"))
            .join(crate::brand::APP_SLUG)
    }
}

/// Where the installer saves the starter model. A machine-wide install uses a folder every user
/// can read; a per-user install keeps models with the rest of the user's data.
pub fn starter_models_dir(scope: InstallScope) -> PathBuf {
    match scope {
        InstallScope::User => user_data_dir().join("models"),
        InstallScope::Machine => {
            #[cfg(windows)]
            {
                std::env::var_os("ProgramData")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
                    .join(APP_NAME)
                    .join("models")
            }
            #[cfg(target_os = "macos")]
            {
                PathBuf::from("/Users/Shared").join(APP_NAME).join("models")
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                PathBuf::from("/var/lib").join(crate::brand::APP_SLUG).join("models")
            }
        }
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_differ() {
        assert_ne!(
            default_install_dir(InstallScope::User),
            default_install_dir(InstallScope::Machine)
        );
    }

    #[test]
    fn install_folders_stay_out_of_the_user_data_folder() {
        let data = user_data_dir();
        for scope in [InstallScope::User, InstallScope::Machine] {
            let dir = default_install_dir(scope);
            assert!(
                !dir.starts_with(&data) && !data.starts_with(&dir),
                "{} overlaps {}",
                dir.display(),
                data.display()
            );
        }
    }

    #[test]
    fn user_models_live_in_user_data() {
        assert!(starter_models_dir(InstallScope::User).starts_with(user_data_dir()));
    }
}
