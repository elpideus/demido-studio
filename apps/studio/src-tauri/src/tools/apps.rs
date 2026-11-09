//! `open_app`: opens an app installed on the computer in a window of its own, as the app menu
//! does. Run in a command's terminal instead, a program with a window keeps the command waiting
//! until it closes; and the command-line program of the same name, when there is one, is not the
//! app at all.
//!
//! On Linux the apps are the menu's (their .desktop entries), opened through GIO with the
//! display's startup notification, so the new window comes to the front.

// The search below opens apps on Linux only (`platform`); its tests run everywhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use serde_json::{Value, json};

use super::{ToolContext, ToolOutput, require_str};

/// Whether this system can open apps.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "The app's name as the app menu shows it (\"Shelly\", \"Firefox\"), or its program's"}
        },
        "required": ["name"]
    })
}

pub async fn open(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let query = require_str(args, "name")?.to_string();
    let name = platform::open(&ctx.state.app, query).await?;
    Ok(ToolOutput {
        ok: true,
        content: json!({
            "opened": name,
            "note": "It opens in a window of its own and runs on its own, until the user closes it.",
        })
        .to_string(),
        display: json!({"kind": "app", "name": name}),
    })
}

/// An installed app, as the search sees it.
#[derive(Clone, Debug, Default)]
pub struct App {
    /// The name the menu shows, then any other it goes by (the untranslated one).
    pub names: Vec<String>,
    /// The program it runs and its desktop id, whole and by its last part
    /// (`org.gnome.Nautilus`, `nautilus`).
    pub programs: Vec<String>,
    /// What kind of app it is ("Web Browser") and its keywords.
    pub about: Vec<String>,
}

/// Programs that start other programs, which say nothing of the app (`flatpak run …`).
const LAUNCHERS: &[&str] = &[
    "flatpak",
    "snap",
    "env",
    "sh",
    "bash",
    "python",
    "python3",
    "java",
    "wine",
    "mono",
    "dotnet",
    "electron",
    "node",
    "gtk-launch",
    "gapplication",
    "dbus-send",
    "xdg-open",
    "kioclient",
    "kioclient5",
    "kde-open",
    "pkexec",
    "sudo",
    "steam",
];

/// The app `query` names among `apps`: the one that matches it best, or, when several match as
/// well, an error naming them so the right one can be asked for.
pub fn pick(apps: &[App], query: &str) -> Result<usize, String> {
    let shown = query.trim();
    let mut scored: Vec<(u32, usize)> = apps
        .iter()
        .enumerate()
        .map(|(i, app)| (score(app, shown), i))
        .filter(|&(score, _)| score > 0)
        .collect();
    // Best first, and in the menu's order among equals.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let Some(&(best, first)) = scored.first() else {
        return Err(format!(
            "No app installed on this computer goes by “{shown}”. A command-line program runs with run_command."
        ));
    };
    let name = |i: usize| apps[i].names.first().cloned().unwrap_or_default();
    let mut tied: Vec<String> = Vec::new();
    for &(score, i) in &scored {
        if score == best && !tied.contains(&name(i)) {
            tied.push(name(i));
        }
    }
    // The same app twice (installed natively and as a Flatpak, say) is no question.
    if tied.len() == 1 {
        return Ok(first);
    }
    tied.truncate(10);
    Err(format!(
        "Several apps go by “{shown}”: {}. Open one by its name, or ask the user which they meant.",
        tied.join(", ")
    ))
}

/// How well `query` names `app`, from 0 (not at all) to 100 (its very name).
fn score(app: &App, query: &str) -> u32 {
    let query = query.trim().to_lowercase();
    let (squashed, words) = (squash(&query), words(&query));
    if squashed.is_empty() {
        return 0;
    }
    let names: Vec<String> = app.names.iter().map(|n| n.to_lowercase()).collect();
    let about: Vec<String> = app.about.iter().map(|a| a.to_lowercase()).collect();
    let every_word_starts = |of: &[String]| {
        let theirs: Vec<&str> = of
            .iter()
            .flat_map(|s| s.split(|c: char| !c.is_alphanumeric()))
            .collect();
        words.iter().all(|w| theirs.iter().any(|t| t.starts_with(w.as_str())))
    };
    if names.contains(&query) {
        100
    } else if app.programs.iter().any(|p| squash(p) == squashed) {
        90
    } else if names.iter().any(|n| {
        n.strip_prefix(query.as_str())
            .is_some_and(|rest| rest.starts_with(|c: char| !c.is_alphanumeric()))
    }) {
        80
    } else if every_word_starts(&names) {
        70
    } else if squashed.chars().count() >= 3 && names.iter().any(|n| squash(n).contains(&squashed)) {
        60
    } else if about.iter().any(|a| squash(a) == squashed) {
        50
    } else if every_word_starts(&about) {
        40
    } else {
        0
    }
}

/// Letters and digits only: "Shelly UI" and "shelly-ui" are the same.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(target_os = "linux")]
mod platform {
    use gtk::gio::{self, prelude::*};

    use super::{App, LAUNCHERS, pick};

