//! What every tool promises, whatever it does.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): a trait's contract
//! suite is its second file, it takes any implementation and exercises what the
//! trait claims, and an implementation that does not call it is not an
//! implementation. `Tool` is an unusual trait to hold that way, because its
//! implementations are not interchangeable: nobody swaps `read_file` for
//! `write_file`. What they share is not a job, it is a set of promises, and
//! those are exactly the promises that decide whether the rest of Demido can
//! trust a tool it did not write.
//!
//! Three of them, and each is a rule from somewhere else made executable:
//!
//! 1. **A schema is a shape and carries no prose.** Hard rule 10 and
//!    [`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md):
//!    a tool's description and its parameter prose are register entries with an
//!    id, a default file, a hash and an `Origin`. A tool that types one back
//!    into its `parameters()` body puts model-facing text somewhere nobody can
//!    edit, version or log.
//! 2. **A schema closes itself.** `additionalProperties: false` is what lets
//!    [`crate::arguments::faults`] refuse a property the model invented rather
//!    than pass it on to a tool that will do something unpredictable with it.
//! 3. **No path reaches the filesystem without `Workspace::resolve`.** Every
//!    spelling of somewhere else is refused, nothing outside is touched, and an
//!    `Intent` never claims to touch a path it could not reach. This is the one
//!    the type system makes easy and cannot enforce: `Resolved` makes the
//!    correct route the shortest one, and a tool determined to call `std::fs`
//!    with a raw string still can.
//!
//! The suite was written with five implementations and before the sixth, which
//! is the point of the rule. `run_command`
//! ([#51](https://github.com/elpideus/demido-studio/issues/51)) was the sixth
//! and is held to it by its `cwd`; a tool named by a server Demido did not
//! write inherits it the same way, by calling it.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use serde_json::{json, Value};

use crate::tool::{Context, Tool};
use crate::workspace::Workspace;

/// A project, and somewhere outside it with something worth stealing in it.
///
/// One argument rather than two, because a caller holding a workspace without
/// an outside would be a caller who can run the first half of this suite and
/// silently skip the half that matters.
#[derive(Debug, Clone, Copy)]
pub struct Rig<'a> {
    /// Where the tool may act.
    pub workspace: &'a Workspace,
    /// A directory that is not the workspace, holding `secret.txt`.
    pub outside: &'a std::path::Path,
}

impl Rig<'_> {
    /// Build one: a project with `src/main.rs` and `README.md` in it, and a
    /// secret outside it. Both directories are the caller's to keep alive.
    pub fn plant(project: &std::path::Path, outside: &std::path::Path) {
        std::fs::create_dir_all(project.join("src")).expect("a project");
        std::fs::write(project.join("src").join("main.rs"), "fn main() {}").expect("a file");
        std::fs::write(project.join("README.md"), "# Hello\n").expect("a file");
        std::fs::write(outside.join("secret.txt"), SECRET).expect("a secret");
    }
}

/// What is outside, and what has to still be there afterwards.
const SECRET: &str = "the passphrase\n";

/// Hold `tool` to everything the trait promises. Panics with what failed.
///
/// Call it from the implementation's own test file. An implementation that does
/// not call it is not an implementation.
pub async fn holds_for(tool: &dyn Tool, rig: Rig<'_>) {
    let named = tool.name().to_owned();

    a_name_is_stable(tool, &named);
    a_schema_is_a_shape_and_carries_no_prose(tool, &named);
    no_path_leaves_the_workspace(tool, rig, &named).await;
}

/// The name is what the log records, so it may not depend on when it was asked.
fn a_name_is_stable(tool: &dyn Tool, named: &str) {
    assert!(!named.is_empty(), "a tool with no name");
    assert_eq!(tool.name(), named, "{named} renamed itself between calls");
}

/// Promises 1 and 2.
fn a_schema_is_a_shape_and_carries_no_prose(tool: &dyn Tool, named: &str) {
    let schema = tool.parameters();
    assert_eq!(
        schema.get("type").and_then(Value::as_str),
        Some("object"),
        "{named} takes something that is not an object"
    );
    assert_eq!(
        schema.get("additionalProperties"),
        Some(&Value::Bool(false)),
        "{named} does not close its schema, so an invented property reaches it"
    );

    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{named} declares no properties"));

    for (property, declared) in properties {
        assert!(
            declared.get("type").and_then(Value::as_str).is_some(),
            "{named}.{property} declares no type, so nothing can be refused about it"
        );
        assert!(
            !carries_prose(declared),
            "{named}.{property} carries prose the tool register should own (rule 10, ADR 0008)"
        );
    }

    for required in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let required = required.as_str().unwrap_or_default();
        assert!(
            properties.contains_key(required),
            "{named} requires {required}, which it never declared"
        );
    }
}

