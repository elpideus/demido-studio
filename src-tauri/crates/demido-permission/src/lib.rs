//! Whether a call runs without asking.
//!
//! Brief B57: "There should be 3 agent modes: Cautious, Balanced, Autonomous."
//!
//! One pure function, [`verdict`]: an [`Intent`] and a [`Mode`] go in, a
//! [`Verdict`] comes out, and nothing is read from disk or asked of anybody.
//! The three modes are three rows in [`ROWS`] rather than three code paths, so
//! a fourth mode is a fourth row. Carried forward from v2's
//! `demido-tools::permission`; the table is
//! [`docs/rules/tools.md`](../../../../docs/rules/tools.md)'s.
//!
//! ## The floor
//!
//! **A destructive call asks**, in every mode including Autonomous, and
//! *always for this tool* cannot waive it. It is not a row, it is checked
//! before any row is: a setting somebody changed six weeks ago should not be
//! what stands between a model and something it cannot put back.
//!
//! ## Nothing but the matrix reads the mode
//!
//! A [`Mode`] is opaque. It has no accessor, no equality and no variant to
//! match on, so the only thing any caller can do with one is hand it to
//! [`verdict`]. That is how "the mode gates permissions and nothing else" is
//! held rather than hoped for: a step limit, a parallelism cap or a delegation
//! depth that wanted to vary by mode would have nothing to branch on but the
//! matrix's own answers. Probing [`verdict`] with a made-up intent to work out
//! which row is in force is still possible, and it is reading the matrix: that
//! is a review finding against whoever writes it, and #54 carries its own
//! assertion that the step limit is not read from the mode.
//!
//! ```compile_fail
//! // A mode cannot be compared, so nothing can ask whether it is Autonomous.
//! use demido_permission::Mode;
//! let _ = Mode::named("autonomous") == Mode::default();
//! ```
//!
//! ```compile_fail
//! // Nor printed, which would be the same question asked through a string.
//! use demido_permission::Mode;
//! let _ = format!("{:?}", Mode::default());
//! ```
//!
//! ```compile_fail
//! // Nor opened up to read its row.
//! use demido_permission::Mode;
//! let Mode(_row) = Mode::default();
//! ```
//!
//! The mode is also **never prose**: nothing here produces text, and
//! `tests/the_matrix.rs` holds every prompt Demido ships to naming no mode.

pub mod inherit;

pub use inherit::{inherit, Request, Resolution};

use demido_tools::{Ability, Intent};

/// One mode: its stored name, and what it does with each ability.
struct Row {
    name: &'static str,
    read: Verdict,
    write: Verdict,
    shell: Verdict,
    network: Verdict,
}

impl Row {
    fn verdict(&self, ability: Ability) -> Verdict {
        match ability {
            Ability::Read => self.read,
            Ability::Write => self.write,
            Ability::Shell => self.shell,
            Ability::Network => self.network,
        }
    }
}

/// The matrix. **The first row is the default**, and the row a name this build
/// has never heard of resolves to, so it is the strictest one.
///
/// Read is Allow in every row, which is only honest because every path has
/// already been through `Workspace::resolve`: *outside the project* never
/// reaches the point of asking.
static ROWS: &[Row] = &[
    Row {
        name: "cautious",
        read: Verdict::Allow,
        write: Verdict::Ask,
        shell: Verdict::Ask,
        network: Verdict::Ask,
    },
    Row {
        name: "balanced",
        read: Verdict::Allow,
        write: Verdict::Allow,
        shell: Verdict::Ask,
        network: Verdict::Ask,
    },
    Row {
        name: "autonomous",
        read: Verdict::Allow,
        write: Verdict::Allow,
        shell: Verdict::Allow,
        network: Verdict::Allow,
    },
];

/// Which row of the matrix is in force. Opaque on purpose: see the crate docs.
///
/// `Copy` because a resolution and its children each hold one
/// ([`inherit`](inherit::inherit)), and a copy of a mode tells a caller nothing
/// a mode does not: there is still no way to read it but to hand it to
/// [`verdict`].
#[derive(Clone, Copy)]
pub struct Mode(&'static Row);

impl Mode {
    /// The mode stored under `name`, or Cautious for a name this build has
    /// never heard of.
    ///
    /// Falling back to the strictest row is the only safe direction: a profile
    /// written by a newer build must never be read as permission to do more.
    /// Names are matched exactly, so `"Autonomous"` is an unknown name too.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self(ROWS.iter().find(|row| row.name == name).unwrap_or(&ROWS[0]))
    }

    /// Every name a mode is stored under, strictest first. What a mode control
    /// offers, and what a setting holding the mode may be.
    pub fn names() -> impl Iterator<Item = &'static str> {
        ROWS.iter().map(|row| row.name)
    }

    /// The stricter of two modes. Crate-private, and the inheritance rule
    /// ([`inherit`](inherit::inherit)) is its only caller: *which of these two
    /// is stricter* is a question about the table, so it is answered inside the
    /// table rather than by anybody holding a mode.
    fn stricter(self, other: Self) -> Self {
        if other.strictness() < self.strictness() {
            other
        } else {
            self
        }
    }

    /// Where this mode's row sits in [`ROWS`], which is ordered strictest
    /// first, so a smaller number is a stricter mode. A mode always points into
    /// `ROWS`, and a mode that somehow did not would be read as the strictest
    /// row, which is the safe direction.
    fn strictness(&self) -> usize {
        ROWS.iter()
            .position(|row| std::ptr::eq(row, self.0))
            .unwrap_or(0)
    }
}

impl Default for Mode {
    /// Cautious: the default answer to a program somebody else wrote is no.
    fn default() -> Self {
        Self(&ROWS[0])
    }
}

/// What the matrix decided on its own, before anybody is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Runs without asking.
    Allow,
    /// The person is asked about this call.
    Ask,
}

/// What to do about one call to `tool`.
///
/// `always` is the tool names the person has said *always for this tool*
/// about. It covers ordinary calls to the tool it names and never a
/// destructive one: "always allow run_command" is a sentence about ordinary
/// commands, and reading it as consent to one that deletes something is putting
/// words in somebody's mouth.
#[must_use]
pub fn verdict(mode: &Mode, tool: &str, intent: &Intent, always: &[String]) -> Verdict {
    if intent.destructive {
        return Verdict::Ask;
    }
    if always.iter().any(|name| name == tool) {
        return Verdict::Allow;
    }
    mode.0.verdict(intent.ability)
}
