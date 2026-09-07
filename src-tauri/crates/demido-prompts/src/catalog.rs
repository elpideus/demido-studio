//! The paragraph register, declared once.
//!
//! A paragraph exists here or it does not exist. The declaration carries its
//! own prose, so a paragraph added here becomes editable, searchable and
//! loggable without anyone registering it a second time.
//!
//! The default text lives in `defaults/` as a real file rather than as a string
//! literal. That is not cosmetic, and it is half of hard rule 10: the built-in
//! text and the text an edit writes to disk are then the same bytes, so a
//! change to a default reads in review as a prose diff instead of as a wall of
//! re-indented quotes, and `scripts/check-rules.mjs` can hash the file a
//! measurement was taken against.
//!
//! Nothing here is read-only. `docs/rules/prompts.md` refused that in writing:
//! the brief says "All prompts should be editable", so an entry that is
//! load-bearing declares its [`Dependant`]s instead, and an edit suppresses the
//! claim rather than being refused.

use serde::Serialize;

/// One paragraph: what it is called, what it is for, what it costs to change,
/// and what it says when nobody has edited it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Paragraph {
    /// Stable identifier, `family.variant`. It is the file name on disk and it
    /// appears in the session log, so renaming one is a migration.
    ///
    /// It carries no locale tag, and `docs/rules/prompts.md` refuses one: a
    /// prompt is English, one text per id, and a translation is an edit like
    /// any other. `a_paragraph_id_carries_no_locale_tag` is the guard.
    pub id: &'static str,
    /// The editor's heading for this entry. A label, never payload.
    pub title: &'static str,
    /// One sentence, shown above the editor and indexed for search. A label,
    /// never payload.
    pub summary: &'static str,
    /// Names that may appear as `{{name}}` in the text.
    ///
    /// Declared rather than discovered so that an edit which invents one is
    /// reported to the person who made it. A paragraph whose placeholder
    /// silently never expands is a paragraph that ships `{{target}}` to a
    /// model.
    pub placeholders: &'static [&'static str],
    /// What this wording is load-bearing for, rendered above the field in the
    /// editor. Empty for an entry nothing else depends on.
    pub dependants: &'static [Dependant],
    /// The text this build ships. Never mutated; an edit is a file on disk.
    pub default: &'static str,
}

/// Something that depends on one paragraph's exact wording.
///
/// This is what replaces a read-only flag. The user is told what an edit
/// costs, in a sentence, and then allowed to make it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Dependant {
    /// The sentence the editor renders above the field.
    pub note: &'static str,
    /// What kind of promise this is, which decides what an edit does to it.
    pub kind: Dependency,
}

/// The two ways a paragraph can be load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dependency {
    /// A number this repo publishes was measured against this exact wording.
    ///
    /// Two different costs, and they are deliberately not the same. On a user's
    /// machine an edit **suppresses** the claim: they may degrade their own
    /// classifier, [`crate::Origin::Edited`] records that they did, and nothing
    /// detects that it hurt. In the repo a change to the shipped default is a
    /// release gate: `scripts/check-rules.mjs` fails until the eval is re-run
    /// and the digest at `pinned_in` replaced with the new numbers.
    Measured {
        /// The file holding the pinned digest and the numbers taken against it,
        /// relative to the repository root.
        pinned_in: &'static str,
    },
    /// One wording, used in more than one place, so an edit changes all of them
    /// at once. Nothing is suppressed and nothing is gated: the user is told.
    Shared,
}

/// Ids, so nothing has to spell a paragraph out twice.
pub mod id {
    pub const CAVEMAN_LITE: &str = "caveman.lite";
    pub const CAVEMAN_FULL: &str = "caveman.full";
    pub const CAVEMAN_ULTRA: &str = "caveman.ultra";
    pub const CAVEMAN_WENYAN_LITE: &str = "caveman.wenyan-lite";
    pub const CAVEMAN_WENYAN_FULL: &str = "caveman.wenyan-full";
    pub const CAVEMAN_WENYAN_ULTRA: &str = "caveman.wenyan-ultra";
    pub const CONTEXT_TREE: &str = "context.tree";
    pub const LESSONS_CLASSIFY: &str = "lessons.classify";
}

/// The name of the one placeholder the caveman paragraphs take: what the rule
/// is being applied to, which is either the reply or the reasoning that
/// precedes it. One text with a target beats two texts that have to be kept in
/// step, because the vocabulary rule is the same and only the subject differs.
pub const TARGET: &str = "target";

