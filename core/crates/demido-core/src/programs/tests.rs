//! Claude Code and OpenCode, against stand-ins that answer in the shapes their documentation shows (and, for
//! OpenCode, the shapes OpenCode 2.2.7 sent on this computer). No real program runs here.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

use super::*;

/// The program's side of a stand-in: what Demido wrote to it, and where it answers.
struct Side {
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    from_demido: BufReader<DuplexStream>,
    to_demido: DuplexStream,
}

impl Side {
    async fn line(&mut self) -> Value {
        let mut line = String::new();
        self.from_demido.read_line(&mut line).await.unwrap();
        serde_json::from_str(line.trim()).unwrap_or(Value::Null)
    }
    async fn say(&mut self, value: Value) {
        self.to_demido.write_all(format!("{value}\n").as_bytes()).await.unwrap();
    }
}

/// Starts nothing: each spawn hands the program's side to the test.
struct StandSpawner(tokio::sync::mpsc::UnboundedSender<Side>, Arc<Mutex<Vec<String>>>);

impl Spawner for StandSpawner {
    fn spawn(&self, _program: &Path, args: &[String], cwd: &Path, env: &[(&str, String)]) -> Result<Started> {
        let (demido_in, program_in) = tokio::io::duplex(1 << 20);
        let (program_out, demido_out) = tokio::io::duplex(1 << 20);
        self.1.lock().unwrap().extend(env.iter().map(|(name, _)| name.to_string()));
        let env = env.iter().map(|(n, v)| (n.to_string(), v.clone())).collect();
        let _ = self.0.send(Side { args: args.to_vec(), cwd: cwd.to_path_buf(), env, from_demido: BufReader::new(program_in), to_demido: program_out });
        Ok(Started { stdin: Box::new(demido_in), stdout: Box::new(BufReader::new(demido_out)), stderr: Arc::default(), interrupt: Box::new(|| {}), child: None })
    }
}

fn stand() -> (Arc<StandSpawner>, tokio::sync::mpsc::UnboundedReceiver<Side>, Arc<Mutex<Vec<String>>>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let env = Arc::new(Mutex::new(Vec::new()));
    (Arc::new(StandSpawner(tx, env.clone())), rx, env)
}

#[test]
fn claude_code_is_run_as_documented_and_keeps_a_folders_own_settings_out() {
    let options = claude_code::Options {
        model: "opus",
        effort: Some("high"),
        mode: "acceptEdits",
        session: "11111111-2222-3333-4444-555555555555",
        resume: false,
        approve_at: Some("http://127.0.0.1:7341/programs/approve/t0k"),
        append: "Answer first.",
    };
    let a = claude_code::args(&options);
    let has = |pair: [&str; 2]| a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
    assert!(a[0] == "-p" && has(["--input-format", "stream-json"]) && has(["--output-format", "stream-json"]) && a.contains(&"--verbose".into()) && a.contains(&"--include-partial-messages".into()));
    assert!(has(["--session-id", "11111111-2222-3333-4444-555555555555"]) && has(["--model", "opus"]) && has(["--effort", "high"]) && has(["--permission-mode", "acceptEdits"]));
    // A folder's own hooks and servers are not loaded; Demido's approval tool is the one server.
    assert!(has(["--setting-sources", "user"]) && a.contains(&"--strict-mcp-config".into()) && has(["--permission-prompt-tool", "mcp__demido__approve"]));
    let servers: Value = serde_json::from_str(&a[a.iter().position(|x| x == "--mcp-config").unwrap() + 1]).unwrap();
    assert_eq!(servers["mcpServers"]["demido"], json!({ "type": "http", "url": "http://127.0.0.1:7341/programs/approve/t0k" }));
    assert!(!a.iter().any(|x| x == "--bare"), "bare mode would not use the person's own login");
    // Going on, at its own default model and effort, with nobody to ask.
    let later = claude_code::args(&claude_code::Options { model: "default", effort: None, resume: true, approve_at: None, ..options });
    let has = |pair: [&str; 2]| later.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
    assert!(has(["--resume", "11111111-2222-3333-4444-555555555555"]) && has(["--permission-prompts", "none"]));
    assert!(!later.contains(&"--model".into()) && !later.contains(&"--effort".into()) && !later.contains(&"--permission-prompt-tool".into()));
    // A person's yes may take longer than Claude Code waits for a tool by itself.
    assert!(claude_code::env().iter().any(|(name, value)| *name == "MCP_TOOL_TIMEOUT" && value.parse::<u64>().unwrap() >= 600_000));
}

