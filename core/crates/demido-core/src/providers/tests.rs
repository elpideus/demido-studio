//! Asking providers, against stand-ins on this computer. Nothing here reaches a provider or spends money.
//! The stand-ins answer in the shapes the providers' own documentation shows (research note 18).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::*;
use crate::db::Db;
use crate::events::EventLog;
use crate::settings::Settings;
use crate::vault::Vault;

const ANTHROPIC_KEY: &str = "sk-ant-test-not-real-0001";
const OPENROUTER_KEY: &str = "sk-or-v1-test-not-real-0002";
const GEMINI_KEY: &str = "AIzaTEST-not-real-0003";

/// What the stand-in saw, and how it is to answer.
#[derive(Default)]
struct Seen {
    /// Each request's path, headers and body, in order.
    asked: Vec<(String, HashMap<String, String>, Value)>,
    /// Anthropic's stand-in refuses a way of thinking, once, as Anthropic does for a model that cannot.
    refuse_thinking_once: bool,
    /// Every stand-in says the key is wrong.
    wrong_key: bool,
}

type Shared = Arc<Mutex<Seen>>;

fn sse(events: &[Value]) -> Response {
    let body: String = events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("message"))).collect();
    ([("content-type", "text/event-stream")], body).into_response()
}

async fn stand_in(State(seen): State<Shared>, headers: HeaderMap, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(request.into_body(), 1 << 20).await.unwrap_or_default()).unwrap_or(Value::Null);
    let headers: HashMap<String, String> = headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect();
    let (refuse, wrong_key) = {
        let mut seen = seen.lock().unwrap();
        seen.asked.push((path.clone(), headers, body.clone()));
        let refuse = seen.refuse_thinking_once && body.get("thinking").is_some();
        if refuse {
            seen.refuse_thinking_once = false;
        }
        (refuse, seen.wrong_key)
    };
    if wrong_key {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } }))).into_response();
    }
    match path.as_str() {
        "/anthropic/v1/models" => axum::Json(json!({ "data": [
            { "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "max_input_tokens": 1_000_000, "max_tokens": 128_000, "capabilities": {
                "image_input": { "supported": true }, "structured_outputs": { "supported": true },
                "thinking": { "supported": true, "types": { "enabled": { "supported": false }, "adaptive": { "supported": true } } },
                "effort": { "supported": true, "low": { "supported": true }, "medium": { "supported": true }, "high": { "supported": true }, "xhigh": { "supported": true }, "max": { "supported": true } } } },
            { "id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "max_input_tokens": 200_000, "max_tokens": 64_000, "capabilities": {
                "thinking": { "supported": true, "types": { "enabled": { "supported": true }, "adaptive": { "supported": false } } }, "effort": { "supported": false } } },
        ], "has_more": false, "last_id": "claude-haiku-4-5-20251001" }))
        .into_response(),
        "/anthropic/v1/messages" if refuse => (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "\"thinking.type.disabled\" is not supported for this model." } })),
        )
            .into_response(),
        "/anthropic/v1/messages" => sse(&[
            json!({ "type": "message_start", "message": { "id": "msg_1", "usage": { "input_tokens": 1000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 3000, "output_tokens": 1 } } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "The person wants a short answer." } }),
            json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "Paris " } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "is the capital." } }),
            json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 500 } }),
            json!({ "type": "message_stop" }),
        ]),
        "/openrouter/models" => axum::Json(json!({ "data": [
            { "id": "meta-llama/llama-3.3-70b-instruct:free", "name": "Llama 3.3 70B (free)", "context_length": 131072, "pricing": { "prompt": "0", "completion": "0" },
              "architecture": { "input_modalities": ["text"], "output_modalities": ["text"] }, "supported_parameters": ["max_tokens", "temperature", "response_format"] },
            { "id": "google/gemini-3.5-flash", "name": "Gemini 3.5 Flash", "context_length": 1048576, "pricing": { "prompt": "0.0000015", "completion": "0.0000075" },
              "architecture": { "input_modalities": ["text", "image"], "output_modalities": ["text"] }, "supported_parameters": ["reasoning", "response_format"],
              "reasoning": { "supported_efforts": ["high", "medium", "low", "minimal"], "default_effort": "medium", "mandatory": true } },
            { "id": "some/image-maker", "name": "Pictures only", "pricing": { "prompt": "0.000001", "completion": "0.000001" }, "architecture": { "output_modalities": ["image"] } },
        ] }))
        .into_response(),
        "/openrouter/key" => axum::Json(json!({ "data": { "label": "sk-or-v1-...002", "limit": 20.0, "limit_remaining": 17.5, "limit_reset": "monthly", "usage_monthly": 2.5, "is_free_tier": false,
            "free_model_daily_requests": { "used": 12, "limit": 1000, "remaining": 988 } } }))
        .into_response(),
        "/gemini/models" => axum::Json(json!({ "object": "list", "data": [
            { "id": "models/gemini-3.5-flash-lite", "object": "model", "owned_by": "google" },
            { "id": "models/gemini-2.5-flash", "object": "model", "owned_by": "google" },
            { "id": "models/gemini-3.8-flash-tts", "object": "model", "owned_by": "google" },
            { "id": "models/gemini-embedding-2", "object": "model", "owned_by": "google" },
        ] }))
        .into_response(),
        "/openrouter/chat/completions" | "/gemini/chat/completions" => {
            let cost = if path.starts_with("/openrouter") && !body["model"].as_str().unwrap_or_default().ends_with(":free") { json!(0.0123) } else { Value::Null };
            sse(&[
                json!({ "id": "gen-1", "choices": [{ "index": 0, "delta": { "role": "assistant", "reasoning": "Thinking briefly." } }] }),
                json!({ "id": "gen-1", "choices": [{ "index": 0, "delta": { "content": "Rome is " } }] }),
                json!({ "id": "gen-1", "choices": [{ "index": 0, "delta": { "content": "in Italy." }, "finish_reason": "stop" }] }),
                json!({ "id": "gen-1", "choices": [], "usage": { "prompt_tokens": 2000, "completion_tokens": 100, "total_tokens": 2150, "cost": cost, "prompt_tokens_details": { "cached_tokens": 500 } } }),
            ])
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A stand-in for every provider, on this computer. Returns its address and what it saw.
async fn stand_ins() -> (String, Shared) {
    let seen: Shared = Arc::default();
    let app = Router::new().fallback(stand_in).with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, seen)
}

