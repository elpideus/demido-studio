//! The environment of programs started from inside an AppImage.
//!
//! An AppImage's launcher (AppRun) and its hooks point the environment at the AppImage's own files
//! before the app starts: `PYTHONHOME` at its `usr/`, its folders in front of `PATH`,
//! `LD_LIBRARY_PATH`, `PYTHONPATH` and the like, GTK and GStreamer at its copies. The app needs
//! that; a program from the system does not, and breaks on it: a Python finds no standard library
//! under that `PYTHONHOME`, a program loads the AppImage's libraries before its own. So every
//! program the app or setup starts from outside the AppImage gets the environment it would have
//! outside ([`use_host_env`]): the launcher only puts its folders in front of what was there, so
//! taking them out gives that back, and what the launcher sets outright is left out. Programs
//! inside the AppImage keep its environment. Outside an AppImage nothing changes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Lists of folders the launcher and its hooks put the AppImage's folders in front of.
const LISTS: [&str; 13] = [
    "PATH",
    "LD_LIBRARY_PATH",
    "PYTHONPATH",
    "XDG_DATA_DIRS",
    "PERLLIB",
    "GSETTINGS_SCHEMA_DIR",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH_1_0",
    "GI_TYPELIB_PATH",
    "GTK_PATH",
    "GIO_EXTRA_MODULES",
];

/// Paths the launcher and its hooks set to the AppImage's files outright.
const PATHS: [&str; 7] = [
    "PYTHONHOME",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GST_PLUGIN_SCANNER_1_0",
    "GST_PTP_HELPER_1_0",
];

/// What the GTK and GStreamer hooks set whatever was there, by the value they set: a program
/// from the system gets them only when someone else set them otherwise.
const HOOKED: [(&str, &str); 3] = [
    ("GDK_BACKEND", "x11"),
    ("GTK_THEME", "Adwaita:"),
    ("GST_REGISTRY_REUSE_PLUGIN_SCANNER", "no"),
];

/// The AppImage runtime's own variables, which say "this is the AppImage".
const RUNTIME: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// The AppImage's folder while running from one: `APPDIR`, which its runtime sets with `APPIMAGE`.
pub fn appdir() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")?;
    std::env::var_os("APPDIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
}

/// Gives `cmd` the environment its program would get outside the AppImage this process runs
/// from, unless the program is inside the AppImage. Outside an AppImage, nothing.
pub fn use_host_env(cmd: &mut std::process::Command) {
    let Some(appdir) = appdir() else {
        return;
    };
    if Path::new(cmd.get_program()).starts_with(&appdir) {
        return;
    }
    for (name, value) in host_env_from(&appdir, |name| std::env::var_os(name)) {
        match value {
            Some(value) => cmd.env(name, value),
            None => cmd.env_remove(name),
        };
    }
}

/// The changes [`use_host_env`] makes, for a command built some other way (a pseudo-terminal's):
/// `(name, None)` removes the variable, `(name, Some(value))` sets it. Empty outside an AppImage.
pub fn host_env() -> Vec<(&'static str, Option<OsString>)> {
    match appdir() {
        Some(appdir) => host_env_from(&appdir, |name| std::env::var_os(name)),
        None => Vec::new(),
    }
}