#[test]
fn claude_codes_stream_is_read_as_it_comes() {
    let (mut text, mut done) = (String::new(), None);
    let mut all = Vec::new();
    for line in [
        json!({ "type": "system", "subtype": "init", "model": "claude-opus-5-5", "apiKeySource": "none", "session_id": "s1", "permissionMode": "acceptEdits" }),
        json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "message_start", "message": {} } }),
        json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "Look first." } } }),
        json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "I'll read it." } } }),
        json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "content": [{ "type": "text", "text": "I'll read it." }, { "type": "tool_use", "id": "toolu_1", "name": "Read", "input": { "file_path": "/w/todo.txt" } }] } }),
        json!({ "type": "user", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": [{ "type": "text", "text": "Buy stamps." }] }] } }),
        json!({ "type": "system", "subtype": "permission_denied", "tool_name": "Bash", "tool_use_id": "toolu_2", "message": "The person said no." }),
        json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed_warning", "resetsAt": 1795300000, "utilization": 0.82 } }),
        // A subagent's words are its own: only its tools are shown.
        json!({ "type": "stream_event", "parent_tool_use_id": "toolu_9", "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "(subagent)" } } }),
        json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "message_start", "message": {} } }),
        json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "It says: buy stamps." } } }),
        json!({ "type": "result", "subtype": "success", "is_error": false, "result": "It says: buy stamps.", "total_cost_usd": 0.0312, "session_id": "s1",
                "usage": { "input_tokens": 40, "cache_read_input_tokens": 9000, "cache_creation_input_tokens": 1200, "output_tokens": 150 } }),
    ] {
        all.extend(claude_code::read_line(&line, &mut text, &mut done));
    }
    assert_eq!(text, "I'll read it.\n\nIt says: buy stamps.");
    assert_eq!(all[0], Event::Started { model: Some("claude-opus-5-5".into()), billing: Some("none".into()) });
    assert!(all.contains(&Event::Thought("Look first.".into())));
    assert!(all.contains(&Event::Tool { id: "toolu_1".into(), name: "Read".into(), input: json!({ "file_path": "/w/todo.txt" }) }));
    assert!(all.contains(&Event::ToolDone { id: "toolu_1".into(), result: "Buy stamps.".into(), failed: false }));
    assert!(all.contains(&Event::Denied { id: "toolu_2".into(), why: "The person said no.".into() }));
    assert!(all.contains(&Event::Limit { status: "allowed_warning".into(), resets_at: Some(1795300000), used: Some(0.82) }));
    assert!(!all.contains(&Event::Text("(subagent)".into())));
    let turn = done.unwrap();
    assert_eq!((turn.cost_usd, turn.session.as_deref(), turn.error.as_ref()), (Some(0.0312), Some("s1"), None));
    assert_eq!(turn.tokens, prices::Tokens { input: 40, cached: 9000, cache_written: 1200, output: 150 });
    // A turn that failed says why, in its own words.
    let mut done = None;
    claude_code::read_line(&json!({ "type": "result", "subtype": "success", "is_error": true, "result": "Not logged in · Please run /login" }), &mut String::new(), &mut done);
    assert_eq!(done.unwrap().error.as_deref(), Some("Not logged in · Please run /login"));
}

#[tokio::test]
async fn a_claude_code_turn_gives_the_message_and_ends_at_its_result() {
    let (spawner, mut sides, _) = stand();
    let started = spawner.spawn(Path::new("claude"), &[], Path::new("/"), &[]).unwrap();
    let mut side = sides.recv().await.unwrap();
    let program = tokio::spawn(async move {
        let message = side.line().await;
        side.say(json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "Hello." } } })).await;
        side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "Hello.", "total_cost_usd": 0.01, "session_id": "abc", "usage": {} })).await;
        message
    });
    let mut events = Vec::new();
    let turn = claude_code::run(started, "Say hello", &AtomicBool::new(false), &mut |e| events.push(e)).await.unwrap();
    let message = program.await.unwrap();
    assert_eq!(message, json!({ "type": "user", "message": { "role": "user", "content": "Say hello" }, "parent_tool_use_id": null }));
    assert_eq!((turn.result.as_deref(), turn.session.as_deref()), (Some("Hello."), Some("abc")));
    assert_eq!(events, [Event::Text("Hello.".into())]);
    // Signed out: said plainly, with where to sign in.
    let started = spawner.spawn(Path::new("claude"), &[], Path::new("/"), &[]).unwrap();
    let mut side = sides.recv().await.unwrap();
    tokio::spawn(async move {
        side.line().await;
        side.say(json!({ "type": "result", "subtype": "success", "is_error": true, "result": "Not logged in · Please run /login" })).await;
    });
    let turn = claude_code::run(started, "Hi", &AtomicBool::new(false), &mut |_| {}).await.unwrap();
    assert!(turn.error.unwrap().contains("Sign in in a terminal: run claude, then /login"));
}

