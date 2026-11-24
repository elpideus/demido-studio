//! Demido Studio's desktop shell (DECISIONS C1 to C3 and F1 to F8; research note 35): a window on the core this
//! computer runs, the core started when none is; a tray; notices on the desktop for what schedules did (J6).
//!
//! It holds no product logic and links nothing of the core. It speaks to the core through the `demido` program
//! beside it (`open --print` for a sign-in link, `serve` to start one, `events --follow` for its news, `stop`),
//! and the window shows the core's own page, as a browser would. No command of the shell can be called from
//! that page: no capability grants the core's address anything (F3).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{NewWindowResponse, PermissionKind, PermissionResponse};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const MAIN: &str = "main";
/// How long a core that was just started has to answer.
const CORE_STARTS_WITHIN: Duration = Duration::from_secs(40);
/// How long to wait before listening again when the core's news stopped (it was restarted, or stopped).
const LISTEN_AGAIN_AFTER: Duration = Duration::from_secs(5);

/// Set on every page of the window, so that the page knows it is in the shell: the shell shows the desktop
/// notices (the page then does not offer its own), and WebRTC is off (IDEAS 142).
const IN_THE_SHELL: &str = r#"Object.defineProperty(window, "__DEMIDO_DESKTOP__", { value: Object.freeze({ notices: true, webrtc: "off" }), configurable: false });"#;

struct Shell {
    /// The `demido` program.
    core: PathBuf,
    /// `host:port` of the core the window shows, once it is joined.
    address: Mutex<Option<String>>,
    /// The session each notice opens when it is clicked.
    notices: Mutex<HashMap<i32, String>>,
    next_notice: AtomicI32,
}

/// The `demido` program: `DEMIDO_CORE` when set (a development build), else the one beside the shell (how an
/// installer lays them out), else whatever `demido` the system finds.
fn core_program() -> PathBuf {
    let name = if cfg!(windows) { "demido.exe" } else { "demido" };
    if let Some(given) = std::env::var_os("DEMIDO_CORE") {
        return given.into();
    }
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(name))
}

/// The program, with no console window of its own on Windows and nothing to read.
fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A one-time sign-in link to the running core, or none when no core answers.
fn sign_in_link(core: &Path) -> Option<Url> {
    let out = command(core).args(["open", "--print"]).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).lines().find(|l| l.starts_with("http://") || l.starts_with("https://")).and_then(|l| Url::parse(l.trim()).ok())
}

/// Starts a core that outlives the shell: schedules and bots go on when the window is closed (F5).
fn start_core(core: &Path, ui: Option<PathBuf>) -> std::io::Result<()> {
    let mut c = command(core);
    c.arg("serve");
    if let Some(ui) = ui {
        c.arg("--ui-dir").arg(ui);
    }
    c.stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a signal to the shell's is not one to the core.
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = c.spawn()?;
    // Waited for, so that it leaves nothing behind if it ends while the shell runs.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn host_port(url: &Url) -> String {
    format!("{}:{}", url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or(0))
}

/// Whether the window may go to this address: the shell's own page, and the core's pages, and nothing else.
/// Nothing is opened elsewhere from here: a page a model gave must not be able to send the person's own
/// browser somewhere (F4).
fn may_show(shell: &Shell, url: &Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        "http" | "https" if url.host_str() == Some("tauri.localhost") => true,
        "http" | "https" => shell.address.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(host_port(url).as_str()),
        // A file the core's page made for itself (a download, a preview): its address names the core's.
        "blob" => Url::parse(url.path()).is_ok_and(|inner| may_show(shell, &inner)),
        _ => false,
    }
}

/// Says something on the shell's own page, while there is no core to show.
fn say(app: &AppHandle, text: &str) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.eval(format!("window.say && window.say({})", serde_json::to_string(text).unwrap_or_default()));
    }
}

/// Joins the core that runs, or starts one and joins it, then listens to its news. On a thread of its own.
fn join_the_core(app: AppHandle, shell: Arc<Shell>) {
    std::thread::spawn(move || {
        let link = match sign_in_link(&shell.core) {
            Some(link) => Ok(link),
            None => {
                say(&app, "Starting Demido…");
                let ui = app.path().resource_dir().ok().map(|d| d.join("ui")).filter(|d| d.join("index.html").is_file());
                match start_core(&shell.core, ui) {
                    Err(e) => Err(format!("Demido's core could not be started ({}): {e}", shell.core.display())),
                    Ok(()) => {
                        let started = Instant::now();
                        loop {
                            std::thread::sleep(Duration::from_millis(400));
                            if let Some(link) = sign_in_link(&shell.core) {
                                break Ok(link);
                            }
                            if started.elapsed() > CORE_STARTS_WITHIN {
                                break Err("Demido's core was started and did not answer within 40 seconds. In a terminal, `demido serve` shows why.".to_string());
                            }
                        }
                    }
                }
            }
        };
        match link {
            Ok(url) => {
                *shell.address.lock().unwrap_or_else(|e| e.into_inner()) = Some(host_port(&url));
                if let Some(w) = app.get_webview_window(MAIN) {
                    let _ = w.navigate(url);
                }
                follow_the_news(&app, &shell);
            }
            Err(why) => say(&app, &why),
        }
    });
}

