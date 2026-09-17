//! What a sub-agent inherits, and the one direction it can move in.
//!
//! `docs/rules/tools.md`: "A sub-agent's offered set is its parent's set. It may
//! narrow it further; it can never add to it, at any depth. The same applies to
//! the mode: a sub-agent runs at its parent's mode or stricter."
//!
//! [`inherit`] is that rule and the whole of it: a parent's [`Resolution`], a
//! child's [`Request`] and the delegation depth in force go in, the child's
//! [`Resolution`] comes out. It is called on the way into every child at every
//! depth rather than asserted once at the top, which is what makes the two
//! controls of S2 ceilings rather than suggestions.
//!
//! ## Three axes and no fourth
//!
//! - **Offered**: `child = parent ∩ requested`, less `delegate_task` where the
//!   chain has reached its limit. A request naming something the parent does not
//!   offer yields a child without it, silently and by construction. There is no
//!   error arm, because widening is not refused here, it is unrepresentable: an
//!   intersection has nowhere to put a name the parent did not have, so there is
//!   no path where it works because somebody forgot a check.
//! - **Mode**: `child = stricter_of(parent, requested)`, and an unknown mode
//!   name still resolves to Cautious, so a name this build has never heard of
//!   can only narrow.
//! - **Level**: one integer, incremented here and nowhere else.
//!
//! **Nothing else is inherited or gated.** The mode gates permissions and
//! nothing else, so parallelism and depth stay independent settings and no
//! fourth ability joins the matrix. `delegate_task` declares `Shell`, and that
//! is the whole of the mode's involvement in delegating.
//!
//! ## The depth is a limit, not a budget carried down
//!
//! What a resolution holds is its **level**: how far down the chain this agent
//! is, counting the conversation as zero, which is the number
//! `demido_trace`'s `agent/delegated` already records as a child's indent. The
//! limit is the ladder's, handed to [`inherit`] by the caller at the moment it
//! dispatches a delegation and never stored, so a depth changed while a
//! conversation is running rules the next delegation
//! ([#64](https://github.com/elpideus/demido-studio/issues/64)). A remaining
//! count decremented on the way down could not do that: it would be a reading
//! of the ladder taken before the change and spent after it.
//!
//! **At the limit the tool is absent**, which is the same absence the picker
//! produces (`tools.md`: *disabled means absent*). No second vocabulary is
//! invented, and a child at the limit is not a child holding a tool that
//! refuses: it is a child that was never shown one.
//!
//! ## It composes with the matrix rather than repeating it
//!
//! A [`Resolution`] carries no verdicts of its own. [`Resolution::verdict`] is
//! [`crate::verdict`] with the child's mode in it, deciding about the same
//! [`Intent`] a parent's call is decided about, so there is one permission shape
//! in the codebase and a child cannot drift from it.

use demido_tools::Intent;

use crate::{verdict, Mode, Verdict};

/// What one session may do: the tools on offer, the mode in force, and how far
/// down the chain of delegations it is.
///
/// Every field is private and there is no setter. A resolution is built once
/// for the conversation ([`Resolution::root`]) and thereafter only by
/// [`inherit`], which is what makes "incremented by this function and by
/// nothing else" a property of the type rather than a convention.
pub struct Resolution {
    /// The tool names on offer, in the order the registry holds them.
    offered: Vec<String>,
    mode: Mode,
    /// How many delegations deep this agent is. The conversation is zero, its
    /// sub-agent is one, and the number is the one the log records as a child's
    /// indent.
    level: u32,
}

impl Resolution {
    /// The conversation's own resolution: what the ladder resolved, at level
    /// zero.
    ///
    /// The only way to make one that is not a child of another, and it is the
    /// **main session's** alone. Minting a root for a sub-agent would hand it
    /// an offered set and a level nothing narrowed, which is the one route
    /// round this module; it is reading the ladder where the parent should have
    /// been read, and it is a review finding against whoever writes it, the
    /// same way probing [`crate::verdict`] to learn the mode is. Below the top
    /// there is [`inherit`] and nothing else.
    #[must_use]
    pub fn root(offered: Vec<String>, mode: Mode) -> Self {
        Self {
            offered,
            mode,
            level: 0,
        }
    }