/// Plays OpenCode's side of ACP for one turn, as OpenCode 2.2.7 answered on this computer.
async fn opencode_side(mut side: Side, permission: bool) -> Vec<Value> {
    let mut asked = Vec::new();
    loop {
        let said = side.line().await;
        if said.is_null() {
            break;
        }
        asked.push(said.clone());
        let id = said["id"].clone();
        match said["method"].as_str().unwrap_or_default() {
            "initialize" => side.say(json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": 1, "agentCapabilities": { "loadSession": true }, "authMethods": [] } })).await,
            "session/new" | "session/resume" => {
                side.say(json!({ "jsonrpc": "2.0", "id": id, "result": { "sessionId": "ses_1", "configOptions": [
                    { "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "opencode/big-pickle",
                      "options": [{ "value": "opencode/big-pickle", "name": "opencode/Big Pickle" }, { "value": "opencode/deepseek-v4.1-flash-free", "name": "opencode/DeepSeek V4.1 Flash Free" }] },
                    { "id": "mode", "name": "Session Mode", "category": "mode", "type": "select", "currentValue": "build", "options": [] } ] } }))
                    .await
            }
            "session/set_config_option" | "session/delete" => side.say(json!({ "jsonrpc": "2.0", "id": id, "result": {} })).await,
            "session/prompt" => {
                let update = |u: Value| json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "ses_1", "update": u } });
                side.say(update(json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "Plan it." } }))).await;
                side.say(update(json!({ "sessionUpdate": "tool_call", "toolCallId": "call_1", "name": "shell", "title": "shell", "kind": "execute", "status": "pending", "rawInput": {} }))).await;
                side.say(update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call_1", "status": "in_progress", "kind": "execute", "title": "shell", "rawInput": { "command": "ls" } })))
                    .await;
                if permission {
                    side.say(json!({ "jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": { "sessionId": "ses_1",
                        "toolCall": { "toolCallId": "call_1", "name": "shell", "title": "ls", "kind": "execute", "rawInput": { "command": "ls" } },
                        "options": [{ "optionId": "once", "kind": "allow_once", "name": "Allow once" }, { "optionId": "always", "kind": "allow_always", "name": "Always allow" }, { "optionId": "reject", "kind": "reject_once", "name": "Reject" }] } }))
                    .await;
                    asked.push(side.line().await);
                }
                side.say(update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call_1", "status": "completed", "content": [{ "type": "content", "content": { "type": "text", "text": "notes.md" } }] }))).await;
                side.say(update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "There is one file." } }))).await;
                side.say(update(json!({ "sessionUpdate": "usage_update", "used": 1540, "size": 200000, "cost": { "amount": 0.0, "currency": "USD" } }))).await;
                side.say(json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn", "usage": { "inputTokens": 1200, "outputTokens": 340, "totalTokens": 1540 } } })).await;
            }
            _ => {}
        }
    }
    asked
}

