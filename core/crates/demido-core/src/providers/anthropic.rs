//! Anthropic's Messages API, read from Anthropic's own documentation on 2026-11-21 (research note 18).
//!
//! The chat's request is turned into Anthropic's: what the model is told first goes into `system`; messages
//! of the same side in a row become one (Anthropic wants them to take turns); a sampling setting is left out
//! (the API version used here refuses it); the shape a reply is held to goes into `output_config.format`
//! when Anthropic can hold a reply to it (every object closed, nothing it does not check), and is left to the
//! instructions when it cannot. The conversation is marked for Anthropic's cache: a turn then reads the
//! unchanged beginning at a twentieth to a tenth of the price.

use serde_json::{Map, Value, json};

use super::{Ask, Failed, Heard, RemoteModel, Think, prices, read_events, send};
use crate::engine::Piece;
use crate::{Error, Result};

/// The version of the API this was written against.
const VERSION: &str = "2026-10-21";

/// Models that think however they are asked: `off` asks for the least (effort `low`).
fn always_thinks(name: &str) -> bool {
    ["claude-fable", "claude-mythos", "claude-opus-5-5"].iter().any(|p| name.starts_with(p))
}

/// Levels of effort Anthropic knows, from the least.
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The models Anthropic lets this key use.
pub(super) async fn list(http: &reqwest::Client, base: &str, key: &str) -> Result<Vec<RemoteModel>> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..10 {
        let mut url = format!("{base}/v1/models?limit=1000");
        if let Some(after) = &after {
            url.push_str(&format!("&after_id={after}"));
        }
        let res = send("anthropic", http.get(url).header("x-api-key", key).header("anthropic-version", VERSION)).await.map_err(|f| match f {
            Failed::Other(e) => e,
            Failed::Shape(why) => Error::Other(format!("Anthropic did not give its list of models: {why}")),
        })?;
        let page: Value = res.json().await.map_err(|e| Error::Net(format!("Anthropic's list of models could not be read: {e}")))?;
        out.extend(page["data"].as_array().into_iter().flatten().filter_map(model_from));
        match (page["has_more"] == true, page["last_id"].as_str()) {
            (true, Some(last)) => after = Some(last.to_string()),
            _ => break,
        }
    }
    Ok(out)
}

/// One model as Anthropic lists it.
pub(super) fn model_from(m: &Value) -> Option<RemoteModel> {
    let name = m["id"].as_str()?;
    let caps = &m["capabilities"];
    let supported = |v: &Value| v["supported"] == true;
    let thinking: Vec<String> = if supported(&caps["effort"]) {
        EFFORTS.iter().filter(|level| caps["effort"][**level]["supported"] != false).map(|l| l.to_string()).collect()
    } else if supported(&caps["thinking"]) {
        vec!["on".into()]
    } else {
        Vec::new()
    };
    let (price, in_table) = prices::of("anthropic", name).expect("anthropic has a table");
    Some(RemoteModel {
        id: format!("anthropic/{name}"),
        provider: "anthropic".into(),
        name: m["display_name"].as_str().unwrap_or(name).to_string(),
        context_length: m["max_input_tokens"].as_u64(),
        most_written: m["max_tokens"].as_u64(),
        price_in: Some(price.input),
        price_out: Some(price.output),
        price_from: if in_table { "table" } else { "highest" }.into(),
        free: false,
        always_thinks: always_thinks(name),
        thinking,
        takes: if supported(&caps["image_input"]) { vec!["images".into()] } else { Vec::new() },
        chosen: false,
    })
}

/// A shape Anthropic can hold a reply to, from one the runtime takes: every object closed, and what
/// Anthropic does not check left out. `None` when an object in it is left open on purpose (its fields are
/// up to the model): that one is left to the instructions.
pub(super) fn closed(shape: &Value) -> Option<Value> {
    match shape {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                if matches!(k.as_str(), "minItems" | "maxItems" | "minLength" | "maxLength" | "minimum" | "maximum" | "multipleOf" | "exclusiveMinimum" | "exclusiveMaximum") {
                    continue;
                }
                out.insert(k.clone(), closed(v)?);
            }
            if out.get("type").and_then(Value::as_str) == Some("object") {
                if !out.contains_key("properties") {
                    return None;
                }
                out.insert("additionalProperties".into(), json!(false));
            }
            Some(Value::Object(out))
        }
        Value::Array(items) => items.iter().map(closed).collect::<Option<Vec<_>>>().map(Value::Array),
        other => Some(other.clone()),
    }
}

