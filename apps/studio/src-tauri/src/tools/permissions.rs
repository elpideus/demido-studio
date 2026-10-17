//! When the assistant asks before it acts.
//!
//! Every tool call has a [`Risk`]: what it can do if the model is wrong, or is steered by
//! something it read. The person's [`PermissionMode`] says which risks are asked about:
//!
//! - Cautious asks before every call that reads or changes anything, and never remembers.
//! - Balanced asks only before risky calls (code, sending email, deleting for good, saving to an
//!   online account, a long download), and remembers "Always allow" where the card offers it.
//! - Autonomous never asks.
//!
//! Most tools are asked about by the agent loop before they run, with a card that shows the
//! call. A tool whose risk depends on its arguments, or that shows the person something better
//! than its arguments (an email about to be sent, the emails about to be moved), asks itself
//! with [`super::ToolContext::confirm`] and is marked `confirms` in the tool list.

use serde::Serialize;

use crate::settings::{PermissionMode, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    /// Changes nothing and brings nothing in: loading tools, reading skills.
    None,
    /// Reads: prices, files, email. Its results land in the chat or the workspace only.
    Read,
    /// Changes something that can be changed back: a workspace file, the chart, the library,
    /// moving or flagging email, a draft, a folder.
    Change,
    /// Runs code, reaches other people, deletes for good, changes an online account, or takes
    /// long: always worth a look.
    Risky,
}

/// Whether a call of `tool` at `risk` waits for the person's OK.
pub fn must_ask(settings: &Settings, tool: &str, risk: Risk) -> bool {
    match settings.permission_mode {
        PermissionMode::Autonomous => false,
        PermissionMode::Cautious => risk >= Risk::Read,
        PermissionMode::Balanced => risk >= Risk::Risky && !settings.always_allowed_tools.contains(tool),
    }
}

/// Whether the card may offer "Always allow": only Balanced remembers, and only where the tool
/// allows it.
pub fn offers_always(settings: &Settings, allowed_by_tool: bool) -> bool {
    allowed_by_tool && settings.permission_mode == PermissionMode::Balanced
}

/// What the system prompt says about asking, so the model neither asks the user in words for what
/// the app asks anyway, nor takes Autonomous as licence to act on what it reads.
pub fn prompt_line(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Cautious => {
            "- The app asks the user to allow each tool call before it runs, so call tools directly instead of asking \
             in words; a declined call is the user's answer.\n"
        }
        PermissionMode::Balanced => {
            "- The app asks the user before risky calls (running code, sending email, deleting for good), so call \
             tools directly instead of asking in words; a declined call is the user's answer. Other calls run at \
             once: move, delete or change only what the user asked for.\n"
        }
        PermissionMode::Autonomous => {
            "- Your tool calls run without the user being asked. Do only what the user asked for; before sending \
             email, deleting for good or anything you cannot undo that they did not clearly ask for, ask them in \
             words first.\n"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: PermissionMode) -> Settings {
        Settings {
            permission_mode: mode,
            ..Settings::default()
        }
    }

    #[test]
    fn each_mode_asks_about_its_risks() {
        let cautious = settings(PermissionMode::Cautious);
        let balanced = settings(PermissionMode::Balanced);
        let autonomous = settings(PermissionMode::Autonomous);
        for risk in [Risk::None, Risk::Read, Risk::Change, Risk::Risky] {
            assert!(!must_ask(&autonomous, "run_command", risk), "autonomous never asks");
        }
        assert!(!must_ask(&cautious, "load_tools", Risk::None));
        assert!(must_ask(&cautious, "mail_list", Risk::Read));
        assert!(must_ask(&cautious, "mail_move", Risk::Change));
        assert!(!must_ask(&balanced, "mail_list", Risk::Read));
        assert!(!must_ask(&balanced, "mail_move", Risk::Change));
        assert!(must_ask(&balanced, "mail_send", Risk::Risky));
    }

    #[test]
    fn always_allow_counts_only_in_balanced() {
        let mut balanced = settings(PermissionMode::Balanced);
        balanced.always_allowed_tools.insert("run_command".into());
        assert!(!must_ask(&balanced, "run_command", Risk::Risky));
        assert!(must_ask(&balanced, "run_python", Risk::Risky));
        let mut cautious = balanced.clone();
        cautious.permission_mode = PermissionMode::Cautious;
        assert!(must_ask(&cautious, "run_command", Risk::Risky), "cautious always asks");
        assert!(offers_always(&balanced, true));
        assert!(!offers_always(&balanced, false));
        assert!(!offers_always(&cautious, true));
    }
}
