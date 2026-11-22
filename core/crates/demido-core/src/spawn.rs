//! Starting a program that was written a moment ago.
//!
//! On Linux a program's file cannot be started while any process still holds it open for writing: the
//! kernel answers "Text file busy". Demido writes programs (an unpacked runtime, a downloaded Claude Code)
//! while it starts other processes, and a process started in the instant such a file is open inherits it
//! until it runs its own program. So a start that finds the file busy is tried again, for a moment. Found as
//! a test that failed about one run in six (IDEAS 139).

use std::time::Duration;

/// How many times a start is tried while the file is busy, 25 ms apart: one second in all.
const TRIES: u32 = 40;
const APART: Duration = Duration::from_millis(25);

/// Whether a start failed only because the program's file is open for writing somewhere.
pub fn busy(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::ExecutableFileBusy
}

/// `command.spawn()`, tried again while the program's file is busy.
pub fn when_free(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    for _ in 1..TRIES {
        match command.spawn() {
            Err(e) if busy(&e) => std::thread::sleep(APART),
            done => return done,
        }
    }
    command.spawn()
}

/// `command.output()` for a program started from async code, tried again while its file is busy.
pub async fn output_when_free(command: &mut tokio::process::Command) -> std::io::Result<std::process::Output> {
    for _ in 1..TRIES {
        match command.output().await {
            Err(e) if busy(&e) => tokio::time::sleep(APART).await,
            done => return done,
        }
    }
    command.output().await
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn a_program_still_open_for_writing_is_started_once_it_is_closed() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::testing::TempDir::new("demido-spawn");
        let path = dir.join("tool.sh");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"#!/bin/sh\necho started\n").unwrap();
        file.set_permissions(std::fs::Permissions::from_mode(0o755)).unwrap();
        // While it is open for writing, it cannot be started.
        assert!(std::process::Command::new(&path).spawn().is_err_and(|e| super::busy(&e)));
        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(file);
        });
        let started = super::when_free(std::process::Command::new(&path).stdout(std::process::Stdio::piped())).unwrap();
        closer.join().unwrap();
        assert_eq!(String::from_utf8(started.wait_with_output().unwrap().stdout).unwrap(), "started\n");
    }
}