/// The request as Anthropic takes it.
pub(super) fn request(ask: &Ask) -> Value {
    let (name, body, plain) = (ask.name, ask.body, ask.plain);
    // What the model is told first, then the conversation with each side's messages in a row as one.
    let mut system: Vec<String> = Vec::new();
    let mut turns: Vec<(String, String)> = Vec::new();
    for m in body["messages"].as_array().into_iter().flatten() {
        let (role, text) = (m["role"].as_str().unwrap_or("user"), m["content"].as_str().unwrap_or_default());
        let text = if text.trim().is_empty() { "(nothing)" } else { text };
        match role {
            "system" => system.push(text.to_string()),
            _ => {
                let role = if role == "assistant" { "assistant" } else { "user" };
                match turns.last_mut() {
                    Some((last, said)) if last == role => {
                        said.push_str("\n\n");
                        said.push_str(text);
                    }
                    _ => turns.push((role.to_string(), text.to_string())),
                }
            }
        }
    }
    // Anthropic wants the person first, and the person last (a reply cannot be begun for the model).
    if turns.first().is_none_or(|(role, _)| role != "user") {
        turns.insert(0, ("user".into(), "(The conversation begins.)".into()));
    }
    if turns.last().is_some_and(|(role, _)| role == "assistant") {
        turns.push(("user".into(), "Go on.".into()));
    }
    let quiet = body["max_tokens"].is_u64();
    let budget_thinking = ask.facts.thinking == ["on"];
    let thinks = always_thinks(name) || *ask.think != Think::Off;
    let asked = body["max_tokens"].as_u64();
    let mut most = match asked {
        Some(n) if budget_thinking && thinks => n.max(8192),
        Some(n) if thinks => n.max(2048),
        Some(n) => n,
        None if thinks => 32_000,
        None => 16_000,
    };
    if let Some(cap) = ask.facts.most_written {
        most = most.min(cap);
    }
    let mut req = json!({
        "model": name,
        "max_tokens": most,
        "stream": true,
        "messages": turns.iter().map(|(role, text)| json!({ "role": role, "content": text })).collect::<Vec<_>>(),
        // The beginning of a conversation does not change from turn to turn (DECISIONS L4): read from the cache.
        "cache_control": { "type": "ephemeral" },
    });
    if !system.is_empty() {
        req["system"] = json!(system.join("\n\n"));
    }
    let mut config = Map::new();
    if !plain.plain_thinking {
        match ask.think {
            Think::Off if always_thinks(name) => {
                config.insert("effort".into(), json!("low"));
            }
            Think::Off if name.starts_with("claude-sonnet-5-5") => {
                req["thinking"] = json!({ "type": "between_tools" });
                if quiet {
                    config.insert("effort".into(), json!("low"));
                }
            }
            // The fours do not think unless asked to.
            Think::Off if name.contains("-4-") => {}
            Think::Off => {
                req["thinking"] = json!({ "type": "disabled" });
                if quiet {
                    config.insert("effort".into(), json!("low"));
                }
            }
            Think::On if budget_thinking => req["thinking"] = json!({ "type": "enabled", "budget_tokens": 4096 }),
            Think::On => req["thinking"] = json!({ "type": "adaptive", "display": "summarized" }),
            Think::Level(level) => {
                req["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
                config.insert("effort".into(), json!(level));
            }
        }
    }
    if !plain.no_shape
        && let Some(shape) = body["response_format"]["schema"].as_object().and_then(|s| closed(&Value::Object(s.clone())))
    {
        config.insert("format".into(), json!({ "type": "json_schema", "schema": shape }));
    }
    if !config.is_empty() {
        req["output_config"] = Value::Object(config);
    }
    req
}

/// Asks, and passes the answer on as it is written.
pub(super) async fn ask(ask: &Ask<'_>, stop: &std::sync::atomic::AtomicBool, heard: &mut (dyn FnMut(Piece) + Send)) -> std::result::Result<Heard, Failed> {
    let req = request(ask);
    let res = send("anthropic", ask.http.post(format!("{}/v1/messages", ask.base)).header("x-api-key", ask.key).header("anthropic-version", VERSION).json(&req)).await?;
    let mut out = Heard::default();
    out.stopped = read_events(res, stop, |said| {
        match said["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let u = &said["message"]["usage"];
                out.tokens.input = u["input_tokens"].as_u64().unwrap_or(0);
                out.tokens.cache_written = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                out.tokens.cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                out.tokens.output = u["output_tokens"].as_u64().unwrap_or(0);
            }
            "content_block_delta" => {
                let delta = &said["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => {
                        if let Some(text) = delta["text"].as_str().filter(|t| !t.is_empty()) {
                            out.content.push_str(text);
                            heard(Piece::Text(text));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = delta["thinking"].as_str().filter(|t| !t.is_empty()) {
                            out.thought.push_str(text);
                            heard(Piece::Thought(text));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                let u = &said["usage"];
                // The count of what was written so far, at the end the whole of it.
                if let Some(n) = u["output_tokens"].as_u64() {
                    out.tokens.output = out.tokens.output.max(n);
                }
                if let Some(reason) = said["delta"]["stop_reason"].as_str() {
                    out.ended = Some(reason.to_string());
                }
            }
            "error" => {
                let e = &said["error"];
                let words = e["message"].as_str().unwrap_or("it did not say what");
                return Err(Failed::Other(match e["type"].as_str() {
                    Some("overloaded_error") => Error::Net(format!("Anthropic is overloaded right now, and stopped the answer. Try again in a minute. ({words})")),
                    _ => Error::Net(format!("Anthropic stopped the answer: {words}")),
                }));
            }
            _ => {}
        }
        Ok(())
    })
    .await?;
    if out.ended.as_deref() == Some("max_tokens") && out.content.trim().is_empty() {
        return Err(Failed::Other(Error::Other("The model used all the room it was given on thinking, and wrote no answer. Ask again with thinking lower.".into())));
    }
    Ok(out)
}
