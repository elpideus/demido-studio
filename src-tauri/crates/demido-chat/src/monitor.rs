//! What the session monitor draws over one assembly.
//!
//! The rebuild itself is [`demido_trace::Replay::rebuild`], because it is a
//! projection of the log and belongs with the other projections. What is here
//! is the one thing the log cannot answer on its own: **a tool group that is
//! not in an assembly, and whether it is missing because somebody switched it
//! off or because nothing offered it.**
//!
//! `docs/rules/tools.md` is explicit about why that distinction is the point:
//!
//! > a user debugging *why did it not use the file tools* opens the session
//! > monitor, sees no file tools in the assembly, and cannot tell a deliberate
//! > absence from a dropped one. The event is what distinguishes them.
//!
//! The event is `tools/offered`, and the field that answers it is its
//! [`Layer`]. Groups are the registry's, which is why this lives in the crate
//! that holds one rather than in the log.

use serde::Serialize;

use demido_trace::{Layer, Offering, Rebuild};

/// One assembly as the monitor draws it: what was sent, and what became of
/// each group of the registry in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assembly {
    /// Flattened, because a window selecting an event asks one question and a
    /// payload with the rebuild nested inside it would make every field of the
    /// thing being read one level deeper than the thing framing it.
    #[serde(flatten)]
    pub rebuild: Rebuild,
    pub groups: Vec<Grouped>,
}

/// What one group of the registry came to in one assembly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grouped {
    /// The group's name. The words a window draws for it are the window's.
    pub group: String,
    /// The tools of the group the assembly offered.
    pub offered: Vec<String>,
    /// The ones it did not.
    pub absent: Vec<String>,
    pub standing: Standing,
}

/// Why a group is or is not in an assembly.
///
/// Five, and the three that matter are the last three: a deliberate absence and
/// a defect must not look identical, and neither may be claimed where the log
/// cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    /// Every tool of the group was offered.
    Offered,
    /// Some were and some were not, which is a person switching tools rather
    /// than a group.
    Partial,
    /// None were, out of a set that had something in it and was named by
    /// somebody on the settings ladder. A choice, and the control that made it
    /// is the tool picker.
    SwitchedOff,
    /// None were, and nobody named a set: what was offered was everything the
    /// registry had, so the group was not in it. No workspace is the ordinary
    /// reason (`demido_tools::Registry::offered`), and a skill that is not
    /// installed will be another.
    Dropped,
    /// The assembly offered no tools at all, and **which of the two that was
    /// cannot be told from the log**.
    ///
    /// Both roads end here. A person can switch every group off in the picker,
    /// and a registry with no workspace offers nothing whatever the picker
    /// says; the set recorded is empty either way and the layer says who chose
    /// the set rather than why it came out empty. So this says the one thing
    /// that is certain, instead of naming a reason and sending somebody to a
    /// control that may not be the one that is wrong.
    Nothing,
}

/// Every group of the registry against the set one assembly was sent with.
///
/// A free function over the two, rather than a method on either, because it is
/// the whole of the reasoning and reading it should not mean reading a chat.
/// `groups` is what the registry holds now; `offering` is what the log recorded
/// then, in the wording it was recorded in.
pub(crate) fn grouped(
    groups: Vec<(&'static str, Vec<String>)>,
    offering: Option<&Offering>,
) -> Vec<Grouped> {
    let named: Vec<&str> = offering
        .map(|offering| {
            offering
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect()
        })
        .unwrap_or_default();

    // Why an absence is an absence, for every group that has one. It is one
    // answer per assembly rather than one per group, and that is a property of
    // the registry rather than a shortcut: a registry drops **all** of itself
    // or none of it, because what it drops for is having nowhere to act
    // (`demido_tools::Registry::offers`). So a set with anything in it proves
    // the registry was not the reason, and every absence in it is the ladder's.
    let absence = match named.is_empty() {
        // Nothing at all was offered, and the log cannot say which road that
        // came down. See [`Standing::Nothing`].
        true => Standing::Nothing,
        // Somebody named this set, which is the picker or a value above it, so
        // what it leaves out was left out on purpose.
        false if offering.is_some_and(|offering| offering.layer != Layer::Registry) => {
            Standing::SwitchedOff
        }
        // The registry's own set: everything it had was offered, so a group not
        // in it was not in the registry.
        false => Standing::Dropped,
    };

    groups
        .into_iter()
        .map(|(group, tools)| {
            let (offered, absent): (Vec<String>, Vec<String>) = tools
                .into_iter()
                .partition(|name| named.contains(&name.as_str()));
            let standing = match (offered.is_empty(), absent.is_empty()) {
                (_, true) => Standing::Offered,
                (false, _) => Standing::Partial,
                (true, _) => absence,
            };
            Grouped {
                group: group.to_owned(),
                offered,
                absent,
                standing,
            }
        })
        .collect()
}
