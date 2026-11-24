//! The sandbox on Windows, for real (DECISIONS T27, research note 29): set up, then programs run in it.
//!
//! Setting it up makes a user and firewall rules on the computer, so this runs only where that is wanted:
//! on the checks' Windows machine, which is an administrator and is thrown away afterwards
//! (`DEMIDO_TRY_WINDOWS_SANDBOX=1`). Anywhere else it says so and passes.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use demido_core::sandbox::{Ran, Sandbox, windows};

async fn run(sandbox: &Sandbox, folder: &Path, script: &str) -> Ran {
    sandbox.run(folder, script, Duration::from_secs(60), &AtomicBool::new(false)).await.unwrap_or_else(|e| panic!("{script}: {e}"))
}

#[tokio::test]
async fn a_program_runs_as_the_sandboxes_own_user_and_changes_nothing_outside_its_folder() {
    if std::env::var_os("DEMIDO_TRY_WINDOWS_SANDBOX").is_none() {
        eprintln!("Not tried: setting the sandbox up makes a user on this computer. DEMIDO_TRY_WINDOWS_SANDBOX=1 tries it (the checks do).");
        return;
    }
    // In the person's profile, as a project usually is; Demido's folder too.
    let root = std::env::temp_dir().join(format!("demido-sandbox-{}", std::process::id()));
    let (home, folder) = (root.join("home"), root.join("project"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    let me = windows::own_sid().unwrap();
    windows::setup(&home, &me).expect("set up, as the administrator the checks' machine runs as");
    let sandbox = Sandbox::find_for(&home, Some(PathBuf::from(env!("CARGO_BIN_EXE_demido"))));
    assert_eq!(sandbox.kind, "windows-user", "{:?}", sandbox.why_not);
    assert!(sandbox.contains());

    let who = run(&sandbox, &folder, "whoami").await;
    assert!(who.output.to_lowercase().contains("demidosandbox"), "{who:?}");

    // Its folder: it may read and change what is there.
    let made = run(&sandbox, &folder, "echo made> made.txt && type made.txt").await;
    assert!(made.output.contains("made") && made.code == Some(0), "{made:?}");
    assert_eq!(std::fs::read_to_string(folder.join("made.txt")).unwrap().trim(), "made");

    // Nothing outside it, even where Windows lets every user change things (a folder made at C:\).
    let public = PathBuf::from(format!(r"C:\demido-sandbox-public-{}", std::process::id()));
    std::fs::create_dir_all(&public).unwrap();
    let out = run(&sandbox, &folder, &format!(r#"echo out> "{}\out.txt""#, public.display())).await;
    assert!(!public.join("out.txt").exists(), "it changed a folder outside its own: {out:?}");
    // The person's profile is not there to read.
    let secret = root.join("secret.txt");
    std::fs::write(&secret, "the person's own").unwrap();
    let read = run(&sandbox, &folder, &format!(r#"type "{}""#, secret.display())).await;
    assert!(!read.output.contains("person's own"), "{read:?}");

    // No network.
    let net = run(&sandbox, &folder, r#"curl.exe -s -m 10 -o NUL -w "%{http_code}" https://example.com"#).await;
    assert!(!net.output.contains("200"), "it reached the internet: {net:?}");

    // Nor this computer's own servers (IDEAS 148): a server of the person's on loopback hears nothing from it,
    // over IPv4 or IPv6, while the person still reaches it.
    for (bind, url) in [("127.0.0.1:0", "http://127.0.0.1"), ("[::1]:0", "http://[::1]")] {
        let server = std::net::TcpListener::bind(bind).unwrap();
        server.set_nonblocking(true).unwrap();
        let port = server.local_addr().unwrap().port();
        let tried = run(&sandbox, &folder, &format!(r#"curl.exe -s -m 5 -o NUL -w "%{{http_code}}" {url}:{port}/"#)).await;
        assert!(server.accept().is_err(), "it reached a server on loopback ({url}): {tried:?}");
        std::net::TcpStream::connect(server.local_addr().unwrap()).expect("the person reaches it");
        std::thread::sleep(Duration::from_millis(200));
        assert!(server.accept().is_ok(), "the person's own connection did not arrive");
    }

    // Nothing of the person's environment comes along.
    // SAFETY: a test of its own; nothing else here reads the environment at the same time.
    unsafe { std::env::set_var("DEMIDO_TEST_SECRET", "a key of the person's") };
    let env = run(&sandbox, &folder, "set").await;
    assert!(!env.output.contains("a key of the person's"), "{env:?}");
    assert!(env.output.contains("TEMP="), "{env:?}");

    // Stopped, it ends with everything it started.
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        stopping.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let long = sandbox.run(&folder, "ping -n 120 127.0.0.1", Duration::from_secs(60), &stop).await.unwrap();
    assert!(long.stopped.is_some() && started.elapsed() < Duration::from_secs(20), "{long:?}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let left = std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq PING.EXE", "/NH"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&left.stdout).to_uppercase().contains("PING.EXE"), "a program it started is still running");

    // Taken back: no entry for the sandbox, and what it made is the person's.
    assert_ne!(windows::owner_of(&folder.join("made.txt")).as_deref(), Some(me.as_str()), "the sandbox's user made it");
    windows::give_back(&home, &folder).unwrap();
    assert_eq!(windows::owner_of(&folder.join("made.txt")).as_deref(), Some(me.as_str()));
    let entries = std::process::Command::new("icacls").arg(&folder).output().unwrap();
    assert!(!String::from_utf8_lossy(&entries.stdout).contains("DemidoSandbox"), "{}", String::from_utf8_lossy(&entries.stdout));
}
