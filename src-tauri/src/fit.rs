//! The fit verdict, as the detail pane asks for it
//! ([#74](https://github.com/elpideus/demido-studio/issues/74)).
//!
//! `demido-vram` owns the reading and the arithmetic, and the conversation
//! knows what its resident model was weighed at holding, which a load would
//! give back (`demido_chat::Chat::held`). This is where the two meet.
//!
//! **The card is read every time the pane asks.** Its free memory is not what
//! it was when the pane was drawn: a browser window opened in between moves it
//! by more than `done.md`'s tightest row has to spare. So nothing here keeps a
//! reading, the window asks again while the pane is open, and the download
//! never acts on a verdict at all: `downloads_add` takes a repository and a
//! choice, and no verdict is among its arguments. A verdict informs; it cannot
//! be the thing a stale reading turned into a refusal.

use std::path::PathBuf;

use demido_vram::{free_now, verdict, Load, Verdict, BESIDE_THE_KV};

use crate::wiring::Wiring;

/// Whether this card can hold weights of `weights` bytes, read now.
///
/// `path` is the model's file when it is already on disk, and then the context
/// is priced too: one slot at the context length in force, read from the
/// file's header by the same reading that prices a slot for the pool, with
/// what the build holds beside it
/// (`demido_models::slot`,
/// [#105](https://github.com/elpideus/demido-studio/issues/105)). A file not
/// yet downloaded has no header to read, and an architecture the reading does
/// not know is not priced, so for both the verdict says *before the context*
/// rather than adding a guess.
#[tauri::command]
pub async fn models_fit(
    wiring: tauri::State<'_, Wiring>,
    weights: u64,
    path: Option<PathBuf>,
) -> demido_core::Result<Verdict> {
    let context = match path {
        Some(path) => {
            let tokens = wiring.chat.resolved().context_length();
            tauri::async_runtime::spawn_blocking(move || {
                demido_models::price(&path, tokens).per_slot
            })
            .await
            .ok()
            .flatten()
            // What the build holds beside the KV, the way the pool counts it,
            // so the verdict and the pool agree about one load.
            .map(|kv| kv.saturating_add(BESIDE_THE_KV))
        }
        None => None,
    };
    Ok(verdict(
        free_now(),
        wiring.chat.held().await,
        Load { weights, context },
    ))
}