/// Providers over a fresh database and a vault in a fresh folder, pointed at the stand-ins.
fn providers(base: &str) -> (Arc<Providers>, Arc<Settings>) {
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
    let home = std::env::temp_dir().join(format!("demido-providers-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let vault = Arc::new(Vault::in_file(&home));
    (Arc::new(Providers::new(db, vault, settings.clone(), events).at(base)), settings)
}

fn chat_body(messages: Value) -> Value {
    json!({ "model": "x", "messages": messages, "stream": true, "chat_template_kwargs": { "enable_thinking": false }, "reasoning_effort": "none" })
}

fn facts(id: &str, thinking: &[&str], always: bool) -> RemoteModel {
    let (provider, _) = id.split_once('/').unwrap();
    RemoteModel {
        id: id.into(),
        provider: provider.into(),
        name: id.into(),
        context_length: None,
        most_written: None,
        price_in: Some(1.0),
        price_out: Some(2.0),
        price_from: "table".into(),
        free: false,
        thinking: thinking.iter().map(|t| t.to_string()).collect(),
        always_thinks: always,
        takes: vec![],
        chosen: true,
    }
}

fn anthropic_request(name: &str, body: &Value, think: Think, thinking: &[&str]) -> Value {
    let facts = facts(&format!("anthropic/{name}"), thinking, anthropic::model_from(&json!({ "id": name })).unwrap().always_thinks);
    let http = reqwest::Client::new();
    anthropic::request(&Ask { http: &http, base: "", key: "", name, facts: &facts, body, think: &think, plain: Plainer::default() })
}

#[test]
fn a_request_becomes_anthropics_own() {
    let body = json!({
        "model": "x", "stream": true, "temperature": 0, "max_tokens": 200,
        "messages": [
            { "role": "system", "content": "Today's date: Saturday 21 November 2026." },
            { "role": "user", "content": "What is the capital of France?" },
            { "role": "assistant", "content": "{\"tool\": \"none\"}" },
            { "role": "assistant", "content": "" },
            { "role": "user", "content": "Answer now." },
        ],
        "response_format": { "type": "json_schema", "schema": { "type": "object", "properties": { "steps": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 9 } }, "required": ["steps"] } },
    });
    let req = anthropic_request("claude-opus-5-5", &body, Think::Off, &["low", "medium", "high"]);
    assert_eq!(req["system"], "Today's date: Saturday 21 November 2026.");
    // The side that spoke twice in a row speaks once; nothing is empty; no sampling setting goes.
    assert_eq!(req["messages"].as_array().unwrap().len(), 3);
    assert_eq!(req["messages"][1]["content"], "{\"tool\": \"none\"}\n\n(nothing)");
    assert!(req.get("temperature").is_none());
    assert_eq!(req["cache_control"], json!({ "type": "ephemeral" }));
    // Opus 5.5 always thinks: off asks for the least, and leaves room for it.
    assert!(req.get("thinking").is_none());
    assert_eq!(req["output_config"]["effort"], "low");
    assert_eq!(req["max_tokens"], 2048);
    // The shape is held, closed, without what Anthropic does not check.
    let shape = &req["output_config"]["format"]["schema"];
    assert_eq!(shape["additionalProperties"], false);
    assert!(shape["properties"]["steps"].get("minItems").is_none() && shape["properties"]["steps"].get("maxItems").is_none(), "{shape}");

    // Each model is told "do not think" in the words it takes.
    assert_eq!(anthropic_request("claude-sonnet-5-5", &body, Think::Off, &["low"])["thinking"], json!({ "type": "between_tools" }));
    assert_eq!(anthropic_request("claude-opus-5", &body, Think::Off, &["low"])["thinking"], json!({ "type": "disabled" }));
    assert!(anthropic_request("claude-opus-4-8", &body, Think::Off, &["low"]).get("thinking").is_none());
    let deep = anthropic_request("claude-sonnet-5-5", &body, Think::Level("high".into()), &["low", "high"]);
    assert_eq!((deep["thinking"]["type"].as_str(), deep["output_config"]["effort"].as_str()), (Some("adaptive"), Some("high")));
    // A model that thinks with a budget of tokens gets one, and room past it.
    let haiku = anthropic_request("claude-haiku-4-5-20251001", &body, Think::On, &["on"]);
    assert_eq!((haiku["thinking"]["type"].as_str(), haiku["max_tokens"].as_u64()), (Some("enabled"), Some(8192)));
}

#[test]
fn a_shape_left_open_on_purpose_is_left_to_the_instructions() {
    // Which tool, with arguments left to the model: Anthropic would hold them empty, so it is not held.
    let open = crate::tools::shape(&crate::tools::offered(false, true, false));
    assert_eq!(anthropic::closed(&open), None);
    // One tool with its arguments by name can be held, every object closed.
    let one = crate::tools::shape_of(crate::tools::named("calculate").unwrap());
    let closed = anthropic::closed(&one).unwrap();
    assert_eq!((closed["additionalProperties"].as_bool(), closed["properties"]["args"]["additionalProperties"].as_bool()), (Some(false), Some(false)));
}

#[test]
fn gemini_and_openrouter_are_asked_to_think_in_their_own_words() {
    let http = reqwest::Client::new();
    let body = json!({ "model": "x", "messages": [{ "role": "user", "content": "Hi" }], "stream": true, "max_tokens": 200, "temperature": 0,
        "chat_template_kwargs": { "enable_thinking": false }, "reasoning_effort": "none", "reasoning_control": true,
        "response_format": { "type": "json_schema", "schema": { "type": "object" } } });
    let ask = |provider: &str, name: &str, facts: &RemoteModel, think: Think| {
        openai::request(provider, &Ask { http: &http, base: "", key: "", name, facts, body: &body, think: &think, plain: Plainer::default() })
    };
    let flash = openai::gemini_model(&json!({ "id": "models/gemini-3.5-flash-lite" })).unwrap();
    let req = ask("gemini", "gemini-3.5-flash-lite", &flash, Think::Off);
    assert_eq!(req["reasoning_effort"], "minimal");
    assert!(req.get("chat_template_kwargs").is_none() && req.get("reasoning_control").is_none(), "the runtime's own switches stay here: {req}");
    assert_eq!(req["response_format"]["json_schema"]["name"], "reply");
    assert_eq!(req["max_tokens"], 2048, "room to think as well as answer");
    let old = openai::gemini_model(&json!({ "id": "models/gemini-2.5-flash" })).unwrap();
    assert_eq!((ask("gemini", "gemini-2.5-flash", &old, Think::Off)["reasoning_effort"].as_str(), old.always_thinks), (Some("none"), false));
    let pro = openai::gemini_model(&json!({ "id": "models/gemini-3.1-pro-preview" })).unwrap();
    assert_eq!(ask("gemini", "gemini-3.1-pro-preview", &pro, Think::Off)["reasoning_effort"], "low");
    // OpenRouter: a model that must think is asked for its least; one that may not, for none; one that cannot, nothing.
    let must = facts("openrouter/google/gemini-3.5-flash", &["minimal", "low", "medium", "high"], true);
    assert_eq!(ask("openrouter", "google/gemini-3.5-flash", &must, Think::Off)["reasoning"], json!({ "effort": "minimal" }));
    let may = facts("openrouter/some/thinker", &["low", "high"], false);
    assert_eq!(ask("openrouter", "some/thinker", &may, Think::Off)["reasoning"], json!({ "effort": "none" }));
    assert_eq!(ask("openrouter", "some/thinker", &may, Think::Level("high".into()))["reasoning"], json!({ "effort": "high" }));
    let plain = facts("openrouter/plain/model", &[], false);
    assert!(ask("openrouter", "plain/model", &plain, Think::Off).get("reasoning").is_none());
}

#[test]
fn lists_keep_what_can_be_talked_to_with_its_price() {
    let free = openai::openrouter_model(&json!({ "id": "a/b:free", "pricing": { "prompt": "0", "completion": "0" } })).unwrap();
    assert!(free.free && free.price_in == Some(0.0));
    let paid = openai::openrouter_model(&json!({ "id": "a/c", "pricing": { "prompt": "0.000003", "completion": "0.000015" }, "supported_parameters": ["reasoning"] })).unwrap();
    assert_eq!((paid.price_in.map(|p| (p * 1e6).round() / 1e6), paid.price_out.map(|p| (p * 1e6).round() / 1e6), paid.thinking.len()), (Some(3.0), Some(15.0), 3));
    // Its own router's price depends on what it picks: not known.
    let auto = openai::openrouter_model(&json!({ "id": "openrouter/auto", "pricing": { "prompt": "-1", "completion": "-1" } })).unwrap();
    assert_eq!((auto.price_in, auto.price_from.as_str()), (None, "unknown"));
    assert!(openai::gemini_model(&json!({ "id": "models/gemini-3.8-flash-tts" })).is_none());
    assert!(openai::gemini_model(&json!({ "id": "models/text-bison" })).is_none());
}

#[tokio::test]
async fn a_chosen_model_answers_and_what_it_cost_is_counted() {
    let (base, seen) = stand_ins().await;
    let (p, _) = providers(&base);
    p.vault().put("anthropic", ANTHROPIC_KEY).unwrap();
    // Not chosen yet: nothing is sent.
    assert!(p.ready("anthropic/claude-opus-5-5").unwrap_err().to_string().contains("not chosen"));
    let list = p.models("anthropic", false).await.unwrap();
    assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["anthropic/claude-opus-5-5", "anthropic/claude-haiku-4-5-20251001"]);
    assert_eq!((list[0].thinking.len(), list[0].always_thinks, list[0].context_length), (5, true, Some(1_000_000)));
    assert_eq!(list[1].thinking, ["on"]);
    let chosen = p.choose("anthropic", "claude-opus-5-5", true).await.unwrap();
    assert!(chosen.chosen);
    assert_eq!(p.models("anthropic", false).await.unwrap().iter().filter(|m| m.chosen).count(), 1);
    assert_eq!(seen.lock().unwrap().asked.len(), 1, "the list was asked once and kept");

    let mut pieces = (String::new(), String::new());
    let body = chat_body(json!([{ "role": "system", "content": "Be short." }, { "role": "user", "content": format!("My key is {ANTHROPIC_KEY}. Capital of France?") }]));
    let answer = p
        .answer(
            "anthropic/claude-opus-5-5",
            body,
            &AtomicBool::new(false),
            &mut |piece| match piece {
                Piece::Text(t) => pieces.0.push_str(t),
                Piece::Thought(t) => pieces.1.push_str(t),
                Piece::Started(_) => {}
            },
            For { session: Some("s1"), message: Some("m1") },
        )
        .await
        .unwrap();
    assert_eq!((answer.content.as_str(), pieces.0.as_str(), pieces.1.as_str()), ("Paris is the capital.", "Paris is the capital.", "The person wants a short answer."));
    assert_eq!((answer.timings.read, answer.timings.reused, answer.timings.written), (1000, 3000, 500));
    // The key went in its header, and nowhere in what was sent.
    let (path, headers, sent) = seen.lock().unwrap().asked.last().cloned().unwrap();
    assert_eq!(
        (path.as_str(), headers.get("x-api-key").map(String::as_str), headers.get("anthropic-version").map(String::as_str)),
        ("/anthropic/v1/messages", Some(ANTHROPIC_KEY), Some("2026-10-21"))
    );
    assert!(!sent.to_string().contains(ANTHROPIC_KEY) && sent.to_string().contains(crate::vault::HIDDEN), "{sent}");
    // $4 a million read, a twentieth of it from the cache, $20 a million written.
    let expected = (1000.0 * 4.0 + 3000.0 * 4.0 * 0.05 + 500.0 * 20.0) / 1e6;
    let (cost, exact) = p.cost_of("m1").unwrap().unwrap();
    assert!((cost - expected).abs() < 1e-12 && !exact, "{cost} {expected}");
    let spending = p.spending().unwrap();
    assert!((spending.spent_usd - expected).abs() < 1e-12 && !spending.exact);
    assert_eq!(spending.by_model[0].model, "claude-opus-5-5");
    assert_eq!(p.view().unwrap().iter().find(|v| v.id == "anthropic").unwrap().chosen.len(), 1);
}

