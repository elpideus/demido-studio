//! What a sub-agent inherits, as a table.
//!
//! `docs/rules/tools.md`: "A sub-agent's offered set is its parent's set. It may
//! narrow it further; it can never add to it, at any depth. The same applies to
//! the mode: a sub-agent runs at its parent's mode or stricter."
//!
//! Like the matrix beside it this is a pure function, so it needs no seam and no
//! fake: every parent set against every requested set, every parent mode against
//! every requested mode including a name this build has never heard of, and the
//! same narrowing run down a chain of three. The expected rule is written out
//! here a second time on purpose, as an intersection and a rank, so that the
//! crate cannot be the only place that says what inheriting means.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use demido_permission::{inherit, verdict, Mode, Request, Resolution, Verdict};
use demido_tools::{Ability, Intent};

const ABILITIES: [Ability; 4] = [
    Ability::Read,
    Ability::Write,
    Ability::Shell,
    Ability::Network,
];

/// The tools the sets below are drawn from. Three is enough for every subset to
/// be enumerated and for one of them to be the one the mode actually gates.
const UNIVERSE: [&str; 3] = ["read_file", "write_file", "run_command"];

fn intent(ability: Ability, destructive: bool) -> Intent {
    Intent {
        ability,
        summary: "do a thing".into(),
        destructive,
        touches: Vec::new(),
    }
}

/// Every subset of [`UNIVERSE`], in the universe's own order.
fn sets() -> Vec<Vec<String>> {
    (0..(1 << UNIVERSE.len()))
        .map(|bits: usize| {
            UNIVERSE
                .iter()
                .enumerate()
                .filter(|(index, _)| bits & (1 << index) != 0)
                .map(|(_, name)| (*name).to_owned())
                .collect()
        })
        .collect()
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|name| (*name).to_owned()).collect()
}

/// What a mode decides about an ordinary call of each ability. The only way to
/// tell two modes apart, on purpose: a `Mode` has no equality and nothing but
/// the matrix reads one.
fn row(mode: &Mode) -> [Verdict; 4] {
    ABILITIES.map(|ability| verdict(mode, "some_tool", &intent(ability, false), &[]))
}

/// The same reading of a resolution, so that a child's mode is only ever
/// observed through the verdicts it produces.
fn row_of(resolution: &Resolution) -> [Verdict; 4] {
    ABILITIES.map(|ability| resolution.verdict("some_tool", &intent(ability, false), &[]))
}

/// How strict a stored name is, strictest first. Unknown is Cautious, per S2.
fn rank(name: &str) -> usize {
    match name {
        "balanced" => 1,
        "autonomous" => 2,
        _ => 0,
    }
}

/// Every name the matrix knows, plus names it does not.
const REQUESTED_MODES: [&str; 8] = [
    "cautious",
    "balanced",
    "autonomous",
    "",
    "Autonomous",
    " balanced",
    "unsupervised",
    "yolo",
];

/// The levels of a chain the tables below are run at. The brief's own example
/// is a chain of three, and the rule is asserted at every one of them rather
/// than once at the top, because that is where it is called.
const LEVELS: [u32; 3] = [1, 2, 3];

/// A parent that is `level` levels down a chain, holding `offered` at `mode`.
///
/// Every level above it asked for nothing, so the parent is the same parent at
/// every depth and a table run over it is the same table. The root starts deep
/// enough that each of [`LEVELS`] is a child of a parent that still had depth.
fn at_level(level: u32, offered: Vec<String>, mode: Mode) -> Resolution {
    let mut resolution = Resolution::root(offered, mode, LEVELS.len() as u32 + 1);
    for _ in 0..level {
        resolution = inherit(&resolution, &Request::inheriting());
    }
    resolution
}

#[test]
fn every_parent_set_against_every_requested_set_at_every_depth() {
    for level in LEVELS {
        for parent_set in sets() {
            let parent = at_level(level, parent_set.clone(), Mode::named("autonomous"));
            for requested in sets() {
                let child = inherit(
                    &parent,
                    &Request::inheriting().narrowed_to(requested.clone()),
                );

                let expected: Vec<String> = parent_set
                    .iter()
                    .filter(|name| requested.contains(name))
                    .cloned()
                    .collect();
                assert_eq!(
                    child.offered(),
                    expected,
                    "at depth {level}, parent {parent_set:?} asked for {requested:?}"
                );
            }
        }
    }
}