#[tokio::test]
async fn an_opencode_turn_goes_through_acp_and_asks_what_opencode_asks() {
    let (spawner, mut sides, _) = stand();
    let started = spawner.spawn(Path::new("opencode"), &["acp".into()], Path::new("/w"), &[]).unwrap();
    let side = sides.recv().await.unwrap();
    let program = tokio::spawn(opencode_side(side, true));
    let asked_for: Arc<Mutex<Vec<Approval>>> = Arc::default();
    let seen = asked_for.clone();
    let approve: &Approver<'_> = &move |a: Approval| {
        seen.lock().unwrap().push(a);
        Box::pin(async { Choice::Once })
    };
    let mut events = Vec::new();
    let turn =
        acp::run(started, Path::new("/w"), "opencode/deepseek-v4.1-flash-free", None, Some("Answer first."), "What is here?", &AtomicBool::new(false), &mut |e| events.push(e), approve).await.unwrap();
    let asked = program.await.unwrap();
    let methods: Vec<&str> = asked.iter().filter_map(|m| m["method"].as_str()).collect();
    assert_eq!(methods, ["initialize", "session/new", "session/set_config_option", "session/prompt"]);
    // Demido says it reads no files and runs no terminal for OpenCode; the folder is the session's.
    assert_eq!(asked[0]["params"]["clientCapabilities"], json!({ "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }));
    assert_eq!(asked[1]["params"]["cwd"], "/w");
    assert_eq!(asked[2]["params"], json!({ "sessionId": "ses_1", "configId": "model", "value": "opencode/deepseek-v4.1-flash-free" }));
    // Demido's way of answering goes to the model only, with the first message.
    assert_eq!(asked[3]["params"]["prompt"][0]["annotations"]["audience"], json!(["assistant"]));
    assert_eq!(asked[3]["params"]["prompt"][1]["text"], "What is here?");
    // What OpenCode asked, Demido answered with the option for once.
    let first = asked_for.lock().unwrap()[0].clone();
    assert_eq!((first.kind.as_str(), first.name.as_str()), ("execute", "shell"));
    assert!(events.contains(&Event::ToolInput { id: "call_1".into(), input: json!({ "command": "ls" }) }));
    assert_eq!(asked[4], json!({ "jsonrpc": "2.0", "id": 900, "result": { "outcome": { "outcome": "selected", "optionId": "once" } } }));
    assert!(events.contains(&Event::Thought("Plan it.".into())) && events.contains(&Event::Text("There is one file.".into())));
    assert!(events.contains(&Event::ToolDone { id: "call_1".into(), result: "notes.md".into(), failed: false }));
    assert_eq!((turn.session.as_deref(), turn.cost_usd, turn.tokens.input, turn.tokens.output, turn.stopped), (Some("ses_1"), Some(0.0), 1200, 340, false));
}

#[tokio::test]
async fn opencodes_models_are_read_from_a_session_made_and_deleted_again() {
    let (spawner, mut sides, _) = stand();
    let started = spawner.spawn(Path::new("opencode"), &["acp".into()], Path::new("/list"), &[]).unwrap();
    let program = tokio::spawn(opencode_side(sides.recv().await.unwrap(), false));
    let models = acp::models(started, Path::new("/list")).await.unwrap();
    assert_eq!(models, [("opencode/big-pickle".to_string(), "opencode/Big Pickle".to_string()), ("opencode/deepseek-v4.1-flash-free".to_string(), "opencode/DeepSeek V4.1 Flash Free".to_string())]);
    let methods: Vec<String> = program.await.unwrap().iter().filter_map(|m| m["method"].as_str().map(str::to_string)).collect();
    assert_eq!(methods, ["initialize", "session/new", "session/delete"]);
}

#[test]
fn opencodes_updates_are_read_as_it_sends_them() {
    let (events, cost) = acp::read_update(
        &json!({ "sessionUpdate": "tool_call", "toolCallId": "ses_child:call_2", "title": "Explore: src", "kind": "read", "status": "completed", "content": [{ "type": "diff", "path": "/w/a.rs" }] }),
    );
    assert_eq!(events[0], Event::Tool { id: "ses_child:call_2".into(), name: "Explore: src".into(), input: json!({}) });
    assert_eq!(events[1], Event::ToolDone { id: "ses_child:call_2".into(), result: "Changed /w/a.rs".into(), failed: false });
    assert_eq!(cost, None);
    let (_, cost) = acp::read_update(&json!({ "sessionUpdate": "usage_update", "used": 10, "size": 100, "cost": { "amount": 0.012, "currency": "USD" } }));
    assert_eq!(cost, Some(0.012));
    let (events, _) = acp::read_update(
        &json!({ "sessionUpdate": "session_info_update", "_meta": { "opencode/retry": { "attempt": 1, "error": { "type": "provider.rate-limit", "message": "Rate limit exceeded" } } } }),
    );
    assert_eq!(events, [Event::Retrying("Rate limit exceeded".into())]);
}

/// A chat with providers whose programs are stand-ins, over a fresh home.
struct World {
    chat: Arc<crate::chat::Chat>,
    sessions: Arc<crate::sessions::Sessions>,
    providers: Arc<crate::providers::Providers>,
    settings: Arc<crate::settings::Settings>,
    home: PathBuf,
}

fn world(spawner: Arc<StandSpawner>) -> World {
    use crate::db::Db;
    use crate::engine::{Engine, ProcessLauncher};
    use crate::events::EventLog;
    use crate::runtime::{Runtimes, Source};
    use crate::settings::Settings;
    let home = std::env::temp_dir().join(format!("demido-programs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
    // A program has to be somewhere: any file stands in for it.
    // Demido runs only its own copies: one of each stands in, as if downloaded (the stand-in spawner never
    // runs them).
    for (provider, file) in [("claude-code", "claude"), ("opencode", "opencode")] {
        let place = home.join("programs").join(provider).join("versions/1.0.0");
        std::fs::create_dir_all(&place).unwrap();
        std::fs::write(place.join(file), b"#!/bin/sh\n").unwrap();
        let kept = copies::Kept { provider: provider.into(), version: "1.0.0".into(), path: place.join(file).display().to_string(), size: 10, checked: "test".into(), from: "test".into(), kept_at: 0 };
        std::fs::write(home.join("programs").join(provider).join("kept.json"), serde_json::to_string(&kept).unwrap()).unwrap();
    }
    let vault = Arc::new(crate::vault::Vault::in_file(&home));
    let providers = Arc::new(crate::providers::Providers::new(db.clone(), vault, settings.clone(), events.clone()).with_programs(&home, spawner));
    // Claude Code runs in an account's profile: one is there, as if signed in.
    providers.add_account("claude-code", Some("Personal"), None).unwrap();
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), Source::default()).unwrap());
    let engine = Arc::new(Engine::new(&home, runtimes, settings.clone(), events.clone(), Arc::new(ProcessLauncher)));
    let sessions = Arc::new(crate::sessions::Sessions::new(db));
    let chat = Arc::new(
        crate::chat::Chat::new(sessions.clone(), engine, events, settings.clone()).with_providers(providers.clone()).keeping_in(home.join("saved-folders")).keeping_programs_in(home.join("programs")),
    );
    chat.set_address("http://127.0.0.1:7341".into());
    World { chat, sessions, providers, settings, home }
}

impl World {
    async fn waiting(&self, session: &str) -> crate::permissions::Request {
        for _ in 0..500 {
            if let Some(r) = self.chat.waiting_for(session) {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let last = self.sessions.messages(session).unwrap().pop().unwrap();
        panic!("nothing asked for the person's yes; the answer is {:?}: {:?}, steps {:?}", last.state, last.problem, last.steps);
    }

    async fn answered(&self, session: &str) -> crate::sessions::Message {
        for _ in 0..500 {
            if !self.chat.answering(session) {
                return self.sessions.messages(session).unwrap().pop().unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the answer never ended");
    }
}

#[tokio::test]
async fn a_session_on_claude_code_asks_the_person_through_demido_and_its_changes_can_be_undone() {
    let (spawner, mut sides, env) = stand();
    let w = world(spawner);
    let folder = w.home.join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("todo.txt"), "Buy stamps.\n").unwrap();
    w.providers.choose("claude-code", "opus", true).await.unwrap();
    let session = w.chat.start(None, Some("claude-code/opus")).await.unwrap();
    w.chat.set_folder(&session.id, Some(folder.to_str().unwrap())).unwrap();
    w.chat.say(&session.id, "Add milk to the list.").unwrap();

    let mut side = sides.recv().await.unwrap();
    assert_eq!(side.cwd, folder.canonicalize().unwrap());
    let url = {
        let a = &side.args;
        let servers: Value = serde_json::from_str(&a[a.iter().position(|x| x == "--mcp-config").unwrap() + 1]).unwrap();
        servers["mcpServers"]["demido"]["url"].as_str().unwrap().to_string()
    };
    assert!(url.starts_with("http://127.0.0.1:7341/programs/approve/"));
    assert!(side.args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"), "Balanced: changes go on, the folder is saved first");
    assert!(env.lock().unwrap().iter().any(|name| name == "MCP_TOOL_TIMEOUT"));
    let token = url.rsplit('/').next().unwrap().to_string();
    let message = side.line().await;
    assert_eq!(message["message"]["content"], "Add milk to the list.");

    // Claude Code wants to run a command: Demido's tool asks the person, and the person says yes.
    side.say(
        json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "echo Milk.>>todo.txt" } }] } }),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let chat = w.chat.clone();
    // The person's own hook has changed the command since the model wrote it: the step shows what is asked.
    let asking = tokio::spawn(async move { chat.program_approval(&token, "Bash", json!({ "command": "echo Milk. >> todo.txt" })).await });
    let request = w.waiting(&session.id).await;
    assert_eq!((request.tool.as_str(), request.what.as_str()), ("Bash", "Claude Code wants to run: echo Milk. >> todo.txt"));
    w.chat.answer_request(&session.id, &request.id, crate::permissions::Said::Once).unwrap();
    assert_eq!(asking.await.unwrap(), (true, String::new()));
    // It runs it (the stand-in does what the command would) and answers.
    std::fs::write(folder.join("todo.txt"), "Buy stamps.\nMilk.\n").unwrap();
    side.say(json!({ "type": "user", "parent_tool_use_id": null, "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "" }] } })).await;
    side.say(json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "Added milk." } } })).await;
    side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "Added milk.", "total_cost_usd": 0.02, "session_id": "cc-1", "usage": { "input_tokens": 100, "output_tokens": 20 } })).await;

    let answer = w.answered(&session.id).await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "Added milk."), "{:?}", answer.problem);
    assert_eq!(answer.steps.iter().map(|s| (s.tool.as_str(), s.state.as_str())).collect::<Vec<_>>(), [("Bash", "done"), (crate::tools::ALL_IT_CHANGED, "done")]);
    assert_eq!(answer.steps[0].args, json!({ "command": "echo Milk. >> todo.txt" }), "what the person was asked, not the model's first wording");
    // What it cost, as Claude Code said it: shown, and not counted against the month's limit.
    let timings = answer.timings.unwrap();
    assert_eq!((timings.cost_usd, timings.written), (Some(0.02), 20));
    let spending = w.providers.spending().unwrap();
    assert_eq!((spending.spent_usd, spending.by_model[0].counted), (0.0, false));
    // All it changed is one change, undone in one go.
    let change = answer.steps[1].change.clone().unwrap();
    assert_eq!(w.chat.change_diff(&session.id, &change).unwrap(), "--- a/todo.txt\n+++ b/todo.txt\n@@ -1 +1,2 @@\n Buy stamps.\n+Milk.\n");
    w.chat.undo(&session.id, &change).unwrap();
    assert_eq!(std::fs::read_to_string(folder.join("todo.txt")).unwrap(), "Buy stamps.\n");

    // The next turn goes on in the same Claude Code session.
    w.chat.say(&session.id, "Thanks.").unwrap();
    let mut side = sides.recv().await.unwrap();
    assert!(side.args.windows(2).any(|w| w[0] == "--resume" && w[1] == "cc-1"), "{:?}", side.args);
    assert_eq!(side.line().await["message"]["content"], "Thanks.", "only what it has not seen");
    side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "You're welcome.", "total_cost_usd": 0.001, "session_id": "cc-1", "usage": {} })).await;
    assert_eq!(w.answered(&session.id).await.content, "You're welcome.");
    let _ = w.settings;
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_session_on_opencode_uses_its_free_model_and_says_what_it_does_with_what_is_sent() {
    let (spawner, mut sides, env) = stand();
    let w = world(spawner);
    // Choosing asks OpenCode for its models: a stand-in answers.
    let listing = tokio::spawn(async move {
        let side = sides.recv().await.unwrap();
        opencode_side(side, false).await;
        sides
    });
    let chosen = w.providers.choose("opencode", "opencode/deepseek-v4.1-flash-free", true).await.unwrap();
    let mut sides = listing.await.unwrap();
    assert!(chosen.free && chosen.note.as_deref() == Some("Free. OpenCode says it keeps nothing and does not train on what is sent."), "{:?}", chosen.note);
    assert_eq!(chosen.name, "DeepSeek V4.1 Flash");
    let pickle = w.providers.models("opencode", false).await.unwrap().into_iter().find(|m| m.id == "opencode/opencode/big-pickle").unwrap();
    assert!(pickle.note.unwrap().contains("may use what is sent to improve the model"));
    let session = w.chat.start(None, Some("opencode/opencode/deepseek-v4.1-flash-free")).await.unwrap();
    // Cautious: OpenCode's command waits for the person.
    w.chat.set_permission(&session.id, "cautious").unwrap();
    w.chat.say(&session.id, "What is here?").unwrap();
    let side = sides.recv().await.unwrap();
    assert_eq!(side.args, ["acp"]);
    // OpenCode is started with the level's rules, given at start: nothing is written into the folder.
    assert!(env.lock().unwrap().iter().any(|name| name == "OPENCODE_CONFIG_CONTENT"));
    let program = tokio::spawn(opencode_side(side, true));
    let request = w.waiting(&session.id).await;
    assert_eq!((request.tool.as_str(), request.what.as_str()), ("shell", "OpenCode wants to run ls"));
    w.chat.answer_request(&session.id, &request.id, crate::permissions::Said::Once).unwrap();
    let answer = w.answered(&session.id).await;
    program.await.unwrap();
    assert_eq!((answer.state.as_str(), answer.content.as_str(), answer.thought.as_deref()), ("done", "There is one file.", Some("Plan it.")));
    assert_eq!(answer.steps.iter().map(|s| (s.tool.as_str(), s.state.as_str(), s.result.as_str())).collect::<Vec<_>>(), [("shell", "done", "notes.md")]);
    assert_eq!(answer.steps[0].args, json!({ "command": "ls" }), "what the tool was used on, from OpenCode's later update");
    assert_eq!(w.sessions.program_session(&session.id, "opencode").unwrap().map(|k| k.external_id).as_deref(), Some("ses_1"));
    std::fs::remove_dir_all(&w.home).ok();
}