#[tokio::test]
async fn openrouter_says_what_it_cost_and_its_free_models_cost_nothing() {
    let (base, seen) = stand_ins().await;
    let (p, settings) = providers(&base);
    p.vault().put("openrouter", OPENROUTER_KEY).unwrap();
    p.choose("openrouter", "google/gemini-3.5-flash", true).await.unwrap();
    p.choose("openrouter", "meta-llama/llama-3.3-70b-instruct:free", true).await.unwrap();
    let body = chat_body(json!([{ "role": "user", "content": "Where is Rome?" }]));
    let answer = p.answer("openrouter/google/gemini-3.5-flash", body.clone(), &AtomicBool::new(false), &mut |_| {}, For { session: None, message: Some("m2") }).await.unwrap();
    assert_eq!((answer.content.as_str(), answer.thought.as_str()), ("Rome is in Italy.", "Thinking briefly."));
    // Its own figure, and the tokens: 500 of 2,000 from the cache, and 150 written with the thinking.
    assert_eq!(p.cost_of("m2").unwrap(), Some((0.0123, true)));
    assert_eq!((answer.timings.read, answer.timings.reused, answer.timings.written), (1500, 500, 150));
    let (_, headers, sent) = seen.lock().unwrap().asked.last().cloned().unwrap();
    assert_eq!(headers.get("authorization").map(String::as_str), Some(format!("Bearer {OPENROUTER_KEY}").as_str()));
    assert_eq!(headers.get("x-openrouter-title").map(String::as_str), Some("Demido Studio"));
    assert_eq!(sent["reasoning"], json!({ "effort": "minimal" }), "it must think: the least");

    // At the month's limit nothing more is sent to a paid model, and a free one still answers.
    settings.set("providers.monthlyLimit", json!("1")).unwrap();
    p.db.with(|c| c.execute("INSERT INTO usage (at, provider, model, input, cached, cache_written, output, cost_usd, exact) VALUES (?1, 'openrouter', 'x', 0, 0, 0, 0, 1.5, 1)", [now_ms()])).unwrap();
    let stopped = p.answer("openrouter/google/gemini-3.5-flash", body.clone(), &AtomicBool::new(false), &mut |_| {}, For::default()).await.unwrap_err().to_string();
    assert!(stopped.contains("the limit is $1.00") && stopped.contains("Spend at most"), "{stopped}");
    let asked_before = seen.lock().unwrap().asked.len();
    let free = p.answer("openrouter/meta-llama/llama-3.3-70b-instruct:free", body, &AtomicBool::new(false), &mut |_| {}, For { session: None, message: Some("m3") }).await.unwrap();
    assert_eq!(free.content, "Rome is in Italy.");
    assert_eq!(seen.lock().unwrap().asked.len(), asked_before + 1);
    assert_eq!(p.cost_of("m3").unwrap(), Some((0.0, true)));

    // What OpenRouter says about the key.
    let account = p.account("openrouter").await.unwrap();
    assert_eq!(
        account.lines.iter().map(|l| (l.label.as_str(), l.used, l.limit)).collect::<Vec<_>>(),
        [("Spent with this key this month", 2.5, None), ("The key's own limit", 2.5, Some(20.0)), ("Requests to free models today", 12.0, Some(1000.0))]
    );
}

