//! Names that appear on disk, in the registry and in shortcuts. Change them here only.

/// Human-facing product name.
pub const APP_NAME: &str = "Demido Studio";
/// Publisher shown in "Installed apps".
pub const PUBLISHER: &str = "Demido";
/// Folder-safe product name used for data directories on Linux.
pub const APP_SLUG: &str = "demido-studio";
/// Stem of the app executable (without platform suffix).
pub const STUDIO_BIN: &str = "demido-studio";
/// Stem of the uninstaller copied into the install folder.
pub const UNINSTALLER_BIN: &str = "uninstall";
/// Registry key under `...\CurrentVersion\Uninstall` on Windows.
pub const UNINSTALL_KEY: &str = "DemidoStudio";
/// Bundle identifier of the app.
pub const APP_ID: &str = "app.demido.studio";
/// Version of this build, shared by the installer and the app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
