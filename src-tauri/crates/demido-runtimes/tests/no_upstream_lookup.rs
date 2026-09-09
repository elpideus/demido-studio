//! "Demido never queries upstream for a version, on any trigger."
//!
//! Nine of this ticket's acceptance criteria are things the crate does, and
//! each has a test that does it. This one is a thing the crate must never do,
//! and absence is not something the other suites can assert: they would all
//! still pass on the day somebody adds a release feed. `scripts/check-rules.mjs`
//! makes the argument this file is built on: "A rule an agent can violate
//! without CI noticing is a rule that will be violated."
//!
//! So the invariant is written as something checkable rather than as a
//! promise: **this crate contains no URL at all.** Every address it fetches
//! arrives through `Fetchable::url`, from a pin that shipped inside the build
//! (`demido_catalog::Archive::url`) or from a path the user pointed at. A
//! literal here would be the first step of the thing section 6 of
//! `docs/rules/runtimes.md` refuses: GitHub's releases API and Chrome for
//! Testing's `last-known-good-versions-with-downloads.json` would both answer,
//! and what they would return is a version nobody has run.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

/// The crate's own source, which is the scope of this rule. The tests are not
/// scanned: `tests/` legitimately points the one fetch implementation at
/// `127.0.0.1`, and that is how resume and cancel are proved without a mock.
fn sources() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("the crate has a src directory")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "rs"))
        .collect();
    files.sort();
    assert!(files.len() > 4, "the scan found almost nothing to scan");
    files
}

/// Everything that is not a comment.
///
/// Comments are stripped rather than searched, because the doc comments here
/// cite issues and rules files by URL on purpose: an explanation that names
/// where a decision was made is the opposite of the thing being refused.
fn code_only(source: &str) -> String {
    let mut code = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find("/*") {
        let (before, after) = rest.split_at(open);
        code.push_str(before);
        rest = match after.find("*/") {
            Some(close) => &after[close + 2..],
            None => "",
        };
    }
    code.push_str(rest);

    code.lines()
        .map(strip_line_comment)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Everything before a line comment, where `//` inside `https://` is not one.
///
/// Naively cutting at the first `//` reads `"https://api.github.com/..."` as
/// the string `"https:` followed by a comment, which silently hides the half
/// of the line that matters. Found by injecting a release feed into the crate
/// and watching only one of these two tests notice.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut at = 0;
    while let Some(found) = line[at..].find("//") {
        let start = at + found;
        if start == 0 || bytes[start - 1] != b':' {
            return &line[..start];
        }
        at = start + 2;
    }
    line
}

#[test]
fn the_crate_that_fetches_holds_no_address_of_its_own() {
    let mut found = Vec::new();
    for file in sources() {
        let source = std::fs::read_to_string(&file).expect("read the source");
        for (number, line) in code_only(&source).lines().enumerate() {
            if line.contains("http") {
                found.push(format!(
                    "{}:{}  {}",
                    file.file_name().unwrap_or_default().to_string_lossy(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "every address comes from a pin the caller hands over, so a URL in \
         this crate is either a version lookup or the start of one:\n{}",
        found.join("\n")
    );
}

/// The other half, and the one that survives a URL arriving through a
/// constant: nothing here reads a release index by name.
#[test]
fn nothing_asks_upstream_what_the_newest_release_is() {
    // The two feeds section 6 of `docs/rules/runtimes.md` names, and the
    // shapes an answer would arrive in.
    const FEEDS: &[&str] = &[
        "api.github.com",
        "releases/latest",
        "last-known-good",
        "LatestRelease",
        "check_for_updates",
    ];

    let mut found = Vec::new();
    for file in sources() {
        let code = code_only(&std::fs::read_to_string(&file).expect("read the source"));
        for feed in FEEDS {
            if code.contains(feed) {
                found.push(format!("{}: {feed}", file.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a pin moves because a new Demido exists, not because a feed answered:\n{}",
        found.join("\n")
    );
}
