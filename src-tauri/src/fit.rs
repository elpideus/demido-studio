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

use demido_vram::{free_now, verdict, Load, Verdict};

use crate::wiring::Wiring;

/// Whether this card can hold weights of `weights` bytes, read now.
///
/// The context is not priced: its geometry is in a header the pane does not
/// fetch, and the verdict says so rather than adding a guess (`demido_vram`'s
/// `fit` module).
#[tauri::command]
pub async fn models_fit(
    wiring: tauri::State<'_, Wiring>,
    weights: u64,
) -> demido_core::Result<Verdict> {
    Ok(verdict(
        free_now(),
        wiring.chat.held().await,
        Load {
            weights,
            context: None,
        },
    ))
}