#[test]
fn opencode_is_told_to_ask_by_the_level() {
    use crate::permissions::Level;
    let rules = |level| opencode_env(level).first().map(|(_, v)| serde_json::from_str::<Value>(v).unwrap()["permissions"].clone());
    assert_eq!(rules(Level::Cautious), Some(json!([{ "action": "*", "resource": "*", "effect": "ask" }])));
    let balanced = rules(Level::Balanced).unwrap();
    let asked: Vec<&str> = balanced.as_array().unwrap().iter().map(|r| r["action"].as_str().unwrap()).collect();
    assert_eq!(asked, ["shell", "webfetch", "websearch", "external_directory"]);
    assert_eq!(rules(Level::Autonomous), None, "OpenCode's own rules, untouched");
}

#[test]
fn a_programs_colours_are_taken_out_of_what_it_says() {
    assert_eq!(plain("\u{1b}[1mopencode\u{1b}[0m \u{1b}[2mv\u{1b}[0m\u{1b}[1m2.2.7\u{1b}[0m"), "opencode v2.2.7");
    assert_eq!(plain("2.1.330 (Claude Code)"), "2.1.330 (Claude Code)");
}

#[tokio::test]
async fn a_program_that_is_not_there_is_said_plainly() {
    let found = look("claude-code", None, &[], "Demido has no copy of Claude Code yet.").await;
    assert!(found.path.is_none() && found.problem.unwrap().contains("no copy"));
}

