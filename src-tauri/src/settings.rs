//! The commands a settings surface calls.
//!
//! Three, and they are the same three whichever surface is asking: the main
//! settings window edits the global tier, a conversation's own panel edits the
//! chat tier, and the set-up wizard will edit the global tier a step at a time.
//! One set of commands over one ladder is what makes that true.
//!
//! **The window names a tier and Rust names the subject.** A frontend that
//! could name a chat could name one that does not exist, and this build has one
//! session ([`crate::wiring`]). When there is a chat list, the id comes from
//! whichever conversation is open, and it still comes from Rust.

use serde_json::Value;

use demido_settings::{Ladder, Row, Scope, Tier};

use crate::wiring::Wiring;

/// Everything a settings page draws for one tier: the value in force, whether
/// this tier is the one saying so, and what reverting would leave behind.
///
/// The schema travels with the rows rather than being fetched separately,
/// because a control is drawn from its declaration and a page that asked for
/// the two halves in two calls could draw a value against the wrong control.
#[tauri::command]
pub fn settings_rows(
    wiring: tauri::State<'_, Wiring>,
    tier: Tier,
) -> demido_core::Result<Vec<Row>> {
    Ok(wiring.settings.view(&ladder(&wiring, tier)?))
}

/// Set one value on one tier.
///
/// Fails when the value is not one the setting takes, and says why in a
/// sentence the window shows: a settings page that quietly reverted would be
/// indistinguishable from one that never saved.
#[tauri::command]
pub fn settings_set(
    wiring: tauri::State<'_, Wiring>,
    tier: Tier,
    id: String,
    value: Value,
) -> demido_core::Result<()> {
    wiring.settings.set(&scope(&wiring, tier)?, &id, &value)?;
    Ok(())
}

/// Forget one tier's opinion, so the value below it applies again.
#[tauri::command]
pub fn settings_clear(
    wiring: tauri::State<'_, Wiring>,
    tier: Tier,
    id: String,
) -> demido_core::Result<()> {
    wiring.settings.clear(&scope(&wiring, tier)?, &id)?;
    Ok(())
}

/// Which tier's values these are, and whose.
///
/// The two tiers this build has a subject for. The model and character tiers
/// are stored and resolvable, and nothing can address them yet: there is no
/// model picker and no character system, so a command that accepted one would
/// be writing values against a subject nobody chose.
fn scope(wiring: &Wiring, tier: Tier) -> demido_core::Result<Scope> {
    match tier {
        Tier::Global => Ok(Scope::Global),
        Tier::Chat => Ok(Scope::chat(wiring.session)),
        Tier::Model | Tier::Character => Err(demido_core::Error::not_found(
            "an editable settings tier",
            tier.slug(),
        )),
    }
}

/// The scopes a page resolves through, ending at the tier it edits.
///
/// The global page's ladder is the global tier alone, so its rows have no
/// resolution to read and no provenance chip to draw: `design/windows.md` gives
/// the main settings window global values "and nothing else". A chat's ladder is
/// global and then that chat, so its rows are either following global or
/// overridden.
fn ladder(wiring: &Wiring, tier: Tier) -> demido_core::Result<Ladder> {
    match scope(wiring, tier)? {
        Scope::Global => Ok(Ladder::global()),
        _ => Ok(Ladder::for_chat(wiring.session)),
    }
}
