//! One event per line, in a file that is only ever appended to.
//!
//! The format is the argument. A session log is the thing a person opens when
//! the app and their memory disagree, and JSON Lines is readable in any editor,
//! greppable, tailable while a turn is running, and appendable without reading
//! or rewriting a byte of what is already there. A database would give indexes
//! that nothing yet needs and take away every one of those.
//!
//! It is also what makes the restart promise cheap: there is no state to
//! recover beyond the last line, and [`JsonLines::open`] recovers it by reading
//! the file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::event::{Entry, Event};
use crate::journal::{now, Error, Journal, Result};

/// A session log on disk.
#[derive(Debug)]
pub struct JsonLines {
    path: PathBuf,
    /// The handle and the next sequence number, under one lock, because they
    /// have to move together: two appends that agreed on a number would put two
    /// lines in the log claiming the same position.
    writing: Mutex<Writing>,
}

#[derive(Debug)]
struct Writing {
    file: std::fs::File,
    next: u64,
}

impl JsonLines {
    /// Open a log, creating the file and its directory if they are not there.
    ///
    /// Reads what is already in the file, which is how numbering carries on
    /// across a restart. A **truncated last line is dropped and the file cut
    /// back to the last complete one**: a process killed mid write leaves half
    /// a line, and refusing to open the log over it would turn one lost event
    /// into a lost session. A malformed line anywhere else is refused, because
    /// that is not a crash, it is something else having written to the file.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
            }
        }

        let complete = trim_partial_line(&path)?;
        let events = parse(&complete, &path)?;
        let next = events.last().map(|event| event.seq + 1).unwrap_or(1);

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::io(format!("opening {}", path.display()), error))?;

        Ok(Self {
            path,
            writing: Mutex::new(Writing { file, next }),
        })
    }

    /// Read a log without opening it for writing, and without repairing it.
    ///
    /// [`JsonLines::open`] creates the file and cuts a torn last line off it,
    /// which is right for a log a session is about to append to and wrong for
    /// one that is evidence. A committed trace fixture is read through here
    /// (`docs/rules/done.md`), so replaying it can never write to it.
    pub fn read(path: impl AsRef<Path>) -> Result<Vec<Event>> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| Error::io(format!("reading {}", path.display()), error))?;
        parse(&bytes, path)
    }
}

impl Journal for JsonLines {
    fn append(&self, entry: Entry) -> Result<Event> {
        let mut writing = self.writing.lock().unwrap_or_else(|held| held.into_inner());

        let event = Event {
            seq: writing.next,
            at: now(),
            session: entry.session,
            turn: entry.turn,
            source: entry.source,
            weight: entry.weight,
            body: entry.body,
        };

        let mut line = serde_json::to_string(&event)
            .map_err(|error| Error::io("writing an event", std::io::Error::other(error)))?;
        line.push('\n');

        writing
            .file
            .write_all(line.as_bytes())
            .map_err(|error| Error::io(format!("appending to {}", self.path.display()), error))?;
        // Flushed to the operating system on every append, so a process that
        // dies still leaves the line. Not `sync_all`: a disk flush per event
        // costs milliseconds and buys only power loss, which is not what this
        // log is protecting against.
        writing
            .file
            .flush()
            .map_err(|error| Error::io(format!("appending to {}", self.path.display()), error))?;

        writing.next += 1;
        Ok(event)
    }

    fn events(&self) -> Result<Vec<Event>> {
        // Under the write lock, so a reader never sees a line that is half
        // written. The lock is the same one an append takes, which is what
        // makes "read while a turn is running" a supported thing to do rather
        // than a race.
        let _writing = self.writing.lock().unwrap_or_else(|held| held.into_inner());
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(Error::io(format!("reading {}", self.path.display()), error)),
        };
        parse(&bytes, &self.path)
    }
}

/// Cut a half written last line off the file, and hand back what is left.
///
/// Returns the complete bytes rather than reading the file twice.
fn trim_partial_line(path: &Path) -> Result<Vec<u8>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(format!("reading {}", path.display()), error)),
    };

    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(bytes);
    }

    let complete = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(at) => at + 1,
        None => 0,
    };

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| Error::io(format!("repairing {}", path.display()), error))?;
    file.set_len(complete as u64)
        .map_err(|error| Error::io(format!("repairing {}", path.display()), error))?;
    tracing::warn!(
        path = %path.display(),
        bytes = bytes.len() - complete,
        // not-a-prompt: a log line. Nothing reads it but a developer.
        "dropped a half written last line from the session log"
    );

    Ok(bytes[..complete].to_vec())
}

fn parse(bytes: &[u8], path: &Path) -> Result<Vec<Event>> {
    let text = std::str::from_utf8(bytes).map_err(|error| Error::Corrupt {
        line: 0,
        detail: format!("{} is not UTF-8: {error}", path.display()),
    })?;

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| Error::Corrupt {
                line: index + 1,
                detail: error.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use crate::event::{Body, Entry, SessionId, Source, Weight};
    use demido_inference::Role;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-trace-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("session.jsonl")
    }

    fn said(what: &str) -> Entry {
        Entry::new(
            SessionId::new("s"),
            1,
            Source::User,
            Weight::estimated(1),
            Body::Message {
                role: Role::User,
                text: what.to_owned(),
            },
        )
    }

    #[test]
    fn a_log_is_created_with_its_directory() {
        let path = scratch("created");
        let journal = JsonLines::open(&path).expect("opened");
        assert!(journal.events().expect("read").is_empty());
        assert!(path.exists(), "opening a log creates the file");
    }

    #[test]
    fn a_half_written_last_line_costs_that_line_and_nothing_else() {
        // A process killed mid append. The line is gone; the session is not.
        let path = scratch("truncated");
        {
            let journal = JsonLines::open(&path).expect("opened");
            journal.append(said("first")).expect("appended");
            journal.append(said("second")).expect("appended");
        }

        let mut bytes = std::fs::read(&path).expect("read");
        bytes.truncate(bytes.len() - 20);
        std::fs::write(&path, &bytes).expect("wrote");

        let journal = JsonLines::open(&path).expect("opened over the damage");
        let events = journal.events().expect("read");
        assert_eq!(events.len(), 1, "the complete line survives");
        assert_eq!(events[0].seq, 1);

        let next = journal.append(said("third")).expect("appended");
        assert_eq!(next.seq, 2, "numbering carries on from the last good line");
        assert_eq!(
            journal.events().expect("read").len(),
            2,
            "the half line was cut off rather than left in the middle of the log"
        );
    }

    #[test]
    fn a_line_that_is_not_an_event_is_refused_rather_than_skipped() {
        // Not a crash: something else wrote to the file. Reading past it would
        // hand the monitor a session with a hole in it and no way to know.
        let path = scratch("corrupt");
        {
            let journal = JsonLines::open(&path).expect("opened");
            journal.append(said("first")).expect("appended");
        }
        std::fs::write(
            &path,
            format!(
                "{{\"not\": \"an event\"}}\n{}",
                std::fs::read_to_string(&path).unwrap()
            ),
        )
        .expect("wrote");

        let error = JsonLines::open(&path).expect_err("refused");
        assert!(
            matches!(error, Error::Corrupt { line: 1, .. }),
            "the report names the line: {error}"
        );
    }
}