const CAVEMAN_PLACEHOLDERS: &[&str] = &[TARGET];

/// The workspace root, written out so the model can quote it back to the user
/// without the tree's own first line having to be parsed for it.
pub const ROOT: &str = "root";
/// The rendered tree itself.
pub const TREE: &str = "tree";

const TREE_PLACEHOLDERS: &[&str] = &[ROOT, TREE];

/// The agreement rates in `evals/lessons/` were taken against one exact
/// classifier wording, and the digest of it is recorded beside the corpus.
// not-a-prompt: the sentence the editor renders above the field, never sent.
const MEASURED_AGAINST_THE_LESSON_CORPUS: &[Dependant] = &[Dependant {
    note: "A measurement in `evals/lessons/` was taken against this wording.",
    kind: Dependency::Measured {
        pinned_in: "evals/lessons/AGENTS.md",
    },
}];

const NOTHING_DEPENDS_ON_IT: &[Dependant] = &[];

/// Every paragraph there is.
// not-a-prompt: what a model reads is the `include_str!` default beside each
// entry. The titles and summaries here are the editor's labels, and
// `check-rules.mjs` refuses a `default` that is anything but a file.
pub static CATALOG: &[Paragraph] = &[
    Paragraph {
        id: id::CAVEMAN_LITE,
        title: "Caveman: Lite",
        summary: "Trim the wrapping. Ordinary sentences, with nothing in them that carries no fact.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.lite.md"),
    },
    Paragraph {
        id: id::CAVEMAN_FULL,
        title: "Caveman: Full",
        summary: "Drop the grammar and keep the facts. The level most models handle without losing detail.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.full.md"),
    },
    Paragraph {
        id: id::CAVEMAN_ULTRA,
        title: "Caveman: Ultra",
        summary: "Telegraph style, one fact per line. The cheapest reply that is still true.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.ultra.md"),
    },
    Paragraph {
        id: id::CAVEMAN_WENYAN_LITE,
        title: "Wenyan: Lite",
        summary: "Classical Chinese, whole clauses. The same facts in a script that spends fewer tokens on them.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.wenyan-lite.md"),
    },
    Paragraph {
        id: id::CAVEMAN_WENYAN_FULL,
        title: "Wenyan: Full",
        summary: "Terse Classical Chinese, four to six characters a clause.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.wenyan-full.md"),
    },
    Paragraph {
        id: id::CAVEMAN_WENYAN_ULTRA,
        title: "Wenyan: Ultra",
        summary: "Classical Chinese at two to four characters a line. The cheapest thing this app can ask for.",
        placeholders: CAVEMAN_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/caveman.wenyan-ultra.md"),
    },
    Paragraph {
        id: id::CONTEXT_TREE,
        title: "The project tree",
        summary: "What is in the workspace, sent with every message so the model asks for a file instead of guessing at its name.",
        placeholders: TREE_PLACEHOLDERS,
        dependants: NOTHING_DEPENDS_ON_IT,
        default: include_str!("../defaults/context.tree.md"),
    },
    Paragraph {
        id: id::LESSONS_CLASSIFY,
        title: "The failure classifier",
        summary: "What the task model is asked when a tool call fails: one class from the closed vocabulary, a remedy, and the fragment that decided it.",
        placeholders: &[],
        dependants: MEASURED_AGAINST_THE_LESSON_CORPUS,
        default: include_str!("../defaults/lessons.classify.md"),
    },
];

/// The declaration for one id, if there is one.
///
/// This is also what keeps a caller from reaching a file it should not: an id
/// that is not in the register has no path, so nothing outside `defaults/` and
/// the prompts directory is ever opened.
pub fn paragraph(id: &str) -> Option<&'static Paragraph> {
    CATALOG.iter().find(|paragraph| paragraph.id == id)
}

/// Every `{{name}}` appearing in `text`, in the order they first appear.
///
/// Used to check an edit against its declaration. Deliberately forgiving about
/// what it will match: `{{ target }}` with spaces is the same placeholder,
/// because a person editing prose will write it that way sooner or later.
pub fn placeholders_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() && !found.iter().any(|seen| seen == name) {
            found.push(name.to_owned());
        }
        rest = &after[end + 2..];
    }

    found
}