/// The core's news, as `demido events --follow` prints it, for the notices. Listens again when it stops.
fn follow_the_news(app: &AppHandle, shell: &Arc<Shell>) {
    loop {
        if let Ok(mut child) = command(&shell.core).args(["events", "--follow"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Ok(event) = serde_json::from_str::<Value>(&line) {
                        notice(app, shell, &event);
                    }
                }
            }
            let _ = child.wait();
        }
        std::thread::sleep(LISTEN_AGAIN_AFTER);
    }
}

/// What a run of a schedule says on the desktop, as the window says it (J6): what a task gave, a monitor that
/// spoke up, a run that failed or did not run, a schedule that paused itself. A monitor with nothing new says
/// nothing. Shown only while the window is not being looked at.
fn said_by_a_run(event: &Value) -> Option<(String, String, String)> {
    if event["kind"] != "schedule.ran" {
        return None;
    }
    let d = &event["data"];
    let text = |k: &str| d[k].as_str().unwrap_or_default().to_string();
    let name = [text("watch"), text("what")].into_iter().find(|n| !n.is_empty()).unwrap_or_else(|| "A schedule".into());
    let late = if d["late"] == true { " (late: the computer was asleep or Demido was not running at its time)" } else { "" };
    let note = text("note");
    let body = if d["paused"] == true {
        format!("Paused itself. {}", d["paused_why"].as_str().unwrap_or(&note))
    } else {
        match d["state"].as_str()? {
            "told" => note.clone(),
            "done" => format!("{note}{late}"),
            "failed" => format!("Failed{late}. {note}"),
            "skipped" => format!("Did not run. {note}"),
            _ => return None,
        }
    };
    Some((name, body.trim().to_string(), text("session")))
}

fn notice(app: &AppHandle, shell: &Shell, event: &Value) {
    let Some((title, body, session)) = said_by_a_run(event) else { return };
    let looked_at = app.get_webview_window(MAIN).is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    #[cfg(debug_assertions)]
    eprintln!("demido-desktop: a run said \"{title}: {body}\"; the window is {}looked at", if looked_at { "" } else { "not " });
    if looked_at {
        return;
    }
    let id = shell.next_notice.fetch_add(1, Ordering::Relaxed);
    shell.notices.lock().unwrap_or_else(|e| e.into_inner()).insert(id, session);
    let _ = app.notification().builder().id(id).title(title).body(body.chars().take(300).collect::<String>()).show();
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Opens a session in the window, from a notice.
fn open_session(app: &AppHandle, session: &str) {
    #[cfg(debug_assertions)]
    eprintln!("demido-desktop: opened session {session}");
    show_window(app);
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.eval(format!("window.location.hash = {}", serde_json::to_string(&format!("#/sessions/{session}")).unwrap_or_default()));
    }
}

/// The settings of the window's webview that Tauri leaves to the system (F6).
fn set_up_the_webview(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::{SettingsExt, WebViewExt};
            if let Some(settings) = WebViewExt::settings(&webview.inner()) {
                // WebRTC off for every page of the window, a page a model gave among them (IDEAS 142). It is off
                // by WebKitGTK's default too; said here so that no later default can turn it on. The microphone
                // is something else (MediaStream), and stays for voice mode.
                // A development build can turn it on (`DEMIDO_DESKTOP_WEBRTC=on`), only to show that the trial
                // that measures it would see it (core/dev/desktop-trial.sh). A release cannot.
                let on = cfg!(debug_assertions) && std::env::var("DEMIDO_DESKTOP_WEBRTC").as_deref() == Ok("on");
                settings.set_enable_webrtc(on);
                settings.set_enable_media_stream(true);
                #[cfg(debug_assertions)]
                eprintln!("demido-desktop: WebRTC {}, MediaStream {}", settings.enables_webrtc(), settings.enables_media_stream());
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = webview;
    });
}

