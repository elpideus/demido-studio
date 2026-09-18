//! The commands the prompt editor calls.
//!
//! Three, and they are the three the register already has: read the list, set
//! one entry's text, forget an edit. Nothing here composes a path, reads a
//! default file or decides what a paragraph says when there is no edit, because
//! `demido_prompts::register` is the seam for all of that and a second opinion
//! about any of it would be a second place deciding what a prompt says, which is
//! the failure `docs/rules/prompts.md` exists to prevent.
//!
//! **Both registers, three commands each.** The paragraph register's landed on
//! [#58](https://github.com/elpideus/demido-studio/issues/58); the tool
//! register's on [#77](https://github.com/elpideus/demido-studio/issues/77),
//! once every document could declare what it is load-bearing for.
//!
//! What a [`Prompt`] carries is what the editor draws, and it is deliberately
//! the whole truth about one entry rather than a projection of it: the
//! declaration with its dependants and its shipped default, the text in force,
//! where that came from, its hash, the base hash an edit was made from, the
//! measured claims the edit suppressed, and the note. The window renders that;
//! it does not compute any of it.

use demido_prompts::{Document, Prompt};
use serde::Serialize;

use crate::wiring::Wiring;

/// Every paragraph, in register order, as it stands right now.
///
/// Read on every open and again after every change, rather than edited in
/// place. That is the rule `web/src/settings/ladder.ts` already follows and the
/// one the register is built for: it holds no state, so what comes back here is
/// what the next turn would send and not a copy that has stopped tracking it.
#[tauri::command]
pub fn prompts_list(wiring: tauri::State<'_, Wiring>) -> Vec<Prompt> {
    wiring.prompts.all()
}

/// Replace one paragraph's text.
///
/// **Nothing is refused for what depends on it.** The brief is unambiguous
/// ("All prompts should be editable"), so an entry that is load-bearing carries
/// its dependants and an edit suppresses the measured claim rather than being
/// turned down. What can still fail is an id this build does not have, a
/// placeholder nothing would ever fill, and the disk.
///
/// Nothing comes back, the way `settings_set` hands nothing back: the page
/// reads the register again after a change, and an entry returned here would be
/// a second answer to a question the read already asks. The register makes that
/// difference real rather than cosmetic, because text equal to the built-in
/// default resets instead of writing a file, so the entry that would come back
/// is not always the one that was asked for.
#[tauri::command]
pub fn prompts_set(
    wiring: tauri::State<'_, Wiring>,
    id: String,
    text: String,
) -> demido_core::Result<()> {
    wiring.prompts.set(&id, &text)?;
    Ok(())
}

/// Forget the edit, so the text this build ships is what the next turn sends.
///
/// This is also the way to take a default that has moved since an edit was made:
/// the note and the diff say it has, and this is the one gesture that acts on
/// them.
#[tauri::command]
pub fn prompts_reset(wiring: tauri::State<'_, Wiring>, id: String) -> demido_core::Result<()> {
    wiring.prompts.reset(&id)?;
    Ok(())
}

/// One tool's document, and the shape it is merged onto.
///
/// The shape is here so the editor can draw it as what it is: the half of what
/// a model is shown that is a contract with the parser and not editable text
/// (`docs/rules/prompts.md`). `None` for a document whose tool this build did
/// not register, which is the state `demido-tools`' `tests/documents.rs`
/// keeps anything from reaching.
#[derive(Debug, Serialize)]
pub struct ToolDocument {
    #[serde(flatten)]
    pub document: Document,
    pub shape: Option<serde_json::Value>,
}

/// Every tool document, in register order, each beside its shape.
///
/// Read the way `prompts_list` is: on every open and after every change.
#[tauri::command]
pub fn tool_documents_list(wiring: tauri::State<'_, Wiring>) -> Vec<ToolDocument> {
    let mut shapes = wiring.chat.shapes();
    wiring
        .tools
        .all()
        .into_iter()
        .map(|document| {
            let shape = shapes
                .iter()
                .position(|(name, _)| name == document.tool.name)
                .map(|at| shapes.swap_remove(at).1);
            ToolDocument { document, shape }
        })
        .collect()
}

/// Replace one tool's document, description and parameter prose together.
///
/// Refused only for what could never reach a model as meant: prose for a
/// parameter the shape does not have, and a placeholder, which no tool
/// document declares. Never for what depends on the wording.
#[tauri::command]
pub fn tool_documents_set(
    wiring: tauri::State<'_, Wiring>,
    name: String,
    text: String,
) -> demido_core::Result<()> {
    wiring.tools.set(&name, &text)?;
    Ok(())
}

/// Forget the edit, so the document this build ships is what the next turn
/// offers.
#[tauri::command]
pub fn tool_documents_reset(
    wiring: tauri::State<'_, Wiring>,
    name: String,
) -> demido_core::Result<()> {
    wiring.tools.reset(&name)?;
    Ok(())
}