// ───────── Demido's own copies ─────────

/// A stand-in for both makers' download services: Anthropic's releases, OpenCode's version, npm.
async fn publisher(claude: Vec<u8>, claude_sum: String, tgz: Vec<u8>, integrity: String) -> (String, Arc<Mutex<Vec<String>>>) {
    use axum::extract::Request;
    use axum::response::IntoResponse;
    let asked: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = asked.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let here = base.clone();
    let platform = copies::claude_code_platform().unwrap();
    let target = copies::opencode_target().unwrap();
    let binary = if cfg!(windows) { "claude.exe" } else { "claude" };
    let app = axum::Router::new().fallback(move |request: Request| {
        let (seen, claude, claude_sum, tgz, integrity, platform, target, here) =
            (seen.clone(), claude.clone(), claude_sum.clone(), tgz.clone(), integrity.clone(), platform.clone(), target.clone(), here.clone());
        async move {
            let path = request.uri().path().to_string();
            seen.lock().unwrap().push(path.clone());
            if path == "/claude/latest" {
                return "2.1.330\n".into_response();
            }
            if path == "/claude/2.1.330/manifest.json" {
                return axum::Json(json!({ "version": "2.1.330", "platforms": { platform.clone(): { "binary": binary, "checksum": claude_sum, "size": claude.len() } } })).into_response();
            }
            if path == format!("/claude/2.1.330/{platform}/{binary}") {
                return claude.into_response();
            }
            if path == "/opencode-latest" {
                return axum::Json(json!({ "channel": "latest", "version": "2.3.2", "metadata": { "package": "@opencode/cli" } })).into_response();
            }
            if path == format!("/npm/@opencode%2fcli-{target}/2.3.2") {
                return axum::Json(
                    json!({ "name": format!("@opencode/cli-{target}"), "version": "2.3.2", "license": "MIT", "dist": { "tarball": format!("{here}/files/cli.tgz"), "integrity": integrity } }),
                )
                .into_response();
            }
            if path == "/files/cli.tgz" {
                return tgz.into_response();
            }
            axum::http::StatusCode::NOT_FOUND.into_response()
        }
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, asked)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// An npm package as OpenCode's is laid out: `package/bin/opencode`.
fn opencode_package(program: &[u8]) -> (Vec<u8>, String) {
    use base64::Engine;
    use sha2::Digest;
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_size(program.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, format!("package/bin/{}", if cfg!(windows) { "opencode.exe" } else { "opencode" }), program).unwrap();
    let tgz = tar.into_inner().unwrap().finish().unwrap();
    let integrity = format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz)));
    (tgz, integrity)
}

fn copies_at(home: &Path, base: &str) -> Arc<copies::Copies> {
    let db = Arc::new(crate::db::Db::memory().unwrap());
    Arc::new(copies::Copies::new(home, Arc::new(crate::events::EventLog::new(db))).from(base))
}