/// Replace every declared placeholder with its value.
///
/// A placeholder with no value given is left standing rather than blanked, so a
/// missing substitution reaches the eye as `{{target}}` instead of as a
/// sentence that reads fine and means something else.
pub fn fill(text: &str, values: &[(&str, &str)]) -> String {
    let mut out = text.to_owned();
    for (name, value) in values {
        out = out
            .replace(&format!("{{{{{name}}}}}"), value)
            .replace(&format!("{{{{ {name} }}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn every_id_is_unique_and_carries_its_own_prose() {
        let mut seen = std::collections::BTreeSet::new();
        for paragraph in CATALOG {
            assert!(seen.insert(paragraph.id), "duplicate id {}", paragraph.id);
            assert!(!paragraph.title.is_empty(), "{} has no title", paragraph.id);
            assert!(
                !paragraph.summary.is_empty(),
                "{} has no summary",
                paragraph.id
            );
            assert!(
                !paragraph.default.trim().is_empty(),
                "{} ships an empty default",
                paragraph.id
            );
        }
    }

    #[test]
    fn a_default_uses_exactly_the_placeholders_it_declares() {
        // Both directions. An undeclared placeholder never expands and reaches
        // the model as braces; a declared one that is absent is a promise the
        // editor makes to the user and then breaks.
        for paragraph in CATALOG {
            let used = placeholders_in(paragraph.default);
            for name in &used {
                assert!(
                    paragraph.placeholders.contains(&name.as_str()),
                    "{} uses undeclared {{{{{name}}}}}",
                    paragraph.id
                );
            }
            for name in paragraph.placeholders {
                assert!(
                    used.iter().any(|seen| seen == name),
                    "{} declares {{{{{name}}}}} and never uses it",
                    paragraph.id
                );
            }
        }
    }

    #[test]
    fn a_paragraph_id_carries_no_locale_tag() {
        // `docs/rules/prompts.md` refuses a locale axis in writing, and this is
        // where a later i18n effort would land it: `caveman.full.it` beside
        // `caveman.full`. A translation is an edit, recorded as `Edited`, and
        // it carries no special status.
        // A BCP 47 tag: two lowercase letters, optionally a region after a
        // separator. Nothing wider, because a wider rule would refuse
        // `caveman.lite` for being five characters long.
        fn reads_as_a_locale(segment: &str) -> bool {
            let (language, region) = match segment.split_once(['-', '_']) {
                Some((language, region)) => (language, Some(region)),
                None => (segment, None),
            };
            language.len() == 2
                && language.chars().all(|c| c.is_ascii_lowercase())
                && region.is_none_or(|region| {
                    (2..=4).contains(&region.len())
                        && region.chars().all(|c| c.is_ascii_alphabetic())
                })
        }

        for paragraph in CATALOG {
            let last = paragraph.id.rsplit('.').next().unwrap_or_default();
            assert!(
                !reads_as_a_locale(last),
                "{} ends in what reads as a locale tag; a prompt is English, one text per id",
                paragraph.id
            );
        }
    }

    #[test]
    fn a_measured_claim_names_a_file_that_holds_the_pin() {
        // A dependant nobody can follow is a warning with no evidence behind
        // it. The gate in `scripts/check-rules.mjs` reads the same file.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for paragraph in CATALOG {
            for dependant in paragraph.dependants {
                assert!(
                    !dependant.note.is_empty(),
                    "{} has a silent dependant",
                    paragraph.id
                );
                let Dependency::Measured { pinned_in } = dependant.kind else {
                    continue;
                };
                let pin = root.join(pinned_in);
                assert!(
                    pin.exists(),
                    "{} says it was measured in {pinned_in}, which does not exist",
                    paragraph.id
                );
                assert!(
                    std::fs::read_to_string(&pin)
                        .unwrap_or_default()
                        .contains(paragraph.id),
                    "{pinned_in} does not mention {}, so nothing there pins this wording",
                    paragraph.id
                );
            }
        }
    }

    #[test]
    fn filling_leaves_an_unknown_placeholder_visible() {
        let filled = fill("say {{target}} to {{nobody}}", &[(TARGET, "your reply")]);
        assert_eq!(filled, "say your reply to {{nobody}}");
    }

    #[test]
    fn a_placeholder_written_with_spaces_is_the_same_placeholder() {
        assert_eq!(placeholders_in("{{ target }}"), vec!["target".to_owned()]);
        assert_eq!(fill("{{ target }}", &[(TARGET, "this")]), "this");
    }

    #[test]
    fn an_unclosed_placeholder_ends_the_scan_rather_than_hanging() {
        assert_eq!(placeholders_in("{{target}} then {{oops"), vec!["target"]);
    }
}
