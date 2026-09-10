//! Checking a tool call's arguments before a tool sees them.
//!
//! Two rules, and the second is the important one.
//!
//! **It refuses only what it can prove wrong.** A required property that is
//! absent, a declared type that does not match, a property the schema forbids.
//! Anything it does not understand it says nothing about.
//!
//! **A call it accepts is not thereby proved right.** This is not a JSON Schema
//! validator and must never be mistaken for one: a tool still checks its own
//! arguments. What this buys is that the commonest failures a small model
//! produces (a missing field, a number where a string goes, a property it
//! invented) come back as one sentence the model can act on, instead of as
//! whatever the tool happened to do with nonsense.

use serde_json::Value;

/// Everything wrong with a call's arguments, in the order a person would read
/// them. Empty means nothing could be proved wrong.
///
/// All of them at once, not the first: a model told about one missing field at
/// a time takes one round trip per field, and each round trip is a chance to
/// change its mind about something else.
pub fn faults(arguments: &Value, schema: &Value) -> Vec<String> {
    let mut found = Vec::new();

    let Some(given) = arguments.as_object() else {
        return vec![format!(
            "the arguments have to be a JSON object, and {} was given",
            article(name_of(arguments))
        )];
    };

    let properties = schema.get("properties").and_then(Value::as_object);

    for name in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !given.contains_key(name) {
            found.push(format!("{name} is required and was not given"));
        }
    }

    // Only when the schema closes itself. An open schema means the tool has
    // said it will take more than it listed, and inventing a complaint about
    // that would be this file claiming to know better than the tool.
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        if let Some(properties) = properties {
            for name in given.keys() {
                if !properties.contains_key(name) {
                    found.push(format!("{name} is not one of this tool's arguments"));
                }
            }
        }
    }

    if let Some(properties) = properties {
        for (name, value) in given {
            let Some(declared) = properties.get(name).and_then(kind_of) else {
                continue;
            };
            if !matches(value, declared) {
                found.push(format!(
                    "{name} has to be {}, and {} was given",
                    article(declared),
                    article(name_of(value))
                ));
            }
        }
    }

    found
}

/// The one type a schema declares, or nothing when it declares several or none.
///
/// A `type` given as a list is a union this file does not reason about, so it
/// says nothing rather than guessing which half was meant.
fn kind_of(schema: &Value) -> Option<&str> {
    schema.get("type")?.as_str()
}

fn matches(value: &Value, declared: &str) -> bool {
    match declared {
        "string" => value.is_string(),
        // A whole number written as JSON is an integer whichever way it arrived,
        // and `1.0` is one that a model will produce sooner or later.
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        // A type this file has never heard of is not something it may refuse.
        _ => true,
    }
}

fn name_of(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "null",
    }
}

/// "a string", "an object". Written out because the sentence goes to a model,
/// and a small one reads a malformed sentence as a malformed instruction.
fn article(kind: &str) -> String {
    let article = match kind.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    };
    format!("{article} {kind}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "lines": { "type": "integer" },
                "deep": { "type": "boolean" },
            },
            "required": ["path"],
            "additionalProperties": false,
        })
    }

    #[test]
    fn a_call_that_fits_is_left_alone() {
        assert!(faults(&json!({ "path": "src/main.rs" }), &schema()).is_empty());
        assert!(faults(&json!({ "path": "a", "lines": 20 }), &schema()).is_empty());
    }

    #[test]
    fn every_fault_comes_back_at_once() {
        // One at a time means one round trip per field, and every round trip is
        // a chance for the model to change its mind about something else.
        let faults = faults(&json!({ "lines": "twenty", "colour": "red" }), &schema());

        assert_eq!(faults.len(), 3, "{faults:?}");
        assert!(faults[0].contains("path is required"));
        assert!(faults.iter().any(|f| f.contains("colour is not one of")));
        assert!(faults
            .iter()
            .any(|f| f.contains("lines has to be an integer, and a string was given")));
    }

    #[test]
    fn a_whole_number_is_an_integer_however_it_was_written() {
        assert!(faults(&json!({ "path": "a", "lines": 20.0 }), &schema()).is_empty());
        assert_eq!(
            faults(&json!({ "path": "a", "lines": 20.5 }), &schema()).len(),
            1
        );
    }

    #[test]
    fn an_open_schema_does_not_complain_about_what_it_did_not_list() {
        // The tool said it would take more than it listed. Refusing anyway
        // would be this file claiming to know better than the tool.
        let open = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
        });
        assert!(faults(&json!({ "path": "a", "extra": 1 }), &open).is_empty());
    }

    #[test]
    fn a_construct_this_file_does_not_understand_is_not_refused() {
        // The whole posture: prove wrong or say nothing.
        let exotic = json!({
            "type": "object",
            "properties": { "either": { "type": ["string", "number"] } },
        });
        assert!(faults(&json!({ "either": true }), &exotic).is_empty());
    }

    #[test]
    fn arguments_that_are_not_an_object_are_one_fault_and_not_a_list_of_them() {
        let faults = faults(&json!([1, 2]), &schema());
        assert_eq!(faults.len(), 1);
        assert!(faults[0].contains("an array was given"), "{faults:?}");
    }
}
