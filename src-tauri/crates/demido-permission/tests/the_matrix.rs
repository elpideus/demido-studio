//! The matrix, as a table.
//!
//! It is a pure function, so it needs no seam and no fake: every ability
//! against every mode, the destructive floor under all of them, a mode name
//! this build has never heard of, and *always for this tool* failing to cover a
//! call it cannot put back. The expected grid is written out here a second time
//! on purpose, so a row edited in the crate has to be edited in the test too.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use demido_permission::{verdict, Mode, Verdict};
use demido_tools::{Ability, Context, DeleteFile, Intent, RunCommand, Tool, Workspace, WriteFile};
use serde_json::json;

const ABILITIES: [Ability; 4] = [
    Ability::Read,
    Ability::Write,
    Ability::Shell,
    Ability::Network,
];

fn intent(ability: Ability, destructive: bool) -> Intent {
    Intent {
        ability,
        summary: "do a thing".into(),
        destructive,
        touches: Vec::new(),
    }
}

/// What each mode does with an ordinary call of each ability, in the order of
/// [`ABILITIES`]. `docs/rules/tools.md` draws the same table.
fn expected(mode: &str) -> [Verdict; 4] {
    use Verdict::{Allow, Ask};
    match mode {
        "cautious" => [Allow, Ask, Ask, Ask],
        "balanced" => [Allow, Allow, Ask, Ask],
        "autonomous" => [Allow, Allow, Allow, Allow],
        other => panic!("the test has no row for {other}"),
    }
}

/// Every ability's verdict under one mode, for an ordinary call nobody has
/// said always about.
fn row(mode: &Mode) -> [Verdict; 4] {
    ABILITIES.map(|ability| verdict(mode, "some_tool", &intent(ability, false), &[]))
}

#[test]
fn there_are_three_modes_and_cautious_is_the_first() {
    assert_eq!(
        Mode::names().collect::<Vec<_>>(),
        ["cautious", "balanced", "autonomous"]
    );
}

#[test]
fn every_ability_against_every_mode() {
    for name in Mode::names() {
        assert_eq!(
            row(&Mode::named(name)),
            expected(name),
            "{name} decided an ordinary call differently from the table"
        );
    }
}

#[test]
fn a_destructive_call_asks_in_every_mode_including_autonomous() {
    // The floor under every row. Whatever ability the call declares, and in
    // the mode that approves everything else.
    for name in Mode::names() {
        for ability in ABILITIES {
            assert_eq!(
                verdict(&Mode::named(name), "some_tool", &intent(ability, true), &[]),
                Verdict::Ask,
                "{name} let a destructive {ability:?} call run without asking"
            );
        }
    }
}

#[test]
fn always_for_this_tool_cannot_cover_a_destructive_call() {
    let always = vec!["run_command".to_owned()];

    for name in Mode::names() {
        let mode = Mode::named(name);
        assert_eq!(
            verdict(
                &mode,
                "run_command",
                &intent(Ability::Shell, false),
                &always
            ),
            Verdict::Allow,
            "{name}: always covers an ordinary call to the tool it names"
        );
        assert_eq!(
            verdict(&mode, "run_command", &intent(Ability::Shell, true), &always),
            Verdict::Ask,
            "{name}: always was read as consent to something that cannot be put back"
        );
    }
}

#[test]
fn always_covers_the_tool_it_names_and_no_other() {
    let always = vec!["write_file".to_owned()];
    let cautious = Mode::named("cautious");

    assert_eq!(
        verdict(
            &cautious,
            "write_file",
            &intent(Ability::Write, false),
            &always
        ),
        Verdict::Allow
    );
    assert_eq!(
        verdict(
            &cautious,
            "run_command",
            &intent(Ability::Shell, false),
            &always
        ),
        Verdict::Ask
    );
}

#[test]
fn a_mode_name_this_build_has_never_heard_of_is_cautious() {
    // A profile written by a newer build must never be read as permission to
    // do more. The names are matched exactly, so a capitalised or padded
    // spelling is a name this build has never heard of too.
    for name in [
        "",
        "unsupervised",
        "Autonomous",
        "AUTONOMOUS",
        " balanced",
        "yolo",
    ] {
        assert_eq!(
            row(&Mode::named(name)),
            expected("cautious"),
            "{name:?} was read as something other than cautious"
        );
    }
    assert_eq!(row(&Mode::default()), expected("cautious"));
}

/// The tools the matrix will actually meet, declaring against a real project.
fn declared(tool: &dyn Tool, arguments: serde_json::Value) -> Intent {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
    let workspace = Workspace::open(dir.path()).expect("a workspace");
    tool.intent(&arguments, &Context::over(&workspace))
}

#[test]
fn in_balanced_a_shell_command_asks_and_a_file_write_does_not() {
    let balanced = Mode::named("balanced");

    let command = declared(&RunCommand, json!({ "command": "cargo build" }));
    let write = declared(
        &WriteFile,
        json!({ "path": "notes.md", "content": "written" }),
    );

    assert_eq!(
        verdict(&balanced, "run_command", &command, &[]),
        Verdict::Ask
    );
    assert_eq!(
        verdict(&balanced, "write_file", &write, &[]),
        Verdict::Allow
    );
}

#[test]
fn deleting_a_file_asks_in_autonomous_even_when_always_names_it() {
    let delete = declared(&DeleteFile, json!({ "path": "README.md" }));
    let always = vec!["delete_file".to_owned()];

    assert_eq!(
        verdict(&Mode::named("autonomous"), "delete_file", &delete, &always),
        Verdict::Ask
    );
}

#[test]
fn no_prompt_demido_ships_names_a_mode() {
    // The mode is never prose. Every paragraph and every tool document is what
    // a model can be sent, so none of them may describe the mode to it: a
    // permission the model is told about is one that depends on the model.
    let texts = demido_prompts::CATALOG
        .iter()
        .map(|paragraph| (paragraph.id, paragraph.default))
        .chain(
            demido_prompts::tools::TOOLS
                .iter()
                .map(|entry| (entry.name, entry.default)),
        );

    for (id, text) in texts {
        let text = text.to_lowercase();
        for name in Mode::names() {
            assert!(!text.contains(name), "{id} mentions the {name} mode");
        }
    }
}