#[tokio::test]
async fn a_way_of_thinking_the_provider_refuses_is_dropped_and_remembered() {
    let (base, seen) = stand_ins().await;
    let (p, _) = providers(&base);
    p.vault().put("anthropic", ANTHROPIC_KEY).unwrap();
    p.choose("anthropic", "claude-haiku-4-5-20251001", true).await.unwrap();
    // Haiku 4.5 is asked with a budget for thinking, and the stand-in says no, as for a model that cannot.
    seen.lock().unwrap().refuse_thinking_once = true;
    let mut body = chat_body(json!([{ "role": "user", "content": "Hi" }]));
    body["reasoning_effort"] = Value::Null;
    body["chat_template_kwargs"]["enable_thinking"] = json!(true);
    let answer = p.answer("anthropic/claude-haiku-4-5-20251001", body.clone(), &AtomicBool::new(false), &mut |_| {}, For::default()).await.unwrap();
    assert_eq!(answer.content, "Paris is the capital.");
    let asked: Vec<Value> = seen.lock().unwrap().asked.iter().filter(|(path, ..)| path.ends_with("/messages")).map(|(_, _, b)| b.clone()).collect();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].get("thinking").is_some() && asked[1].get("thinking").is_none());
    // The next request goes the plain way at once.
    p.answer("anthropic/claude-haiku-4-5-20251001", body, &AtomicBool::new(false), &mut |_| {}, For::default()).await.unwrap();
    let last = seen.lock().unwrap().asked.last().cloned().unwrap().2;
    assert!(last.get("thinking").is_none());
}

