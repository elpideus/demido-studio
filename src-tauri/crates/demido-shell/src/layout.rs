//! What the desk looked like last time, and the number that says whether this
//! build still understands it.
//!
//! The value is deliberately small. The desk this slice arranges has one
//! adjustable thing on it, which is the edge the rail is docked to
//! (`design/shell.md`), and a layout type that declared fields for panels
//! nothing can yet open would be a shape nothing can be held to. [`GENERATION`]
//! is what makes growing it cheap: a file written by an older shape is not
//! migrated and not repaired, it is dropped, and the desk opens on the default.

use serde::{Deserialize, Serialize};

/// The shape of a written layout.
///
/// Bumped whenever [`Shell`] stops being able to read what the previous shape
/// wrote. A file whose generation is not this one is discarded silently, so a
/// bump costs the user their arrangement once and costs the code nothing: there
/// is no migration path to write, no half-migrated file to debug, and no way
/// for a layout to keep a window from opening.
pub const GENERATION: u32 = 1;

/// Which edge the rail is docked to.
///
/// Brief B42: "a VSCode-like Icons-only sidebar"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Where VS Code puts it, and where Demido opens with it.
    #[default]
    Left,
    Right,
}

impl Side {
    /// The other edge. The rail has exactly two homes, so moving it is a
    /// toggle rather than a coordinate.
    #[must_use]
    pub fn flipped(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// The arrangement of the desk.
///
/// `Default` is the desk a fresh profile gets, and it is also what a layout
/// that will not load falls back to, which is why it is a `Default` rather than
/// a constructor with arguments: there must be no way to reach for the fallback
/// and get it wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Shell {
    /// The edge the rail is docked to.
    pub rail: Side,
}

/// A layout as it sits in `shell.json`: the arrangement, plus the generation
/// that wrote it.
///
/// Separate from [`Shell`] on purpose. The generation is a fact about the
/// **file**, not about the desk, and the window is never told it: a frontend
/// that could read the number would eventually branch on it, and then the
/// discard rule would have a second implementation living in TypeScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub generation: u32,
    #[serde(flatten)]
    pub shell: Shell,
}

impl From<Shell> for Written {
    fn from(shell: Shell) -> Self {
        Self {
            generation: GENERATION,
            shell,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn a_fresh_desk_has_the_rail_on_the_left() {
        assert_eq!(Shell::default().rail, Side::Left);
    }

    #[test]
    fn the_rail_has_two_homes_and_moving_it_is_a_toggle() {
        assert_eq!(Side::Left.flipped(), Side::Right);
        assert_eq!(Side::Right.flipped().flipped(), Side::Right);
    }

    /// The window is handed the arrangement and never the generation. If this
    /// ever fails, the discard rule has grown a second home in the frontend.
    #[test]
    fn the_arrangement_the_window_sees_carries_no_generation() {
        let json = serde_json::to_string(&Shell::default()).expect("serialised");
        assert_eq!(json, r#"{"rail":"left"}"#);
    }

    #[test]
    fn a_written_layout_stamps_the_generation_it_was_written_by() {
        let written = Written::from(Shell { rail: Side::Right });
        let json = serde_json::to_string(&written).expect("serialised");
        assert!(json.contains(r#""generation":1"#), "{json}");
        assert!(json.contains(r#""rail":"right""#), "{json}");
    }
}
