//! Recovery of output a model stranded in the reasoning channel.
//!
//! The Qwen chat template opens `<think>` in the prompt itself, so generation begins
//! *inside* the reasoning block: `</think>` is not something the model adds, it is
//! something it must not forget. A system prompt that pressures the model to compress
//! makes forgetting common — it writes its output and hits EOS still inside the block.
//! llama.cpp then reports the whole turn as `reasoning_content` with empty content.
//!
//! Two things get stranded that way, and both arrive as a blank message:
//!
//! 1. **Tool calls** — see [`recover`]. Qwen templates mandate this XML form in the
//!    *content* channel, after `</think>`:
//!
//! ```text
//! <tool_call>
//! <function=wfm_search_items>
//! <parameter=query>
//! Serration
//! </parameter>
//! </function>
//! </tool_call>
//! ```
//!
//!    llama.cpp only runs its tool parser over content, so a call left in reasoning
//!    is never parsed. The text is a well-formed call in the model's own format, one
//!    channel over, so we parse it ourselves rather than lose the turn.
//!
//! 2. **The final prose answer** — see [`promote_stranded_answer`]. Nothing to parse;
//!    the answer is simply in the wrong field.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

use super::{ToolCall, ToolDef};

static TOOL_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<tool_call>\s*(.*?)\s*</tool_call>").unwrap());
static FUNCTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<function=([^>\s]+)>\s*(.*?)\s*</function>").unwrap());
static PARAM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<parameter=([^>\s]+)>\n?(.*?)\n?</parameter>").unwrap());

pub struct Recovered {
    pub calls: Vec<ToolCall>,
    /// Source text with the recovered blocks removed.
    pub cleaned: String,
}

/// Coerce a raw XML parameter value using the tool's declared JSON Schema type.
/// The wire format carries no types — every value arrives as text — so a schema
/// saying `integer` is the only thing that distinguishes 5 from "5".
fn coerce(raw: &str, schema: Option<&Value>) -> Value {
    let ty = schema
        .and_then(|s| s.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("string");
    match ty {
        "string" => Value::String(raw.to_string()),
        "integer" => raw
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        "number" => raw
            .trim()
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        "boolean" => match raw.trim() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(raw.to_string()),
        },
        "array" | "object" => {
            serde_json::from_str(raw.trim()).unwrap_or_else(|_| Value::String(raw.to_string()))
        }
        _ => Value::String(raw.to_string()),
    }
}

/// Scan `text` for tool-call blocks the provider failed to parse.
///
/// Only calls naming a tool in `tools` are recovered: the same XML can legitimately
/// appear in prose (a user pasting a transcript, a model explaining the format), and
/// executing that would be acting on text that was never a call.
pub fn recover(text: &str, tools: &[ToolDef]) -> Option<Recovered> {
    if !text.contains("<tool_call>") {
        return None;
    }

    let mut calls = Vec::new();
    let mut cleaned = text.to_string();

    for tc in TOOL_CALL_RE.captures_iter(text) {
        let whole = tc.get(0).unwrap().as_str();
        let inner = &tc[1];

        let Some(func) = FUNCTION_RE.captures(inner) else {
            continue;
        };
        let name = func[1].trim().to_string();

        let Some(def) = tools.iter().find(|t| t.name == name) else {
            continue; // unknown tool — not ours to execute
        };

        let props = def.input_schema.get("properties");
        let mut args = Map::new();
        for p in PARAM_RE.captures_iter(&func[2]) {
            let key = p[1].trim().to_string();
            let schema = props.and_then(|pr| pr.get(&key));
            args.insert(key, coerce(&p[2], schema));
        }

        calls.push(ToolCall {
            id: format!("recovered_{}", uuid::Uuid::new_v4().simple()),
            name,
            arguments: Value::Object(args),
            thought_signature: None,
        });
        cleaned = cleaned.replace(whole, "");
    }

    if calls.is_empty() {
        return None;
    }
    Some(Recovered {
        calls,
        cleaned: cleaned.trim().to_string(),
    })
}