    /// The tools on offer, in the parent's order.
    #[must_use]
    pub fn offered(&self) -> &[String] {
        &self.offered
    }

    /// How many delegations deep this agent is. Zero is the conversation.
    #[must_use]
    pub fn level(&self) -> u32 {
        self.level
    }

    /// Whether this session may delegate under a depth of `depth`.
    ///
    /// What decides that `delegate_task` is in the offered set or absent from
    /// it ([#64](https://github.com/elpideus/demido-studio/issues/64)). A
    /// question asked of the number rather than of the mode: the mode gates
    /// permissions and nothing else.
    ///
    /// `depth` is the ladder's, read where the delegation is dispatched. It is
    /// a parameter rather than a field for that reason: a resolution that
    /// remembered a depth would answer with the reading taken when it was
    /// built.
    #[must_use]
    pub fn may_delegate(&self, depth: u32) -> bool {
        self.level < depth
    }

    /// What the matrix decides about one call under this resolution.
    ///
    /// The composition with S2: the same [`Intent`], through the same
    /// [`crate::verdict`], with this session's mode. A child has no permission
    /// shape of its own to drift from its parent's.
    #[must_use]
    pub fn verdict(&self, tool: &str, intent: &Intent, always: &[String]) -> Verdict {
        verdict(&self.mode, tool, intent, always)
    }
}

/// What a child asks for, on the two axes it may ask about.
///
/// [`None`] on either is the child asking for nothing there, which is its
/// parent's. Neither field can widen anything: the offered names are
/// intersected and the mode is taken at its stricter, so a request is only ever
/// read as *less*.
///
/// The depth is not here, and deliberately: it is not a thing a child asks for,
/// it is the limit the chain is under, and so it is [`inherit`]'s third
/// argument.
#[derive(Default)]
pub struct Request {
    offered: Option<Vec<String>>,
    /// The mode's stored name, resolved through [`Mode::named`] like any other,
    /// so a name this build has never heard of is Cautious here too.
    mode: Option<String>,
}

impl Request {
    /// A request that asks for nothing: the child runs with exactly what its
    /// parent had, one level lower.
    #[must_use]
    pub fn inheriting() -> Self {
        Self::default()
    }

    /// Ask for these tools. What is not also the parent's is not the child's.
    #[must_use]
    pub fn narrowed_to(mut self, names: Vec<String>) -> Self {
        self.offered = Some(names);
        self
    }

    /// Ask to run at the mode stored under this name, which is honoured only
    /// when it is stricter than the parent's.
    #[must_use]
    pub fn at_mode(mut self, name: impl Into<String>) -> Self {
        self.mode = Some(name.into());
        self
    }
}

/// The inheritance rule: a parent's resolution, a child's request and the depth
/// in force in, the child's resolution out.
///
/// Pure, total, and the only producer of a resolution below the root. `depth`
/// is the ladder's as it stands at this dispatch, and it is read rather than
/// remembered: see the module docs for that and for the three axes.
#[must_use]
pub fn inherit(parent: &Resolution, request: &Request, depth: u32) -> Resolution {
    let mut offered = match request.offered.as_ref() {
        // An intersection in the parent's order. A name the parent does not
        // offer has nowhere to land, which is the whole of the widening
        // defence.
        Some(requested) => parent
            .offered
            .iter()
            .filter(|name| requested.contains(name))
            .cloned()
            .collect(),
        None => parent.offered.clone(),
    };

    let mode = match request.mode.as_deref() {
        Some(name) => parent.mode.stricter(Mode::named(name)),
        None => parent.mode,
    };

    // Saturating rather than wrapping: a chain long enough to overflow a `u32`
    // stays at the bottom rather than arriving back at the top. It would take
    // four billion nested turns to reach, and the one place the depth is
    // enforced is not where a wrap may happen.
    let level = parent.level.saturating_add(1);

    // The rule with the number in it, and the same comparison
    // [`Resolution::may_delegate`] makes. A child with no depth left is a child
    // the tool is **absent** from, exactly as a tool the picker switched off is
    // absent, rather than a child holding one that answers with a refusal.
    if level >= depth {
        offered.retain(|name| name != demido_tools::DelegateTask::NAME);
    }

    Resolution {
        offered,
        mode,
        level,
    }
}