/// [`host_env`] for an AppImage in `appdir`, with the environment read through `get`.
fn host_env_from(appdir: &Path, get: impl Fn(&str) -> Option<OsString>) -> Vec<(&'static str, Option<OsString>)> {
    let inside = |p: &OsStr| Path::new(p).starts_with(appdir);
    let mut changes = Vec::new();
    for name in LISTS {
        let Some(value) = get(name) else {
            continue;
        };
        let kept: Vec<PathBuf> = std::env::split_paths(&value)
            .filter(|p| !p.as_os_str().is_empty() && !inside(p.as_os_str()))
            .collect();
        if kept.is_empty() {
            changes.push((name, None));
        } else if let Ok(joined) = std::env::join_paths(kept)
            && joined != value
        {
            changes.push((name, Some(joined)));
        }
    }
    for name in PATHS {
        if get(name).is_some_and(|v| inside(&v)) {
            changes.push((name, None));
        }
    }
    for (name, set) in HOOKED {
        if get(name).is_some_and(|v| v.to_string_lossy().starts_with(set)) {
            changes.push((name, None));
        }
    }
    for name in RUNTIME {
        if get(name).is_some() {
            changes.push((name, None));
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The environment AppRun and the GTK and GStreamer hooks leave for an AppImage mounted at
    /// /tmp/.mount_Demido, over a desktop session's.
    fn session() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("APPIMAGE", "/home/x/Apps/Demido-Studio.AppImage"),
            ("APPDIR", "/tmp/.mount_Demido"),
            ("ARGV0", "./Demido-Studio.AppImage"),
            ("OWD", "/home/x"),
            ("PYTHONHOME", "/tmp/.mount_Demido/usr/"),
            (
                "PATH",
                "/tmp/.mount_Demido/usr/bin/:/tmp/.mount_Demido/usr/sbin/:/tmp/.mount_Demido/usr/games/:/tmp/.mount_Demido/bin/:/tmp/.mount_Demido/sbin/:/home/x/.local/bin:/usr/bin",
            ),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_Demido/usr/lib/:/tmp/.mount_Demido/usr/lib/x86_64-linux-gnu/:/tmp/.mount_Demido/lib/:",
            ),
            ("PYTHONPATH", "/tmp/.mount_Demido/usr/share/pyshared/:"),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_Demido/usr/share:/usr/share:/tmp/.mount_Demido/usr/share/:/home/x/.local/share/flatpak/exports/share:/usr/local/share:/usr/share",
            ),
            ("GTK_PATH", "/tmp/.mount_Demido//usr/lib/gtk-3.0"),
            ("GTK_EXE_PREFIX", "/tmp/.mount_Demido//usr"),
            ("GDK_BACKEND", "x11"),
            ("GTK_THEME", "Adwaita:dark"),
            (
                "GST_PLUGIN_SYSTEM_PATH_1_0",
                "/tmp/.mount_Demido/usr/lib/gstreamer-1.0:",
            ),
            ("HOME", "/home/x"),
            ("WAYLAND_DISPLAY", "wayland-0"),
        ])
    }

    fn changes(env: &HashMap<&'static str, &'static str>) -> HashMap<&'static str, Option<String>> {
        host_env_from(Path::new("/tmp/.mount_Demido"), |name| {
            env.get(name).map(OsString::from)
        })
        .into_iter()
        .map(|(name, value)| (name, value.map(|v| v.to_string_lossy().into_owned())))
        .collect()
    }

    #[test]
    fn programs_from_the_system_get_the_sessions_environment_back() {
        let got = changes(&session());
        assert_eq!(got["PATH"].as_deref(), Some("/home/x/.local/bin:/usr/bin"));
        assert_eq!(
            got["XDG_DATA_DIRS"].as_deref(),
            Some("/usr/share:/home/x/.local/share/flatpak/exports/share:/usr/local/share:/usr/share")
        );
        // Set by the launcher alone: gone.
        for name in [
            "PYTHONHOME",
            "LD_LIBRARY_PATH",
            "PYTHONPATH",
            "GTK_PATH",
            "GTK_EXE_PREFIX",
            "GDK_BACKEND",
            "GTK_THEME",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "APPDIR",
            "APPIMAGE",
            "ARGV0",
            "OWD",
        ] {
            assert_eq!(got.get(name), Some(&None), "{name}");
        }
        // The session's own are left alone.
        assert!(!got.contains_key("HOME") && !got.contains_key("WAYLAND_DISPLAY"));
    }

    #[test]
    fn what_the_session_had_before_the_launcher_stays() {
        let mut env = session();
        env.insert(
            "LD_LIBRARY_PATH",
            "/tmp/.mount_Demido/usr/lib/:/tmp/.mount_Demido/lib/:/opt/rocm/lib",
        );
        env.insert("GDK_BACKEND", "wayland");
        env.insert("GTK_THEME", "Breeze");
        let got = changes(&env);
        assert_eq!(got["LD_LIBRARY_PATH"].as_deref(), Some("/opt/rocm/lib"));
        assert!(!got.contains_key("GDK_BACKEND") && !got.contains_key("GTK_THEME"));
    }

    #[test]
    fn nothing_changes_outside_an_appimage() {
        // `use_host_env` and `host_env` only look when APPIMAGE and APPDIR are set; the tests run
        // outside one.
        if std::env::var_os("APPIMAGE").is_none() {
            assert!(host_env().is_empty());
            let mut cmd = std::process::Command::new("true");
            use_host_env(&mut cmd);
            assert_eq!(cmd.get_envs().count(), 0);
        }
    }
}
