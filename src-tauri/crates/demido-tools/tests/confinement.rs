//! Every tool in the Files group, held to the `Tool` contract.
//!
//! The contract itself is `demido_tools::contract`, beside the trait, per
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md). This is the file
//! that calls it with each implementation, which is what makes those five
//! implementations rather than five structs that happen to compile: a sixth
//! tool added without going through `Context::resolve` fails here on the commit
//! that adds it.
//!
//! Everything stands on a real `tempfile::TempDir`. These tools deliberately
//! get no filesystem seam: `read_file` against a real directory is a better
//! test than `read_file` against a trait, and confinement is only interesting
//! against real paths, real symlinks and the real path shapes Windows accepts.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_tools::contract::{holds_for, Rig};
use demido_tools::{files, Call, Registry, Workspace};
use serde_json::{json, Value};

/// A project, a secret outside it, and both directories kept alive.
fn rig() -> (tempfile::TempDir, tempfile::TempDir, Workspace) {
    let project = tempfile::tempdir().expect("a directory");
    let outside = tempfile::tempdir().expect("a directory");
    Rig::plant(project.path(), outside.path());

    let workspace = Workspace::open(project.path()).expect("a workspace");
    (project, outside, workspace)
}

/// A call, with only the arguments this tool's schema declares.
///
/// A tool's schema closes itself, so handing every tool the same arguments
/// would have most of them refused for a property they never declared, and a
/// refusal about an argument proves nothing about a path.
fn call(spec: &demido_tools::Spec, given: Value) -> Call {
    let declared = spec.parameters["properties"].as_object().expect("declared");
    let kept: serde_json::Map<String, Value> = given
        .as_object()
        .expect("an object")
        .iter()
        .filter(|(name, _)| declared.contains_key(*name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    Call {
        id: format!("call-{}", spec.name),
        name: spec.name.clone(),
        arguments: Value::Object(kept).to_string(),
    }
}

#[tokio::test]
async fn every_tool_in_the_files_group_keeps_the_contract() {
    // Read off `files()` rather than written out, so the group and the suite
    // cannot come apart. A fresh rig per tool, because the contract asserts
    // what is left outside afterwards.
    for tool in files() {
        let (_project, outside, workspace) = rig();
        holds_for(
            tool.as_ref(),
            Rig {
                workspace: &workspace,
                outside: outside.path(),
            },
        )
        .await;
    }
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "making a symlink on Windows needs developer mode or elevation"
)]
async fn no_tool_follows_a_symlink_out_of_the_workspace() {
    // The oldest way around a prefix comparison, and one an ordinary package
    // manager can create without anybody meaning anything by it. It is here
    // rather than in the contract because creating one is a privilege the
    // contract's callers cannot all be assumed to have.
    let (project, outside, workspace) = rig();

    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), project.path().join("escape")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(outside.path(), project.path().join("escape")).unwrap();

    let registry = Registry::of_files(Some(workspace));
    for spec in registry.offered() {
        let outcome = registry
            .run(&call(
                &spec,
                json!({
                    "path": "escape/secret.txt",
                    "content": "planted by a tool",
                    "text": "passphrase",
                }),
            ))
            .await;

        assert!(
            outcome.is_err(),
            "{} followed a link out of the workspace: {outcome:?}",
            spec.name
        );
    }

    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
        "the passphrase\n"
    );
}

#[tokio::test]
async fn a_path_inside_the_workspace_still_works_however_deep_it_is() {
    // The other half of every rule above. A confinement check that refused
    // everything would pass all of them, and 260 characters is where Windows
    // changes its mind about paths.
    let (_project, _outside, workspace) = rig();
    let registry = Registry::of_files(Some(workspace));
    let deep: String = (0..20).map(|n| format!("directory-{n:03}/")).collect();

    let named = |name: &str, arguments: Value| Call {
        id: format!("call-{name}"),
        name: name.to_owned(),
        arguments: arguments.to_string(),
    };

    let written = registry
        .run(&named(
            "write_file",
            json!({ "path": format!("{deep}note.txt"), "content": "tallow\n" }),
        ))
        .await
        .expect("a long path inside the workspace");
    assert!(written.contains("Created"), "{written}");

    let read = registry
        .run(&named(
            "read_file",
            json!({ "path": format!("{deep}note.txt") }),
        ))
        .await
        .expect("read back");
    assert!(read.contains("tallow"), "{read}");

    let found = registry
        .run(&named("search_files", json!({ "text": "tallow" })))
        .await
        .expect("searched");
    assert!(found.contains("note.txt"), "{found}");

    let gone = registry
        .run(&named(
            "delete_file",
            json!({ "path": format!("{deep}note.txt") }),
        ))
        .await
        .expect("deleted");
    assert!(gone.contains("Deleted"), "{gone}");
}