#[tokio::test]
async fn demidos_copies_come_from_their_makers_checked_and_stay_in_its_folder() {
    let claude = b"#!/bin/sh\necho 'stand-in claude'\n".to_vec();
    let (tgz, integrity) = opencode_package(b"#!/bin/sh\necho 'stand-in opencode'\n");
    let (base, asked) = publisher(claude.clone(), sha256_hex(&claude), tgz, integrity).await;
    let home = std::env::temp_dir().join(format!("demido-copies-{}", uuid::Uuid::new_v4()));
    let c = copies_at(&home, &base);
    assert_eq!(c.kept("claude-code"), None);
    assert_eq!(c.latest("claude-code").await.unwrap().version, "2.1.330");

    let kept = c.download("claude-code").await.unwrap();
    assert_eq!((kept.version.as_str(), kept.size, kept.checked.as_str()), ("2.1.330", claude.len() as u64, "SHA-256 from Anthropic's manifest"));
    assert!(kept.path.starts_with(home.join("programs").join("claude-code").to_str().unwrap()), "{}", kept.path);
    assert_eq!(std::fs::read(&kept.path).unwrap(), claude, "unchanged");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&kept.path).unwrap().permissions().mode() & 0o777, 0o755);
    }
    assert_eq!(c.kept("claude-code"), Some(kept.clone()));
    // Asked again for the same version: nothing is downloaded twice.
    let before = asked.lock().unwrap().iter().filter(|p| p.ends_with("/claude") || p.ends_with("/claude.exe")).count();
    c.download("claude-code").await.unwrap();
    assert_eq!(asked.lock().unwrap().iter().filter(|p| p.ends_with("/claude") || p.ends_with("/claude.exe")).count(), before);
    // It runs with a profile of the account's own, beside it, and does not update itself.
    let env = c.env("claude-code", "a1");
    // Paths are compared as paths: on Windows a path written with `/` is the same path as one with `\`.
    assert!(env.iter().any(|(n, v)| *n == "CLAUDE_CONFIG_DIR" && Path::new(v) == home.join("programs/claude-code/profiles/a1")), "{env:?}");
    assert!(env.contains(&("DISABLE_AUTOUPDATER", "1".to_string())));

    let kept = c.download("opencode").await.unwrap();
    assert_eq!((kept.version.as_str(), kept.checked.as_str()), ("2.3.2", "SHA-512 from npm"));
    assert_eq!(std::fs::read(&kept.path).unwrap(), b"#!/bin/sh\necho 'stand-in opencode'\n");
    assert!(!Path::new(&kept.path).with_file_name("opencode.tgz").exists(), "the package goes once unpacked");
    let env = c.env("opencode", "default");
    assert!(env.iter().any(|(n, v)| *n == "XDG_DATA_HOME" && Path::new(v).starts_with(home.join("programs/opencode/profiles/default"))), "{env:?}");
    assert!(env.contains(&("OPENCODE_DISABLE_AUTOUPDATE", "1".to_string())));

    // Removed: the program goes, its profiles (their settings and logins) stay.
    std::fs::write(home.join("programs/claude-code/profiles/a1/settings.json"), "{}").unwrap();
    c.remove("claude-code").unwrap();
    assert_eq!(c.kept("claude-code"), None);
    assert!(home.join("programs/claude-code/profiles/a1/settings.json").exists());
    // An account's profile goes when the account does.
    c.remove_profile("claude-code", "a1").unwrap();
    assert!(!home.join("programs/claude-code/profiles/a1").exists());
    std::fs::remove_dir_all(&home).ok();
}

#[tokio::test]
async fn a_copy_that_is_not_what_its_maker_published_is_not_kept() {
    let claude = b"the real program".to_vec();
    let (tgz, _) = opencode_package(b"real");
    let (base, _) = publisher(b"something else entirely".to_vec(), sha256_hex(&claude), tgz, "sha512-AAAA".into()).await;
    let home = std::env::temp_dir().join(format!("demido-copies-bad-{}", uuid::Uuid::new_v4()));
    let c = copies_at(&home, &base);
    let said = c.download("claude-code").await.unwrap_err().to_string();
    assert!(said.contains("checksum does not match"), "{said}");
    assert_eq!(c.kept("claude-code"), None);
    let said = c.download("opencode").await.unwrap_err().to_string();
    assert!(said.contains("SHA-512 does not match"), "{said}");
    assert_eq!(c.kept("opencode"), None);
    std::fs::remove_dir_all(&home).ok();
}

