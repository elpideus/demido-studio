//! The OpenAI way of asking, which Gemini (Google's compatible API, in beta) and OpenRouter both take. Read
//! from their own documentation on 2026-11-21 (research note 18).
//!
//! The chat's request is nearly theirs already. What changes: the runtime's own switches are left out; the
//! shape a reply is held to is wrapped the way these APIs want it; and thinking is asked for in each one's
//! words. Gemini's newer models cannot be told not to think: `off` asks for the least (`minimal` or `low`).
//! OpenRouter says per model whether it must think, and with which levels.

use serde_json::{Value, json};

use super::{Account, AccountLine, Ask, Failed, Heard, RemoteModel, Think, prices, read_events, send};
use crate::engine::Piece;
use crate::{Error, Result};

/// How OpenRouter knows who is asking: the project's own name and address.
fn with_names(provider: &str, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if provider == "openrouter" { request.header("HTTP-Referer", "https://github.com/elpideus/demido-studio").header("X-OpenRouter-Title", "Demido Studio") } else { request }
}

/// What Gemini's list holds that is not a model to talk to: speech, pictures, live sound, embeddings.
const NOT_FOR_TALKING: [&str; 13] = ["embedding", "tts", "image", "live", "transcribe", "native-audio", "robotics", "omni", "banana", "decision", "aqa", "veo", "imagen"];

/// Gemini models that can be told not to think at all.
fn gemini_may_not_think(name: &str) -> bool {
    name.starts_with("gemini-2.5-flash")
}

/// The models a provider lists.
pub(super) async fn list(provider: &str, http: &reqwest::Client, base: &str, key: &str) -> Result<Vec<RemoteModel>> {
    let mut asking = with_names(provider, http.get(format!("{base}/models")));
    if !key.is_empty() {
        asking = asking.bearer_auth(key);
    }
    let res = send(provider, asking).await.map_err(|f| match f {
        Failed::Other(e) => e,
        Failed::Shape(why) => Error::Other(format!("The list of models could not be had: {why}")),
    })?;
    let said: Value = res.json().await.map_err(|e| Error::Net(format!("The list of models could not be read: {e}")))?;
    let data = said["data"].as_array().cloned().unwrap_or_default();
    Ok(match provider {
        "gemini" => data.iter().filter_map(gemini_model).collect(),
        _ => data.iter().filter_map(openrouter_model).collect(),
    })
}

pub(super) fn gemini_model(m: &Value) -> Option<RemoteModel> {
    let id = m["id"].as_str()?;
    let name = id.strip_prefix("models/").unwrap_or(id);
    if !name.starts_with("gemini-") || NOT_FOR_TALKING.iter().any(|w| name.contains(w)) {
        return None;
    }
    let mut thinking: Vec<String> = ["low", "medium", "high"].iter().map(|l| l.to_string()).collect();
    if name.starts_with("gemini-4-neon") || name.starts_with("gemini-4-hydrogen") {
        thinking.push("xhigh".into());
    }
    let (price, in_table) = prices::of("gemini", name).expect("gemini has a table");
    Some(RemoteModel {
        id: format!("gemini/{name}"),
        provider: "gemini".into(),
        name: m["display_name"].as_str().unwrap_or(name).to_string(),
        context_length: None,
        most_written: None,
        price_in: Some(price.input),
        price_out: Some(price.output),
        price_from: if in_table { "table" } else { "highest" }.into(),
        free: false,
        always_thinks: !gemini_may_not_think(name),
        thinking,
        takes: vec!["images".into()],
        chosen: false,
    })
}

/// A price OpenRouter gives as dollars per token, in a string. Negative: it depends (its own router).
fn per_million(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()).filter(|p| *p >= 0.0).map(|p| p * 1_000_000.0)
}

pub(super) fn openrouter_model(m: &Value) -> Option<RemoteModel> {
    let name = m["id"].as_str()?;
    let arch = &m["architecture"];
    let writes_text = arch["output_modalities"].as_array().is_none_or(|out| out.iter().any(|o| o == "text"));
    if !writes_text {
        return None;
    }
    let (price_in, price_out) = (per_million(&m["pricing"]["prompt"]), per_million(&m["pricing"]["completion"]));
    let free = name.ends_with(":free") || (price_in == Some(0.0) && price_out == Some(0.0));
    let reasons = m["supported_parameters"].as_array().is_some_and(|p| p.iter().any(|x| x == "reasoning")) || m["reasoning"].is_object();
    let thinking: Vec<String> = if !reasons {
        Vec::new()
    } else {
        let listed: Vec<String> = m["reasoning"]["supported_efforts"].as_array().into_iter().flatten().filter_map(|e| e.as_str()).filter(|e| *e != "none").map(str::to_string).collect();
        if listed.is_empty() {
            vec!["low".into(), "medium".into(), "high".into()]
        } else {
            // OpenRouter lists them from the most; Demido lists from the least.
            listed.into_iter().rev().collect()
        }
    };
    Some(RemoteModel {
        id: format!("openrouter/{name}"),
        provider: "openrouter".into(),
        name: m["name"].as_str().unwrap_or(name).to_string(),
        context_length: m["context_length"].as_u64(),
        most_written: m["top_provider"]["max_completion_tokens"].as_u64(),
        price_in,
        price_out,
        price_from: if price_in.is_some() { "provider" } else { "unknown" }.into(),
        free,
        always_thinks: m["reasoning"]["mandatory"] == true,
        thinking,
        takes: if arch["input_modalities"].as_array().is_some_and(|i| i.iter().any(|x| x == "image")) { vec!["images".into()] } else { Vec::new() },
        chosen: false,
    })
}

