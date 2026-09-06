//! The composition root.
//!
//! `docs/rules/tiles.md`: "The composition root names exactly one
//! implementation per trait, in one place." This is that place. Swapping a
//! compile-time tile is a single line here and a recompile, and the value of
//! that is only real while it stays a single line, so a trait is never
//! constructed anywhere else and no crate reaches for another crate's concrete
//! type.
//!
//! It is empty today, and deliberately so: the first trait arrives with the
//! `Backend` on the next ticket of this slice. What it establishes now is that
//! there is one root rather than a `new()` in every crate, which is the part
//! that is expensive to retrofit across two dozen directories later.

/// Every subsystem, wired once.
///
/// Built at startup, handed to Tauri as managed state, and read from there by
/// every command. A field is a trait object, so a test builds a `Wiring` of
/// fakes and the application builds one of real implementations, with the
/// commands unable to tell the difference.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Wiring {}

impl Wiring {
    /// The application's wiring: one implementation per trait.
    ///
    /// Fallible from the first line, because it will be. A subsystem that
    /// cannot start is reported and skipped rather than fatal (`AGENTS.md`),
    /// so a failure here is reserved for the case where there would be no
    /// window worth opening at all.
    pub fn assemble() -> crate::Result<Self> {
        Ok(Self {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_assembles() {
        assert!(Wiring::assemble().is_ok());
    }
}