#[tokio::test]
async fn only_demidos_own_copy_is_run_and_with_its_own_profile() {
    let claude = b"#!/bin/sh\n".to_vec();
    let (tgz, integrity) = opencode_package(b"#!/bin/sh\n");
    let (base, _) = publisher(claude.clone(), sha256_hex(&claude), tgz, integrity).await;
    let home = std::env::temp_dir().join(format!("demido-copies-only-{}", uuid::Uuid::new_v4()));
    let c = copies_at(&home, &base);
    let db = Arc::new(crate::db::Db::memory().unwrap());
    let events = Arc::new(crate::events::EventLog::new(db.clone()));
    let settings = Arc::new(crate::settings::Settings::load(db.clone(), events.clone()).unwrap());
    let p = crate::providers::Providers::new(db, Arc::new(crate::vault::Vault::in_file(&home)), settings, events).with_copies(c.clone());
    // Not downloaded: nothing is run, whatever is installed on the computer (Claude Code is, here), and no
    // folder is made for it.
    assert!(p.program("claude-code").unwrap_err().to_string().contains("Download it on the Providers screen"));
    assert!(p.look_at("opencode", None).await.path.is_none());
    assert!(!home.join("programs/opencode").exists(), "nothing is made for a program not downloaded");
    c.download("claude-code").await.unwrap();
    assert_eq!(p.program("claude-code").unwrap(), PathBuf::from(c.kept("claude-code").unwrap().path));
    // With no account it is not run at all, and no profile is made for it.
    assert!(p.look_at("claude-code", None).await.problem.unwrap().contains("no account"));
    assert!(!home.join("programs/claude-code/profiles").exists(), "no profile without an account");
    assert!(p.program_env("claude-code", Some("a1")).iter().any(|(n, _)| *n == "CLAUDE_CONFIG_DIR"));
    std::fs::remove_dir_all(&home).ok();
}

#[tokio::test]
async fn a_session_on_claude_code_runs_demidos_copy_with_its_own_profile() {
    let (spawner, mut sides, env) = stand();
    let w = world(spawner);
    w.providers.choose("claude-code", "haiku", true).await.unwrap();
    let session = w.chat.start(None, Some("claude-code/haiku")).await.unwrap();
    w.chat.say(&session.id, "Hi.").unwrap();
    let mut side = sides.recv().await.unwrap();
    {
        let names = env.lock().unwrap();
        assert!(names.iter().any(|n| n == "CLAUDE_CONFIG_DIR") && names.iter().any(|n| n == "DISABLE_AUTOUPDATER"), "{names:?}");
    }
    side.line().await;
    side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "Hello.", "total_cost_usd": 0.0, "session_id": "cc-9", "usage": {} })).await;
    assert_eq!(w.answered(&session.id).await.content, "Hello.");
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn each_claude_code_account_has_its_own_profile_and_a_session_can_change_account() {
    let (spawner, mut sides, _) = stand();
    let w = world(spawner);
    let personal = w.providers.accounts(Some("claude-code")).unwrap().remove(0);
    let work = w.providers.add_account("claude-code", Some("Work"), None).unwrap();
    assert!(matches!(w.providers.add_account("claude-code", Some("Team"), Some("sk-ant-not-for-claude-code")), Err(crate::Error::Invalid(_))));
    w.providers.choose("claude-code", "haiku", true).await.unwrap();
    let session = w.chat.start(None, Some("claude-code/haiku")).await.unwrap();
    let profile = |side: &Side| side.env.iter().find(|(n, _)| n == "CLAUDE_CONFIG_DIR").map(|(_, v)| PathBuf::from(v)).unwrap();

    // Named by no account: the default's profile.
    w.chat.say(&session.id, "What is 2 + 2?").unwrap();
    let mut side = sides.recv().await.unwrap();
    assert_eq!(profile(&side), w.home.join("programs/claude-code/profiles").join(&personal.id));
    side.line().await;
    side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "4.", "total_cost_usd": 0.01, "session_id": "cc-p", "usage": {} })).await;
    assert_eq!(w.answered(&session.id).await.content, "4.");

    // Another account: Claude Code starts again in that account's profile, and is given the conversation.
    assert_eq!(w.chat.set_account(&session.id, Some(&work.id)).unwrap().account.as_deref(), Some(work.id.as_str()));
    assert!(w.chat.set_account(&session.id, Some("nothere")).is_err());
    w.chat.say(&session.id, "And times 3?").unwrap();
    let mut side = sides.recv().await.unwrap();
    assert_eq!(profile(&side), w.home.join("programs/claude-code/profiles").join(&work.id));
    assert!(!side.args.iter().any(|a| a == "--resume"), "the other account's session is not in this profile: {:?}", side.args);
    let said = side.line().await["message"]["content"].as_str().unwrap().to_string();
    assert!(said.contains("Person: What is 2 + 2?") && said.contains("Model: 4.") && said.ends_with("And times 3?"), "{said}");
    side.say(json!({ "type": "result", "subtype": "success", "is_error": false, "result": "12.", "total_cost_usd": 0.02, "session_id": "cc-w", "usage": {} })).await;
    assert_eq!(w.answered(&session.id).await.content, "12.");
    // What each turn cost is counted against its account.
    let view = w.providers.view().await.unwrap();
    let spent: Vec<(String, f64)> = view.iter().find(|v| v.id == "claude-code").unwrap().accounts.iter().map(|a| (a.label.clone(), a.spent_usd)).collect();
    assert_eq!(spent, [("Personal".to_string(), 0.01), ("Work".to_string(), 0.02)]);

    // The account deleted, its profile goes and the session goes back to the default.
    std::fs::write(w.home.join("programs/claude-code/profiles").join(&work.id).join("settings.json"), "{}").unwrap();
    w.providers.remove_account("claude-code", &work.id).await.unwrap();
    assert!(!w.home.join("programs/claude-code/profiles").join(&work.id).exists());
    assert_eq!(w.sessions.get(&session.id).unwrap().account, None);
    std::fs::remove_dir_all(&w.home).ok();
}