/// Decide whether a turn's reasoning should be shown as its answer.
///
/// Fires only for a turn that would otherwise render as nothing at all: no content,
/// no tool call to run, but reasoning present. That combination means the model never
/// closed `</think>`, so what it wrote for the user is sitting in the reasoning field.
///
/// Returns the text to promote, or `None` to leave the turn alone.
///
/// This deliberately promotes the *whole* reasoning rather than guessing which
/// trailing part is "the answer": a wrong guess silently truncates the reply, while
/// promoting everything is at worst more verbose than intended. The caller should
/// clear the thinking field so the text is not shown twice.
pub fn promote_stranded_answer(content: &str, thinking: &str) -> Option<String> {
    if !content.trim().is_empty() || thinking.trim().is_empty() {
        return None;
    }
    Some(thinking.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tools() -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "wfm_search_items".into(),
                description: "search".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "language": {"type": "string"},
                        "limit": {"type": "integer"},
                    }
                }),
            },
            ToolDef {
                name: "wfm_get_top_orders".into(),
                description: "orders".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "slug": {"type": "string"} }
                }),
            },
        ]
    }

    /// Verbatim reasoning_content from the failing turn.
    const LEAKED: &str = "Overguard isn't tradable. Let me search for a different maxed mod.\n\n<tool_call>\n<function=wfm_search_items>\n<parameter=language>\nen\n</parameter>\n<parameter=query>\nGreed\n</parameter>\n</function>\n</tool_call>";

    /// Further reasoning_content captured verbatim from Qwen3.5-9B-Q4_K_M leaking
    /// under the caveman system prompt. Kept real rather than synthesised: the
    /// wording, parameter order and value shape all vary run to run.
    const LEAKED_REAL: &[(&str, &str)] = &[
        (
            "Overguard has 0 matches. Trying a different maxed mod.\n\n<tool_call>\n<function=wfm_search_items>\n<parameter=language>\nen\n</parameter>\n<parameter=query>\nOverload\n</parameter>\n</function>\n</tool_call>",
            "Overload",
        ),
        (
            "Overguard not tradable. Pick tradable maxed mod. Search for Amplify Damage.\n\n<tool_call>\n<function=wfm_search_items>\n<parameter=language>\nen\n</parameter>\n<parameter=query>\nAmplify Damage\n</parameter>\n</function>\n</tool_call>",
            "Amplify Damage",
        ),
    ];

    #[test]
    fn recovers_every_real_capture() {
        for (text, expected_query) in LEAKED_REAL {
            let r = recover(text, &tools()).unwrap_or_else(|| panic!("failed to recover: {text}"));
            assert_eq!(r.calls.len(), 1);
            assert_eq!(r.calls[0].name, "wfm_search_items");
            assert_eq!(r.calls[0].arguments["query"], json!(expected_query));
            assert_eq!(r.calls[0].arguments["language"], json!("en"));
            assert!(!r.cleaned.contains("<tool_call>"));
            assert!(!r.cleaned.is_empty(), "reasoning prose should survive");
        }
    }

    #[test]
    fn recovers_real_leaked_call() {
        let r = recover(LEAKED, &tools()).expect("should recover");
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "wfm_search_items");
        assert_eq!(r.calls[0].arguments["query"], json!("Greed"));
        assert_eq!(r.calls[0].arguments["language"], json!("en"));
        assert_eq!(
            r.cleaned,
            "Overguard isn't tradable. Let me search for a different maxed mod."
        );
    }

    #[test]
    fn coerces_by_schema_type() {
        let src = "<tool_call>\n<function=wfm_search_items>\n<parameter=query>\n5\n</parameter>\n<parameter=limit>\n5\n</parameter>\n</function>\n</tool_call>";
        let r = recover(src, &tools()).unwrap();
        // Same text, different declared types.
        assert_eq!(r.calls[0].arguments["query"], json!("5"));
        assert_eq!(r.calls[0].arguments["limit"], json!(5));
    }

    #[test]
    fn multi_word_and_underscore_values_survive() {
        let src = "<tool_call>\n<function=wfm_get_top_orders>\n<parameter=slug>\ndetect_vulnerability\n</parameter>\n</function>\n</tool_call>";
        let r = recover(src, &tools()).unwrap();
        assert_eq!(r.calls[0].arguments["slug"], json!("detect_vulnerability"));
    }

    #[test]
    fn recovers_multiple_blocks() {
        let src = format!("{}\n{}", LEAKED, "<tool_call>\n<function=wfm_get_top_orders>\n<parameter=slug>\nserration\n</parameter>\n</function>\n</tool_call>");
        let r = recover(&src, &tools()).unwrap();
        assert_eq!(r.calls.len(), 2);
        assert_eq!(r.calls[1].name, "wfm_get_top_orders");
    }

    #[test]
    fn ignores_unknown_tool() {
        let src = "<tool_call>\n<function=rm_rf_everything>\n<parameter=path>\n/\n</parameter>\n</function>\n</tool_call>";
        assert!(recover(src, &tools()).is_none());
    }

    #[test]
    fn ignores_plain_text() {
        assert!(recover(
            "No tool call here. Just prose about <function=x>.",
            &tools()
        )
        .is_none());
    }

    /// Verbatim thinking from the turn that rendered blank: reasoning and the finished
    /// answer, both stranded because `</think>` never arrived.
    const STRANDED_ANSWER: &str = "Got top sell orders for Assault Mode mod. Prices: 1, 1, 2, 2, 2 platinum. Average = 7/5 = 1.4 platinum. No buy orders available.\n\nAverage sell price = **1.4 platinum** (from 5 in-game sellers).";

    #[test]
    fn promotes_real_stranded_answer() {
        let got = promote_stranded_answer("", STRANDED_ANSWER).expect("should promote");
        assert!(got.contains("1.4 platinum"));
        assert!(got.contains("5 in-game sellers"));
    }

    #[test]
    fn leaves_healthy_turn_alone() {
        // Content present — reasoning is genuinely just reasoning.
        assert!(promote_stranded_answer("Average is 1.4 platinum.", STRANDED_ANSWER).is_none());
    }

    #[test]
    fn ignores_whitespace_only_fields() {
        assert!(promote_stranded_answer("   \n", "  ").is_none());
        assert!(promote_stranded_answer("", "").is_none());
    }

    #[test]
    fn ids_are_unique() {
        let a = recover(LEAKED, &tools()).unwrap();
        let b = recover(LEAKED, &tools()).unwrap();
        assert_ne!(a.calls[0].id, b.calls[0].id);
    }
}