/// The request as these APIs take it.
pub(super) fn request(provider: &str, ask: &Ask) -> Value {
    let (name, body, plain, facts) = (ask.name, ask.body, ask.plain, ask.facts);
    let mut req = json!({ "model": name, "messages": body["messages"], "stream": true, "stream_options": { "include_usage": true } });
    if let Some(t) = body.get("temperature") {
        req["temperature"] = t.clone();
    }
    let thinks = facts.always_thinks || *ask.think != Think::Off;
    if let Some(asked) = body["max_tokens"].as_u64() {
        // What a model thinks counts against what it may write: room for both.
        req["max_tokens"] = json!(if thinks { asked.max(2048) } else { asked });
    }
    if !plain.no_shape
        && let Some(shape) = body["response_format"].get("schema")
    {
        req["response_format"] = json!({ "type": "json_schema", "json_schema": { "name": "reply", "strict": false, "schema": shape } });
    }
    if !plain.plain_thinking {
        match provider {
            "gemini" => {
                let level = match ask.think {
                    Think::Off if gemini_may_not_think(name) => Some("none".to_string()),
                    // The least it does: Flash models know "minimal", the others start at "low".
                    Think::Off if name.contains("flash") => Some("minimal".to_string()),
                    Think::Off => Some("low".to_string()),
                    Think::On => None,
                    Think::Level(level) => Some(level.clone()),
                };
                if let Some(level) = level {
                    req["reasoning_effort"] = json!(level);
                }
            }
            _ if facts.thinking.is_empty() => {}
            _ => {
                let effort = match ask.think {
                    Think::Off if facts.always_thinks => facts.thinking.first().cloned(),
                    Think::Off => Some("none".to_string()),
                    Think::On => None,
                    Think::Level(level) => Some(level.clone()),
                };
                if let Some(effort) = effort {
                    req["reasoning"] = json!({ "effort": effort });
                }
            }
        }
    }
    req
}

/// Asks, and passes the answer on as it is written.
pub(super) async fn ask(provider: &str, ask: &Ask<'_>, stop: &std::sync::atomic::AtomicBool, heard: &mut (dyn FnMut(Piece) + Send)) -> std::result::Result<Heard, Failed> {
    let req = request(provider, ask);
    let res = send(provider, with_names(provider, ask.http.post(format!("{}/chat/completions", ask.base))).bearer_auth(ask.key).json(&req)).await?;
    let mut out = Heard::default();
    out.stopped = read_events(res, stop, |said| {
        if said["error"].is_object() {
            let words = said["error"]["message"].as_str().unwrap_or("it did not say what");
            return Err(Failed::Other(Error::Net(format!("The provider stopped the answer: {words}"))));
        }
        let choice = &said["choices"][0];
        let delta = &choice["delta"];
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            out.content.push_str(text);
            heard(Piece::Text(text));
        }
        // OpenRouter names a model's thinking `reasoning`; others `reasoning_content`.
        if let Some(text) = delta["reasoning"].as_str().or(delta["reasoning_content"].as_str()).filter(|t| !t.is_empty()) {
            out.thought.push_str(text);
            heard(Piece::Thought(text));
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            out.ended = Some(reason.to_string());
        }
        let u = &said["usage"];
        if u.is_object() {
            let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0).min(prompt);
            let written = u["completion_tokens"].as_u64().unwrap_or(0);
            // Whether thinking is counted in `completion_tokens` differs; the total always holds it.
            let total = u["total_tokens"].as_u64().unwrap_or(0);
            out.tokens = prices::Tokens { input: prompt - cached, cached, cache_written: 0, output: written.max(total.saturating_sub(prompt)) };
            if let Some(cost) = u["cost"].as_f64() {
                out.cost = Some(cost);
            }
        }
        Ok(())
    })
    .await?;
    if out.ended.as_deref() == Some("length") && out.content.trim().is_empty() {
        return Err(Failed::Other(Error::Other("The model used all the room it was given on thinking, and wrote no answer. Ask again with thinking lower.".into())));
    }
    Ok(out)
}

/// What OpenRouter says about the key's account.
pub(super) async fn account(http: &reqwest::Client, base: &str, key: &str) -> Result<Account> {
    let res = send("openrouter", with_names("openrouter", http.get(format!("{base}/key"))).bearer_auth(key)).await.map_err(|f| match f {
        Failed::Other(e) => e,
        Failed::Shape(why) => Error::Other(format!("OpenRouter did not say: {why}")),
    })?;
    let said: Value = res.json().await.map_err(|e| Error::Net(format!("OpenRouter's answer could not be read: {e}")))?;
    let d = &said["data"];
    let mut lines =
        vec![AccountLine { label: "Spent with this key this month".into(), used: d["usage_monthly"].as_f64().unwrap_or(0.0), limit: None, unit: "$".into(), reset: Some("each month (UTC)".into()) }];
    if let (Some(limit), Some(left)) = (d["limit"].as_f64(), d["limit_remaining"].as_f64()) {
        lines.push(AccountLine {
            label: "The key's own limit".into(),
            used: (limit - left).max(0.0),
            limit: Some(limit),
            unit: "$".into(),
            reset: d["limit_reset"].as_str().map(|r| format!("resets {r}")),
        });
    }
    let free = &d["free_model_daily_requests"];
    if let (Some(used), Some(limit)) = (free["used"].as_f64(), free["limit"].as_f64()) {
        lines.push(AccountLine { label: "Requests to free models today".into(), used, limit: Some(limit), unit: "requests".into(), reset: Some("at midnight UTC".into()) });
    }
    let note = (d["is_free_tier"] == true).then(|| "This account has never bought credits, so free models allow fewer requests a day.".to_string());
    Ok(Account { provider: "openrouter".into(), lines, note })
}
