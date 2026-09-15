//! Every host tool against its document in the tool register.
//!
//! `docs/rules/prompts.md`: a tool is one document, its description and its
//! parameter prose, and **the schema's shape stays a contract with the parser
//! and is not editable**. The shape is what `Tool::parameters` returns and what
//! `arguments::faults` refuses a call against; the prose is what
//! `demido-prompts` holds. This file is where the two are bound, in both
//! directions, so neither can move without the other noticing:
//!
//! - every host tool has a document, and every document names a host tool;
//! - a document declares exactly the properties the schema has;
//! - what the registry offers is that shape with prose added and nothing else,
//!   whatever an edit, or a file written by hand, says.
//!
//! It is not in `contract::holds_for`, because that suite is also what a tool
//! Demido did not write is held to, and such a tool has no host document.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;

use demido_prompts::{Tools, TOOLS};
use demido_tools::{delegating, delegation, files, shell, Call, Registry, Tool, Workspace};
use serde_json::{json, Value};

fn host() -> Vec<Box<dyn Tool>> {
    files()
        .into_iter()
        .chain(shell())
        .chain(delegation(answering()))
        .collect()
}

/// A delegation that answers without opening anything. What a sub-agent really
/// is belongs to the turn loop
/// ([#63](https://github.com/elpideus/demido-studio/issues/63)); what this file
/// is about is the document the tool is offered in.
fn answering() -> demido_tools::Delegating {
    delegating(|_| async { Ok("the sub-agent answered".to_owned()) })
}

/// A workspace, a prompts directory, and a registry of every host tool.
fn rig() -> (tempfile::TempDir, Tools, Registry) {
    let dir = tempfile::tempdir().expect("a directory");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("README.md"), "# Hello\n").unwrap();

    let tools = Tools::open(dir.path().join("prompts"));
    let registry = Registry::of_files(Some(Workspace::open(&project).expect("a workspace")))
        .with_group(shell())
        .with_group(delegation(answering()));
    (dir, tools, registry)
}

/// A schema with every `description` taken out, at any depth: the shape.
fn shape(schema: &Value) -> Value {
    match schema {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| key.as_str() != "description")
                .map(|(key, value)| (key.clone(), shape(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(shape).collect()),
        other => other.clone(),
    }
}

fn properties(schema: &Value) -> BTreeSet<String> {
    schema["properties"]
        .as_object()
        .expect("properties")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn every_host_tool_has_a_document_and_every_document_names_a_host_tool() {
    let tools: BTreeSet<String> = host().iter().map(|tool| tool.name().to_owned()).collect();
    let documented: BTreeSet<String> = TOOLS.iter().map(|entry| entry.name.to_owned()).collect();

    assert_eq!(
        tools, documented,
        "a host tool with no document ships a model no words; a document with no tool is an entry nothing sends"
    );
}

#[test]
fn a_document_declares_exactly_the_properties_its_schema_has() {
    for tool in host() {
        let entry = demido_prompts::tool(tool.name()).expect("documented");
        let declared: BTreeSet<String> =
            entry.parameters.iter().map(|it| (*it).to_owned()).collect();
        assert_eq!(
            declared,
            properties(&tool.parameters()),
            "{}'s document and its schema disagree about what it takes",
            tool.name()
        );
    }
}

#[test]
fn an_offered_tool_is_its_shape_with_its_documents_prose_on_it() {
    let (_dir, tools, registry) = rig();
    let offered = registry.offered(&tools);
    assert_eq!(offered.len(), host().len(), "every host tool is offered");

    for (spec, tool) in offered.iter().zip(host()) {
        let document = tools.get(tool.name()).expect("documented");

        assert_eq!(spec.name, tool.name());
        assert_eq!(spec.description, document.description());
        assert_eq!(spec.document.hash, document.hash);
        assert_eq!(
            shape(&spec.parameters),
            tool.parameters(),
            "{} was offered a different shape from the one its calls are parsed against",
            spec.name
        );
        for property in properties(&tool.parameters()) {
            assert_eq!(
                spec.parameters["properties"][&property]["description"].as_str(),
                document.parameter(&property),
                "{}.{property} was offered prose its document does not hold",
                spec.name
            );
        }
    }
}

#[test]
fn an_edit_changes_what_is_said_about_a_tool_and_never_its_shape() {
    let (dir, tools, registry) = rig();
    tools
        .set("read_file", "Open a file.\n\n## path\n\nWhich file.")
        .expect("an edit");

    // Stand in for a hand-edited file that gives prose to a property the tool
    // does not have. `set` refuses this; a file on disk cannot be refused.
    let hand = dir.path().join("prompts").join("tools");
    std::fs::write(hand.join("write_file.md"), "Write.\n\n## colour\n\nRed.").unwrap();

    let offered = registry.offered(&tools);
    let read = offered
        .iter()
        .find(|spec| spec.name == "read_file")
        .unwrap();
    assert_eq!(read.description, "Open a file.");
    assert_eq!(
        read.parameters["properties"]["path"]["description"],
        json!("Which file.")
    );
    assert!(
        read.parameters["properties"]["lines"]
            .get("description")
            .is_none(),
        "a dropped section leaves the property without prose, and still a property"
    );

    let write = offered
        .iter()
        .find(|spec| spec.name == "write_file")
        .unwrap();
    assert!(
        write.parameters["properties"].get("colour").is_none(),
        "prose cannot add a property"
    );
    assert_eq!(
        shape(&write.parameters),
        demido_tools::WriteFile.parameters()
    );

    // And the parser is untouched: a call inventing the property is still
    // refused for it.
    let refused = registry
        .plan(&Call {
            id: "call-1".into(),
            name: "write_file".into(),
            arguments: json!({ "path": "a.txt", "content": "", "colour": "red" }).to_string(),
        })
        .expect_err("colour is not an argument");
    assert!(
        refused.message.contains("colour is not one of"),
        "{refused}"
    );
}