    /// Opens the app on GTK's thread, where the display's launch context lives.
    pub async fn open(handle: &tauri::AppHandle, query: String) -> Result<String, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .run_on_main_thread(move || {
                let display = gtk::gdk::Display::default().and_then(|d| d.app_launch_context());
                let context = display.map_or_else(gio::AppLaunchContext::new, |c| c.upcast());
                let _ = tx.send(open_with(&query, &context));
            })
            .map_err(|e| e.to_string())?;
        rx.await.map_err(|_| "The app could not be opened.".to_string())?
    }

    /// Finds the app the menu lists under `query` and opens it with `context`. Returns its name.
    pub fn open_with(query: &str, context: &gio::AppLaunchContext) -> Result<String, String> {
        let infos: Vec<gio::DesktopAppInfo> = gio::AppInfo::all()
            .into_iter()
            .filter(|a| a.should_show())
            .filter_map(|a| a.downcast::<gio::DesktopAppInfo>().ok())
            .collect();
        let apps: Vec<App> = infos.iter().map(describe).collect();
        let found = pick(&apps, query)?;
        let (info, name) = (&infos[found], apps[found].names[0].clone());
        // The app gets the person's environment, not what Demido set for itself or brought
        // from its AppImage.
        for key in demido_core::platform::set_for_webview() {
            context.unsetenv(key);
        }
        for (key, value) in demido_core::appimage::host_env() {
            match value {
                Some(value) => context.setenv(key, value),
                None => context.unsetenv(key),
            }
        }
        info.launch(&[], Some(context))
            .map_err(|e| format!("{name} could not be opened: {}", e.message()))?;
        tracing::info!(app = %info.id().unwrap_or_default(), "opened an app");
        Ok(name)
    }

    fn describe(info: &gio::DesktopAppInfo) -> App {
        let mut names = vec![info.name().to_string()];
        // The untranslated name, for the English one in a menu in another language.
        if let Some(name) = info.string("Name").map(|n| n.to_string())
            && !names.contains(&name)
        {
            names.push(name);
        }
        let mut programs = Vec::new();
        if let Some(program) = info.executable().file_name().and_then(|p| p.to_str())
            && !program.is_empty()
            && !LAUNCHERS.contains(&program)
        {
            programs.push(program.to_string());
        }
        if let Some(id) = info.id() {
            let stem = id.strip_suffix(".desktop").unwrap_or(&id);
            programs.push(stem.to_string());
            if let Some((_, last)) = stem.rsplit_once('.') {
                programs.push(last.to_string());
            }
        }
        let mut about: Vec<String> = info.generic_name().map(|g| g.to_string()).into_iter().collect();
        about.extend(info.keywords().iter().map(|k| k.to_string()));
        App { names, programs, about }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    pub async fn open(_handle: &tauri::AppHandle, _query: String) -> Result<String, String> {
        Err("Apps cannot be opened this way on this system; run the app's program with run_command.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(names: &[&str], programs: &[&str], about: &[&str]) -> App {
        let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        App {
            names: owned(names),
            programs: owned(programs),
            about: owned(about),
        }
    }

    fn menu() -> Vec<App> {
        vec![
            app(
                &["Shelly"],
                &["shelly-ui", "com.shellyorg.shelly", "shelly"],
                &["program", "software", "store", "package"],
            ),
            app(&["Firefox"], &["firefox", "firefox"], &["Web Browser", "internet"]),
            app(&["Firefox"], &["org.mozilla.firefox", "firefox"], &["Web Browser"]),
            app(
                &["Files", "Files"],
                &["nautilus", "org.gnome.Nautilus", "Nautilus"],
                &["File Manager"],
            ),
            app(&["Visual Studio Code"], &["code", "code"], &["Text Editor"]),
            app(&["Konsole"], &["konsole", "org.kde.konsole", "konsole"], &["Terminal"]),
            app(&["Alacritty"], &["alacritty", "Alacritty"], &["Terminal"]),
            app(
                &["Système", "System Monitor"],
                &["gnome-system-monitor"],
                &["Task Manager"],
            ),
        ]
    }

    fn opens(query: &str) -> String {
        let apps = menu();
        let i = pick(&apps, query).unwrap_or_else(|e| panic!("{query}: {e}"));
        apps[i].names[0].clone()
    }

    #[test]
    fn apps_are_found_by_name_program_or_kind() {
        assert_eq!(opens("Shelly"), "Shelly");
        assert_eq!(opens("shelly"), "Shelly");
        assert_eq!(opens("shelly-ui"), "Shelly");
        assert_eq!(opens("Shelly UI"), "Shelly");
        assert_eq!(opens("nautilus"), "Files");
        assert_eq!(opens("visual studio"), "Visual Studio Code");
        assert_eq!(opens("code"), "Visual Studio Code");
        assert_eq!(opens("  KONSOLE "), "Konsole");
        assert_eq!(opens("web browser"), "Firefox");
        // The untranslated name of an app in a menu in another language.
        assert_eq!(opens("system monitor"), "Système");
    }

    #[test]
    fn the_same_app_twice_is_opened_and_different_ones_are_asked_about() {
        // Firefox natively and as a Flatpak: the first in the menu.
        let apps = menu();
        assert_eq!(pick(&apps, "firefox"), Ok(1));
        let several = pick(&apps, "terminal").unwrap_err();
        assert!(several.contains("Konsole, Alacritty"), "{several}");
    }

    #[test]
    fn nothing_matching_says_so() {
        let error = pick(&menu(), "photoshop").unwrap_err();
        assert!(
            error.contains("“photoshop”") && error.contains("run_command"),
            "{error}"
        );
        assert!(pick(&menu(), "  ").is_err());
        // Two letters are too few to find inside a name.
        assert!(pick(&menu(), "ir").is_err());
    }
}
