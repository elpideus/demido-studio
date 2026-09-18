//! The fit verdict, as the detail pane asks for it
//! ([#74](https://github.com/elpideus/demido-studio/issues/74)).
//!
//! `demido-vram` owns the reading and the arithmetic. What is here is the one
//! fact only the root knows: which model Demido has resident right now, whose
//! memory a load would give back.
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
    let replacing = resident(&wiring).await;
    Ok(verdict(
        free_now(),
        replacing,
        Load {
            weights,
            context: None,
        },
    ))
}

/// What the resident model was weighed at holding when it loaded, which a
/// model chosen next gets back. Zero when nothing is resident.
///
/// Weighed around the load (`demido_chat::Pool::weigh`) rather than read off
/// the model file: the rig's development model is a 7813 MiB file that holds
/// about 5 GiB of the card, so the file's size would count as room memory the
/// desktop and every other program are using.
async fn resident(wiring: &Wiring) -> u64 {
    if wiring.inference.current().await.is_none() {
        return 0;
    }
    wiring.chat.held()
}
