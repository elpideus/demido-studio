//! The commands the prompt editor calls.
//!
//! Three, and they are the three the register already has: read the list, set
//! one entry's text, forget an edit. Nothing here composes a path, reads a
//! default file or decides what a paragraph says when there is no edit, because
//! `demido_prompts::register` is the seam for all of that and a second opinion
//! about any of it would be a second place deciding what a prompt says, which is
//! the failure `docs/rules/prompts.md` exists to prevent.
//!
//! **Only the paragraph register.** The tool register's editor is S3, and this
//! build offers no command that would open one: a page listing twenty five tool
//! documents before there is a surface that declares their dependants would be
//! offering to change something whose cost is not yet written down.
//!
//! What a [`Prompt`] carries is what the editor draws, and it is deliberately
//! the whole truth about one entry rather than a projection of it: the
//! declaration with its dependants and its shipped default, the text in force,
//! where that came from, its hash, the base hash an edit was made from, the
//! measured claims the edit suppressed, and the note. The window renders that;
//! it does not compute any of it.

use demido_prompts::Prompt;

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
