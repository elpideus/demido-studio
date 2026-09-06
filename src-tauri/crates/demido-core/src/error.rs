//! One error type, crossing one boundary.
//!
//! Every command the frontend can call returns [`Result`], and a failure
//! reaches the window as a shape the frontend can branch on rather than as a
//! string it has to parse. The variants are deliberately few: a crate with a
//! richer error of its own converts into one of these at the edge, which is
//! what keeps the boundary readable as the workspace grows.

use std::fmt::Display;

/// What went wrong, in the vocabulary the window understands.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A subsystem is not available. Startup never blocks (`AGENTS.md`), so
    /// this is the shape of a subsystem that was reported and skipped rather
    /// than of a fatal condition.
    #[error("{subsystem} is unavailable: {reason}")]
    Unavailable { subsystem: String, reason: String },

    /// The caller asked for something that does not exist.
    #[error("no such {kind}: {id}")]
    NotFound { kind: String, id: String },

    /// The request itself is wrong, and retrying it unchanged will fail again.
    #[error("invalid {what}: {reason}")]
    Invalid { what: String, reason: String },

    /// The operating system, the filesystem or the network said no.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// A subsystem that could not start. The reason is shown to the user, so it
    /// says what happened rather than naming a Rust type.
    pub fn unavailable(subsystem: impl Display, reason: impl Display) -> Self {
        Self::Unavailable {
            subsystem: subsystem.to_string(),
            reason: reason.to_string(),
        }
    }

    /// Something the caller named and this build does not have.
    pub fn not_found(kind: impl Display, id: impl Display) -> Self {
        Self::NotFound {
            kind: kind.to_string(),
            id: id.to_string(),
        }
    }

    /// A request that is wrong rather than unlucky.
    pub fn invalid(what: impl Display, reason: impl Display) -> Self {
        Self::Invalid {
            what: what.to_string(),
            reason: reason.to_string(),
        }
    }

    /// An IO failure, with the thing being attempted attached. `std::io::Error`
    /// alone says "access is denied" without saying to what.
    pub fn io(context: impl Display, source: std::io::Error) -> Self {
        Self::Io {
            context: context.to_string(),
            source,
        }
    }

    /// The machine-readable half, so the frontend branches on a tag rather than
    /// on the wording of a sentence.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unavailable { .. } => "unavailable",
            Self::NotFound { .. } => "not-found",
            Self::Invalid { .. } => "invalid",
            Self::Io { .. } => "io",
        }
    }
}

/// Serialised as `{ kind, message }`: the tag to branch on and the sentence to
/// show. `thiserror` gives the sentence; [`Error::kind`] gives the tag.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("Error", 2)?;
        out.serialize_field("kind", self.kind())?;
        out.serialize_field("message", &self.to_string())?;
        out.end()
    }
}

/// The workspace's result type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_carries_a_tag_and_a_sentence() {
        let error = Error::not_found("model", "gemma-4-E4B-it");
        assert_eq!(error.kind(), "not-found");
        assert_eq!(error.to_string(), "no such model: gemma-4-E4B-it");
    }

    #[test]
    fn an_io_failure_says_what_was_being_attempted() {
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access is denied");
        let error = Error::io("reading the layout", source);
        assert!(error.to_string().starts_with("reading the layout: "));
    }
}
