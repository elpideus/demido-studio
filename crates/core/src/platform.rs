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

/// The variables [`prepare_webview`] set for this process: they are for the webview, so the
/// commands the app runs for the person go without them.
static SET_FOR_WEBVIEW: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();

/// Works around WebKitGTK on NVIDIA's driver under Wayland, where the window ends the app the
/// moment it is shown ("Error 71 (Protocol error) dispatching to Wayland display"): the driver's
/// explicit sync, which WebKitGTK's renderer trips over, is turned off for this process. The
/// renderer keeps the GPU, unlike WEBKIT_DISABLE_DMABUF_RENDERER=1, the other way out. Only
/// there, and never over the person's own choice: with either variable set, nothing changes.
///
/// Call it first thing in `main`, before any other thread starts.
pub fn prepare_webview() {
    #[cfg(target_os = "linux")]
    {
        let set = |name: &str| std::env::var_os(name).is_some();
        if needs_explicit_sync_off(
            set("WAYLAND_DISPLAY"),
            std::env::var("GDK_BACKEND").ok().as_deref(),
            std::path::Path::new("/sys/module/nvidia_drm").exists(),
            set("__NV_DISABLE_EXPLICIT_SYNC") || set("WEBKIT_DISABLE_DMABUF_RENDERER"),
        ) {
            // SAFETY: called at the start of `main`, before any other thread exists to read the
            // environment at the same time.
            unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
            let _ = SET_FOR_WEBVIEW.set(vec!["__NV_DISABLE_EXPLICIT_SYNC"]);
        }
    }
}

/// What [`prepare_webview`] set.
pub fn set_for_webview() -> &'static [&'static str] {
    SET_FOR_WEBVIEW.get().map_or(&[], Vec::as_slice)
}

/// A Wayland session (GTK uses it unless GDK_BACKEND asks for X11 first), NVIDIA's driver
/// loaded, and neither workaround chosen already.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn needs_explicit_sync_off(wayland: bool, gdk_backend: Option<&str>, nvidia: bool, chosen: bool) -> bool {
    let gtk_on_wayland = wayland && !gdk_backend.is_some_and(|b| b.trim_start().starts_with("x11"));
    gtk_on_wayland && nvidia && !chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nvidia_under_wayland_gets_the_webview_workaround() {
        assert!(needs_explicit_sync_off(true, None, true, false));
        assert!(needs_explicit_sync_off(true, Some("wayland,x11"), true, false));
        // X11, another GPU's driver, or a choice the person made already.
        assert!(!needs_explicit_sync_off(false, None, true, false));
        assert!(!needs_explicit_sync_off(true, Some("x11"), true, false));
        assert!(!needs_explicit_sync_off(true, None, false, false));
        assert!(!needs_explicit_sync_off(true, None, true, true));
    }
}