#[test]
fn a_child_never_offers_what_its_parent_did_not_at_any_depth() {
    for level in LEVELS {
        for parent_set in sets() {
            let parent = at_level(level, parent_set.clone(), Mode::default());
            for requested in sets() {
                let child = inherit(&parent, &Request::inheriting().narrowed_to(requested));
                for name in child.offered() {
                    assert!(
                        parent_set.contains(name),
                        "at depth {level}, {name} reached a child of {parent_set:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_request_naming_a_tool_the_parent_does_not_offer_is_a_child_without_it() {
    // Silently and by construction. There is no error to return: `inherit`
    // answers with a resolution, so a widening request is not refused, it is
    // unrepresentable.
    let parent = Resolution::root(names(&["read_file"]), Mode::named("autonomous"), 2);
    let child = inherit(
        &parent,
        &Request::inheriting().narrowed_to(names(&["read_file", "run_command"])),
    );

    assert_eq!(child.offered(), names(&["read_file"]));
}

#[test]
fn narrowing_further_is_allowed() {
    let parent = Resolution::root(
        names(&["read_file", "write_file", "run_command"]),
        Mode::named("autonomous"),
        2,
    );
    let child = inherit(
        &parent,
        &Request::inheriting().narrowed_to(names(&["read_file"])),
    );

    assert_eq!(child.offered(), names(&["read_file"]));
}

#[test]
fn a_child_that_asks_for_nothing_gets_its_parents_set() {
    let parent = Resolution::root(names(&["read_file", "run_command"]), Mode::default(), 2);
    let child = inherit(&parent, &Request::inheriting());

    assert_eq!(child.offered(), parent.offered());
}

#[test]
fn every_parent_mode_against_every_requested_mode_at_every_depth() {
    for level in LEVELS {
        for parent_name in Mode::names() {
            let parent = at_level(level, names(&UNIVERSE), Mode::named(parent_name));
            for requested in REQUESTED_MODES {
                let child = inherit(&parent, &Request::inheriting().at_mode(requested));

                let stricter = if rank(requested) < rank(parent_name) {
                    requested
                } else {
                    parent_name
                };
                assert_eq!(
                    row_of(&child),
                    row(&Mode::named(stricter)),
                    "at depth {level}, {parent_name} delegating at {requested:?} \
                     did not run at {stricter}"
                );
            }
        }
    }
}

#[test]
fn a_mode_name_this_build_has_never_heard_of_cannot_widen() {
    // It resolves to Cautious, which is the strictest row, so it can only ever
    // narrow. A profile written by a newer build is never read as permission to
    // do more, at any depth.
    for parent_name in Mode::names() {
        let parent = Resolution::root(names(&UNIVERSE), Mode::named(parent_name), 3);
        let child = inherit(&parent, &Request::inheriting().at_mode("unsupervised"));

        assert_eq!(row_of(&child), row(&Mode::named("cautious")));
    }
}

#[test]
fn a_child_that_asks_for_no_mode_runs_at_its_parents() {
    for name in Mode::names() {
        let parent = Resolution::root(names(&UNIVERSE), Mode::named(name), 3);
        let child = inherit(&parent, &Request::inheriting());

        assert_eq!(row_of(&child), row(&Mode::named(name)));
    }
}

#[test]
fn the_narrowing_holds_identically_at_depth_one_two_and_three() {
    // One chain, three levels, and every level asking for everything back.
    let root = Resolution::root(
        names(&["read_file", "write_file"]),
        Mode::named("balanced"),
        3,
    );
    let everything = || {
        Request::inheriting()
            .narrowed_to(names(&UNIVERSE))
            .at_mode("autonomous")
    };

    let first = inherit(&root, &everything());
    assert_eq!(first.offered(), names(&["read_file", "write_file"]));
    assert_eq!(row_of(&first), row(&Mode::named("balanced")));
    assert_eq!(first.depth(), 2);

    // The middle of the chain sheds a tool and a mode. Both are ceilings from
    // here down, and neither can be taken back below.
    let second = inherit(
        &first,
        &Request::inheriting()
            .narrowed_to(names(&["read_file"]))
            .at_mode("cautious"),
    );
    assert_eq!(second.offered(), names(&["read_file"]));
    assert_eq!(row_of(&second), row(&Mode::named("cautious")));
    assert_eq!(second.depth(), 1);

    let third = inherit(&second, &everything());
    assert_eq!(third.offered(), names(&["read_file"]));
    assert_eq!(row_of(&third), row(&Mode::named("cautious")));
    assert_eq!(third.depth(), 0);
}

#[test]
fn depth_is_one_integer_decremented_on_the_way_down() {
    let root = Resolution::root(names(&UNIVERSE), Mode::default(), 2);
    assert_eq!(root.depth(), 2);
    assert!(root.may_delegate());

    let first = inherit(&root, &Request::inheriting());
    assert_eq!(first.depth(), 1);
    assert!(first.may_delegate());

    let second = inherit(&first, &Request::inheriting());
    assert_eq!(second.depth(), 0);
    assert!(!second.may_delegate());

    // Nothing else moves it, and it does not wrap: a chain that kept going
    // past the limit stays at the limit rather than becoming unlimited.
    let third = inherit(&second, &Request::inheriting());
    assert_eq!(third.depth(), 0);
    assert!(!third.may_delegate());
}

#[test]
fn a_child_rules_a_call_the_way_the_matrix_does() {
    // The composition with S2: a resolution answers about an `Intent`, through
    // `verdict`, rather than carrying a second permission shape of its own.
    let parent = Resolution::root(names(&UNIVERSE), Mode::named("balanced"), 3);
    let child = inherit(&parent, &Request::inheriting());
    let balanced = Mode::named("balanced");

    for ability in ABILITIES {
        for destructive in [false, true] {
            let intent = intent(ability, destructive);
            assert_eq!(
                child.verdict("run_command", &intent, &[]),
                verdict(&balanced, "run_command", &intent, &[]),
                "a child decided a {ability:?} call differently from the matrix"
            );
        }
    }
}

#[test]
fn the_destructive_floor_survives_every_level_of_a_chain() {
    // Story 15: *always for this tool* granted on the delegation does not reach
    // the sub-agent's own destructive calls.
    let always = names(&["delete_file"]);
    let mut resolution = Resolution::root(names(&UNIVERSE), Mode::named("autonomous"), 3);

    for depth in 0..4 {
        assert_eq!(
            resolution.verdict("delete_file", &intent(Ability::Write, true), &always),
            Verdict::Ask,
            "a destructive call ran without asking {depth} levels down"
        );
        resolution = inherit(&resolution, &Request::inheriting());
    }
}