#[tokio::test]
async fn a_key_the_provider_does_not_take_is_said_plainly() {
    let (base, seen) = stand_ins().await;
    let (p, _) = providers(&base);
    p.vault().put("gemini", GEMINI_KEY).unwrap();
    let list = p.models("gemini", false).await.unwrap();
    assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gemini/gemini-3.5-flash-lite", "gemini/gemini-2.5-flash"]);
    p.choose("gemini", "gemini-2.5-flash", true).await.unwrap();
    seen.lock().unwrap().wrong_key = true;
    let said = p.answer("gemini/gemini-2.5-flash", chat_body(json!([{ "role": "user", "content": "Hi" }])), &AtomicBool::new(false), &mut |_| {}, For::default()).await.unwrap_err().to_string();
    assert!(said.contains("Gemini (Google AI) did not accept the key"), "{said}");
    // Without a key, the provider is not asked at all.
    p.vault().remove("gemini").unwrap();
    let asked = seen.lock().unwrap().asked.len();
    assert!(
        p.answer("gemini/gemini-2.5-flash", chat_body(json!([])), &AtomicBool::new(false), &mut |_| {}, For::default())
            .await
            .unwrap_err()
            .to_string()
            .contains("There is no Gemini (Google AI) key yet")
    );
    assert_eq!(seen.lock().unwrap().asked.len(), asked);
}

