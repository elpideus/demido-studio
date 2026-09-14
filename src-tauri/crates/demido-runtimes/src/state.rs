//! The ledger: one row per runtime, and the state that decides everything.
//!
//! `docs/rules/runtimes.md` section 0: a row is managed, linked or absent, and
//! every rule keys off the state rather than off a path comparison. That is
//! not a comment here, it is the shape of [`RowState`] itself. A linked row
//! carries a path and nothing to sum into a total, an absent row carries only
//! a reason, and a managed row is the only variant with bytes on disk to add
//! up, so the total that counts a binary the user built is not merely avoided,
//! it cannot be written.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A runtime the ledger tracks.
///
/// A string rather than an enum with a match arm per row, because "the
/// required group is data, so the capability group can be added without a
/// second screen" has to be true of the ledger and not only of
/// `demido_catalog::MANIFEST`. Adding `agent-browser` later is a new id.
pub type RuntimeId = String;

/// What Demido may do with a row, decided by which of these it is in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RowState {
    /// Not fetched, or a fetch or a link attempt was refused by verification.
    ///
    /// `reason` is `None` only for a row nobody has ever acted on. Once a
    /// fetch or a link has been tried and refused, sections 2 and 10 both say
    /// the row is "absent with a reason", never a bare absence again.
    Absent { reason: Option<String> },

    /// Demido fetched this pin into the profile's runtimes folder.
    ///
    /// `archives` names what was unpacked into the row's directory, which for
    /// the required group is the build and its `cudart` companion: section 7
    /// makes those "one row's worth of action", and section 2 verifies both
    /// with one command because a CUDA build that cannot resolve
    /// `cublasLt64_13.dll` does not load a model. They are one row here for
    /// the same reason.
    ///
    /// `on_disk_mib` is measured from what actually unpacked, never copied
    /// from the manifest's estimate, so the ledger reports what Demido spent
    /// rather than what it expected to spend.
    Managed {
        pin: String,
        archives: Vec<String>,
        on_disk_mib: f64,
    },

    /// A path the user pointed at. Demido reads and launches it, never writes
    /// inside it, and there is no delete control on it at all.
    Linked {
        path: PathBuf,
        detected_version: Option<String>,
    },
}

impl RowState {
    /// The disk Demido spent on this row, in MiB. Zero for anything that is
    /// not `Managed`: a linked binary's bytes are never Demido's to claim,
    /// and an absent row has none.
    pub fn on_disk_mib(&self) -> f64 {
        match self {
            RowState::Managed { on_disk_mib, .. } => *on_disk_mib,
            RowState::Absent { .. } | RowState::Linked { .. } => 0.0,
        }
    }

    /// What a person is told this row is, and what the refusal messages call
    /// it. One place, so a row cannot be described two ways.
    pub fn label(&self) -> &'static str {
        match self {
            RowState::Absent { .. } => "an absent",
            RowState::Managed { .. } => "a managed",
            RowState::Linked { .. } => "a linked",
        }
    }

    pub fn is_managed(&self) -> bool {
        matches!(self, RowState::Managed { .. })
    }

    pub fn is_linked(&self) -> bool {
        matches!(self, RowState::Linked { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: RuntimeId,
    pub state: RowState,
}

/// The shape this build can read. A ledger from another generation is kept
/// aside rather than misread ([`crate::file::Files`]).
pub const GENERATION: u32 = 1;

/// What a managed row's directory is called, under the profile's runtimes
/// folder.
///
/// One function rather than a `format!` at each call site, because the
/// directory a row deletes and the directory `unused` decides is claimed have
/// to be the same string. Two copies of this rule drifting apart is a row
/// deleting nothing and its bytes then being offered for removal as an
/// orphan.
pub fn directory_name(id: &str, pin: &str) -> String {
    format!("{id}-{pin}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub generation: u32,
    pub rows: Vec<Row>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            generation: GENERATION,
            rows: Vec::new(),
        }
    }
}

impl Ledger {
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.id == id)
    }

    pub fn state(&self, id: &str) -> Option<&RowState> {
        self.row(id).map(|row| &row.state)
    }

    /// Replace `id`'s row, or add it if the ledger has never seen it.
    pub fn set(&mut self, id: impl Into<RuntimeId>, state: RowState) {
        let id = id.into();
        match self.rows.iter_mut().find(|row| row.id == id) {
            Some(row) => row.state = state,
            None => self.rows.push(Row { id, state }),
        }
    }

    /// What every managed row together spent. `RowState::on_disk_mib` already
    /// excludes linked and absent rows, so this cannot drift from that rule by
    /// forgetting a filter here.
    pub fn on_disk_mib(&self) -> f64 {
        self.rows.iter().map(|row| row.state.on_disk_mib()).sum()
    }

    /// The directories under the runtimes folder that a row is currently
    /// running from.
    ///
    /// Section 5's Unused is everything else. The diff is against what the
    /// ledger claims rather than against every pin the manifest names,
    /// deliberately: a directory holding the current pin that no row is
    /// managed at is exactly section 5's first case, a fetch killed halfway,
    /// and a manifest-shaped diff would call those bytes claimed and never
    /// show them to anybody.
    pub fn claimed_directories(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|row| match &row.state {
                RowState::Managed { pin, .. } => Some(directory_name(&row.id, pin)),
                RowState::Absent { .. } | RowState::Linked { .. } => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn a_linked_rows_bytes_are_never_in_the_total() {
        let mut ledger = Ledger::default();
        ledger.set(
            "llama.cpp",
            RowState::Linked {
                path: PathBuf::from("C:/tools/llama-server.exe"),
                detected_version: Some("b10820".into()),
            },
        );
        assert_eq!(
            ledger.on_disk_mib(),
            0.0,
            "a llama.cpp the user built is not Demido's 671 MiB"
        );
    }

    #[test]
    fn only_managed_rows_sum_into_the_ledger() {
        let mut ledger = Ledger::default();
        ledger.set(
            "llama.cpp",
            RowState::Managed {
                pin: "b10816".into(),
                archives: vec!["llama-b10816-bin-win-cuda-13.3-x64.zip".into()],
                on_disk_mib: 182.6,
            },
        );
        ledger.set("model", RowState::Absent { reason: None });
        assert_eq!(ledger.on_disk_mib(), 182.6);
    }

    #[test]
    fn a_linked_row_claims_no_directory_to_protect_from_the_unused_diff() {
        let mut ledger = Ledger::default();
        ledger.set(
            "llama.cpp",
            RowState::Linked {
                path: PathBuf::from("C:/tools/llama-server.exe"),
                detected_version: None,
            },
        );
        assert!(
            ledger.claimed_directories().is_empty(),
            "a linked row runs from outside the runtimes folder entirely"
        );
    }
}