fn main() {
    let shell = Arc::new(Shell { core: core_program(), address: Mutex::default(), notices: Mutex::default(), next_notice: AtomicI32::new(1) });
    tauri::Builder::default()
        // A second start shows the window that is there.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_window(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup({
            let shell = shell.clone();
            move |app| {
                let handle = app.handle().clone();
                {
                    let (handle, shell) = (handle.clone(), shell.clone());
                    app.notification().on_action(move |done| {
                        if done.action_id() != "tap" {
                            return;
                        }
                        let session = done.notification().and_then(|n| shell.notices.lock().unwrap_or_else(|e| e.into_inner()).get(&n.id()).cloned());
                        match session {
                            Some(session) => open_session(&handle, &session),
                            None => show_window(&handle),
                        }
                    })?;
                }
                let (guard, opener, asks) = (shell.clone(), handle.clone(), shell.clone());
                let mut builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
                    .title("Demido Studio")
                    .inner_size(1280.0, 840.0)
                    .min_inner_size(720.0, 480.0)
                    .initialization_script(IN_THE_SHELL)
                    .on_navigation(move |url| may_show(&guard, url))
                    // A link to elsewhere opens in the person's own browser; nothing opens in a window of the shell.
                    .on_new_window(move |url, _| {
                        if matches!(url.scheme(), "http" | "https") {
                            let _ = opener.opener().open_url(url.as_str(), None::<&str>);
                        }
                        NewWindowResponse::Deny
                    })
                    // The microphone for the core's own page (voice mode, dictation); nothing else, for nobody (F6).
                    .on_permission_request(move |webview, kind| {
                        let ours = webview.url().is_ok_and(|u| u.scheme() != "tauri" && may_show(&asks, &u));
                        match kind {
                            PermissionKind::Microphone if ours => PermissionResponse::Allow,
                            _ => PermissionResponse::Deny,
                        }
                    });
                #[cfg(windows)]
                {
                    // WebView2 is Chromium: UDP only through a proxy, and the proxy one that is not there, so that
                    // WebRTC reaches nothing (F6). The core is on this computer, which no proxy is asked for.
                    builder = builder.additional_browser_args(
                        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --force-webrtc-ip-handling-policy=disable_non_proxied_udp --proxy-server=http://127.0.0.1:9",
                    );
                }
                // `--hidden`: only in the tray, as when started with the computer.
                builder = builder.visible(!std::env::args().any(|a| a == "--hidden"));
                let window = builder.build()?;
                set_up_the_webview(&window);

                let open = MenuItem::with_id(app, "open", "Open Demido", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Close the app (Demido keeps working)", true, None::<&str>)?;
                let stop = MenuItem::with_id(app, "stop", "Stop Demido and close", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit, &stop])?;
                let core = shell.core.clone();
                TrayIconBuilder::with_id("demido")
                    .icon(app.default_window_icon().cloned().ok_or("the app has no icon")?)
                    .tooltip("Demido Studio")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| match event.id().as_ref() {
                        "open" => show_window(app),
                        "quit" => app.exit(0),
                        "stop" => {
                            let _ = command(&core).arg("stop").stdout(Stdio::null()).stderr(Stdio::null()).status();
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                            show_window(tray.app_handle());
                        }
                    })
                    .build(app)?;
                join_the_core(handle, shell.clone());
                Ok(())
            }
        })
        // Closing the window hides it: the app stays in the tray, and the core goes on (F5).
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("the desktop shell could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(address: Option<&str>) -> Shell {
        Shell { core: PathBuf::new(), address: Mutex::new(address.map(str::to_string)), notices: Mutex::default(), next_notice: AtomicI32::new(1) }
    }

    #[test]
    fn the_window_shows_the_shells_page_and_the_cores_and_nothing_else() {
        let s = shell(Some("127.0.0.1:7341"));
        let ok = |u: &str| may_show(&s, &Url::parse(u).unwrap());
        assert!(ok("tauri://localhost/index.html") && ok("http://tauri.localhost/index.html"));
        assert!(ok("http://127.0.0.1:7341/#/sessions") && ok("http://127.0.0.1:7341/page-frame") && ok("about:srcdoc") && ok("blob:http://127.0.0.1:7341/2c1f"));
        assert!(!ok("http://127.0.0.1:7342/") && !ok("https://example.com/?q=secret") && !ok("blob:https://example.com/2c1f") && !ok("file:///etc/passwd") && !ok("javascript:alert(1)"));
        // Before the core is joined, only the shell's own page.
        assert!(!may_show(&shell(None), &Url::parse("http://127.0.0.1:7341/").unwrap()));
    }

    #[test]
    fn a_run_says_on_the_desktop_what_it_says_in_the_window() {
        let ran = |data: Value| serde_json::json!({ "kind": "schedule.ran", "data": data });
        assert_eq!(
            said_by_a_run(&ran(serde_json::json!({ "state": "done", "what": "Morning report", "note": "3 new emails.", "session": "s1" }))),
            Some(("Morning report".into(), "3 new emails.".into(), "s1".into()))
        );
        assert_eq!(
            said_by_a_run(&ran(serde_json::json!({ "state": "failed", "watch": "Price of oats", "what": "x", "note": "No answer.", "late": true, "session": "s2" }))).unwrap().1,
            "Failed (late: the computer was asleep or Demido was not running at its time). No answer."
        );
        assert_eq!(
            said_by_a_run(&ran(serde_json::json!({ "state": "done", "paused": true, "paused_why": "It failed three times.", "session": "s3" }))).unwrap().1,
            "Paused itself. It failed three times."
        );
        // A monitor with nothing new, and other news, say nothing.
        assert_eq!(said_by_a_run(&ran(serde_json::json!({ "state": "quiet", "session": "s4" }))), None);
        assert_eq!(said_by_a_run(&serde_json::json!({ "kind": "session.message", "data": {} })), None);
    }
}
