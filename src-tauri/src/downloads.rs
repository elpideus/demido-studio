//! The download queue, as the window reaches it
//! ([#73](https://github.com/elpideus/demido-studio/issues/73)).
//!
//! `demido-download` does the fetching and keeps the rows. What is here is the
//! window's handle on it: the commands behind the indicator's pause, resume,
//! cancel and `Pause all`, and one event carrying every row change, which the
//! indicator puts in a ref rather than in state (#76).
//!
//! **The window names a choice, never a URL.** `downloads_add` takes a
//! repository and a choice's name and reads the files from the index again,
//! so what is fetched is what the index lists and where it lands is the
//! library's decision, not something the window could spell differently.

use demido_download::{Event, Id, Item, Row, HOST};
use demido_models::index::Answer;
use demido_models::Library;
use tauri::{AppHandle, Emitter, Manager};

use crate::wiring::Wiring;

/// Every row change, as [`Event`].
pub const EVENT: &str = "downloads://event";

/// Carry on with what the profile left running, and tell the window about
/// every change from here on. Called once, from setup; the queue is started
/// from inside the runtime, which is where its transfers are spawned.
pub fn start(app: &AppHandle) {
    let queue = app.state::<Wiring>().downloads.clone();
    let mut events = queue.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        queue.start();
        loop {
            match events.recv().await {
                Ok(event) => tell(&app, &event),
                // A window that fell behind lost some progress figures, never
                // a row: `downloads_rows` is always whole, and the next change
                // carries the row as it is now.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::debug!(missed, "the window fell behind the download queue");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

fn tell(app: &AppHandle, event: &Event) {
    if let Err(error) = app.emit(EVENT, event) {
        tracing::warn!(%error, "the window was not told about a download");
    }
}

/// Every row, in the order they were queued, for a window that opened late.
#[tauri::command]
pub fn downloads_rows(wiring: tauri::State<'_, Wiring>) -> Vec<Row> {
    wiring.downloads.rows()
}

/// Queue the choice called `name` in `repo`: its weights, every shard of
/// them, and the projector they need, as one row.
#[tauri::command]
pub async fn downloads_add(
    wiring: tauri::State<'_, Wiring>,
    repo: String,
    name: String,
) -> demido_core::Result<Id> {
    let models = &wiring.setup.models;
    let choices = match models.choices(&repo).await {
        Answer::Read { found } => found,
        Answer::Unreadable { reason, .. } => {
            return Err(demido_core::Error::unavailable("the index", reason))
        }
    };
    let Some(choice) = choices.iter().find(|choice| choice.name == name) else {
        return Err(demido_core::Error::invalid(
            "a download",
            format!("{repo} has no {name}"),
        ));
    };
    let library = Library::open(&models.folders());
    Ok(wiring
        .downloads
        .enqueue(Item::chosen(HOST, &repo, choice, &library)))
}

// Pause, resume and cancel are async so they run on the runtime rather than
// the window's thread: a cancel deletes partial files, and a resume spawns the
// transfer on the runtime it is already on.

#[tauri::command]
pub async fn downloads_pause(wiring: tauri::State<'_, Wiring>, id: Id) -> demido_core::Result<()> {
    wiring.downloads.pause(id);
    Ok(())
}

#[tauri::command]
pub async fn downloads_pause_all(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<()> {
    wiring.downloads.pause_all();
    Ok(())
}

/// Resume a paused item, or retry a failed one from the bytes on disk.
#[tauri::command]
pub async fn downloads_resume(wiring: tauri::State<'_, Wiring>, id: Id) -> demido_core::Result<()> {
    wiring.downloads.resume(id);
    Ok(())
}

/// Take an item out of the queue and delete its partial files.
#[tauri::command]
pub async fn downloads_cancel(wiring: tauri::State<'_, Wiring>, id: Id) -> demido_core::Result<()> {
    wiring.downloads.cancel(id);
    Ok(())
}