#[test]
fn a_model_is_of_a_provider_only_by_its_name() {
    assert_eq!(split("anthropic/claude-opus-5-5").map(|(d, m)| (d.id, m)), Some(("anthropic", "claude-opus-5-5")));
    assert_eq!(split("openrouter/meta-llama/llama-3.3-70b-instruct:free").map(|(d, m)| (d.id, m)), Some(("openrouter", "meta-llama/llama-3.3-70b-instruct:free")));
    assert_eq!(split("Qwen3-1.7B-Q8_0").map(|(d, _)| d.id), None);
    assert_eq!(split("nobody/model").map(|(d, _)| d.id), None);
    assert_eq!(split("anthropic/").map(|(d, _)| d.id), None);
}

#[tokio::test]
async fn a_session_with_a_model_elsewhere_is_answered_and_says_what_it_cost() {
    use crate::chat::Chat;
    use crate::engine::{Engine, ProcessLauncher};
    use crate::runtime::{Runtimes, Source};
    use crate::sessions::Sessions;

    let (base, seen) = stand_ins().await;
    let home = std::env::temp_dir().join(format!("demido-providers-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
    // One request for the turn: the tools are tried elsewhere.
    settings.set("tools.use", json!(false)).unwrap();
    let p = Arc::new(Providers::new(db.clone(), Arc::new(Vault::in_file(&home)), settings.clone(), events.clone()).at(&base));
    // No runtime and no model on this computer: none is needed.
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), Source::default()).unwrap());
    let engine = Arc::new(Engine::new(&home, runtimes, settings.clone(), events.clone(), Arc::new(ProcessLauncher)));
    let sessions = Arc::new(Sessions::new(db));
    let chat = Arc::new(Chat::new(sessions.clone(), engine, events, settings).with_providers(p.clone()));

    p.vault().put("anthropic", ANTHROPIC_KEY).unwrap();
    assert!(chat.start(None, Some("anthropic/claude-opus-5-5")).await.unwrap_err().to_string().contains("not chosen"));
    p.choose("anthropic", "claude-opus-5-5", true).await.unwrap();
    let session = chat.start(None, Some("anthropic/claude-opus-5-5")).await.unwrap();
    assert_eq!(session.model, "anthropic/claude-opus-5-5");
    // Its levels of thinking are the provider's; one it does not have is refused.
    assert_eq!(chat.change(&session.id, None, None, Some("high")).await.unwrap().thinking, "high");
    assert!(chat.change(&session.id, None, None, Some("on")).await.is_err());
    chat.change(&session.id, None, None, Some("off")).await.unwrap();
    // A conversation with it is given room up to what Demido allows a model elsewhere, not the local context.
    assert_eq!(chat.context_use(&session.id).unwrap().limit, 200_000);

    chat.say(&session.id, "Capital of France?").unwrap();
    let mut answer = None;
    for _ in 0..500 {
        if !chat.answering(&session.id) {
            answer = sessions.messages(&session.id).unwrap().pop();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let answer = answer.expect("the answer ended");
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "Paris is the capital."), "{:?}", answer.problem);
    let timings = answer.timings.unwrap();
    let expected = (1000.0 * 4.0 + 3000.0 * 4.0 * 0.05 + 500.0 * 20.0) / 1e6;
    assert!((timings.cost_usd.unwrap() - expected).abs() < 1e-12 && timings.cost_exact == Some(false), "{timings:?}");
    let sent = seen.lock().unwrap().asked.last().cloned().unwrap().2;
    assert!(sent["system"].as_str().unwrap().starts_with("Today's date: "), "{sent}");
    assert_eq!(sent["messages"], json!([{ "role": "user", "content": "Capital of France?" }]));
    std::fs::remove_dir_all(home).ok();
}