/// Whether a schema fragment holds a `description` anywhere in it.
///
/// Recursive because a schema nests: a property that is an array of objects
/// hides its prose two levels down, and the rule is about where model-facing
/// text lives rather than about how deep it was buried.
fn carries_prose(schema: &Value) -> bool {
    match schema {
        Value::Object(fields) => {
            fields.contains_key("description") || fields.values().any(carries_prose)
        }
        Value::Array(items) => items.iter().any(carries_prose),
        _ => false,
    }
}

/// Promise 3, against a real directory and every spelling of somewhere else.
async fn no_path_leaves_the_workspace(tool: &dyn Tool, rig: Rig<'_>, named: &str) {
    let context = Context::over(rig.workspace);
    let schema = tool.parameters();
    let properties = schema["properties"].as_object().expect("declared");

    // A tool that takes no path at all is exercised by the shape half alone,
    // and says so rather than passing in silence: a suite that quietly proves
    // nothing about a tool is worse than one that skips it out loud.
    let takes = ["path", "cwd", "directory"];
    if !takes.iter().any(|it| properties.contains_key(*it)) {
        return;
    }

    for escape in escapes(rig.outside) {
        let arguments = filled(properties, &escape);

        let intent = tool.intent(&arguments, &context);
        assert!(
            intent.touches.is_empty(),
            "{named} said it would touch something for {escape}, which nothing could put back"
        );

        let outcome = tool.run(&arguments, &context).await;
        let failure = match outcome {
            Err(failure) => failure,
            Ok(answer) => panic!("{named} answered {answer:?} for {escape}"),
        };
        assert!(
            failure.message.contains("outside the workspace"),
            "{named} refused {escape} for the wrong reason, so the refusal is not the \
             confinement: {failure}"
        );
    }

    // The messages above are evidence. This is the property: what is outside
    // was not read from, written to, or taken away.
    assert_eq!(
        std::fs::read_to_string(rig.outside.join("secret.txt")).expect("still there"),
        SECRET,
        "{named} changed or removed something outside the workspace"
    );
    let left: Vec<String> = std::fs::read_dir(rig.outside)
        .expect("readable")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        left,
        ["secret.txt"],
        "{named} left something outside the workspace"
    );
}

/// Every escaping spelling, as a model would write it.
///
/// The Windows half is not decoration, and only the first of them looks like an
/// escape: a UNC share is another volume, `\\?\C:\...` is the same drive
/// wearing the extended-length prefix, `C:tmp` means whatever directory this
/// process happens to be in on that drive, and `\Windows` means the root of the
/// current one. A long path is here because 260 characters is where Windows
/// changes its mind about paths, and deep is not the same as outside.
fn escapes(outside: &std::path::Path) -> Vec<String> {
    let mut all = vec![
        "..".to_owned(),
        "../secret.txt".to_owned(),
        "src/../../secret.txt".to_owned(),
        "../nowhere/secret.txt".to_owned(),
        format!("{}secret.txt", "../".repeat(40)),
        outside.join("secret.txt").to_string_lossy().into_owned(),
    ];
    if cfg!(windows) {
        all.extend([
            r"\\server\share\secret.txt".to_owned(),
            r"\\?\C:\Windows\win.ini".to_owned(),
            r"C:tmp\secret.txt".to_owned(),
            r"\Windows\win.ini".to_owned(),
        ]);
    }
    all
}

/// A call a tool would accept, with every path-shaped argument set to `escape`.
///
/// Built from the schema rather than written out, so a tool this file has never
/// heard of is still called with arguments it will get as far as acting on.
/// Anything that gets refused by `arguments::faults` instead would prove
/// nothing about confinement.
fn filled(properties: &serde_json::Map<String, Value>, escape: &str) -> Value {
    let mut arguments = serde_json::Map::new();
    for (property, declared) in properties {
        let value = match property.as_str() {
            "path" | "cwd" | "directory" => json!(escape),
            _ => match declared.get("type").and_then(Value::as_str) {
                Some("string") => json!("planted by a tool"),
                Some("integer") | Some("number") => json!(1),
                Some("boolean") => json!(false),
                Some("array") => json!([]),
                _ => continue,
            },
        };
        arguments.insert(property.clone(), value);
    }
    Value::Object(arguments)
}
