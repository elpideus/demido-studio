//! A `llama.cpp` server Demido starts and owns.
//!
//! It picks the port, launches the process, waits for it to actually answer,
//! keeps the tail of its log, and kills it when the handle goes away.
//!
//! **A failure that explains itself** is most of why this is code rather than
//! documentation telling somebody to run `llama-server` by hand. A server that
//! never comes up has always said why, on stderr, before dying. Holding the
//! last lines of that log turns "inference did not start" into "the model file
//! is for a newer GGUF version", which is the difference between a bug report
//! and a fix.
//!
//! **The binary is not bundled** (`AGENTS.md` hard rule 3): it is fetched from
//! upstream onto the user's machine at set-up. This module only expects to find
//! one at a path, and says exactly where it looked when it does not. Which
//! build that is, where it lives, and how the previous one is deleted all
//! belong to the runtimes work, which is the only place that layout is written
//! down.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::backend::{Backend, Cancel, ChunkStream, Error, Result};
use crate::model::{Chunk, FinishReason, Loaded, Request, Role, Usage};

/// How many stderr lines to keep. Enough to hold the startup banner, which is
/// where the answer usually is, without growing without bound over a long
/// session.
const LOG_LINES: usize = 200;

/// How much of the model goes onto the GPU.
///
/// `--n-gpu-layers` takes `auto`, `all`, or a count, and this mirrors that
/// vocabulary rather than encoding "all" as a very large number. A sentinel
/// count is exactly the bug this shape replaced in v2: `u32::MAX` is not a
/// layer count, the parser rejected it, and what reached the user was the tail
/// of a usage message instead of an explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Offload {
    /// Let `llama.cpp` fit what it can and leave the rest on the CPU. The
    /// default, and the honest one: it knows how much memory the card has free
    /// at load time, and nothing calling this does.
    #[default]
    Auto,
    /// Every layer on the GPU, or fail loudly. What to ask for when the point
    /// is to find out whether the model fits.
    All,
    /// A fixed count. `0` is CPU only, which always works.
    Layers(u32),
}

impl Offload {
    /// The value `--n-gpu-layers` is given.
    pub fn argument(self) -> String {
        match self {
            Offload::Auto => "auto".into(),
            Offload::All => "all".into(),
            Offload::Layers(count) => count.to_string(),
        }
    }
}

/// Everything needed to start one server.
///
/// Deliberately not read from settings here: the caller owns the settings
/// ladder and the VRAM budget, and a backend that read them itself could not be
/// started twice with different numbers, which is what the contract suite does
/// on every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The `llama-server` executable. Fetched from upstream, never bundled.
    pub binary: PathBuf,
    /// The GGUF to load.
    pub model: PathBuf,
    /// What to call this model on the wire. `Request::model` must match it.
    pub alias: String,
    /// A fixed port, or `None` to take whatever the OS has free.
    pub port: Option<u16>,
    pub offload: Offload,
    /// The context **one generation** gets, which is the number the user was
    /// shown. Not what `--ctx-size` is given: see [`arguments`].
    pub context_length: u32,
    /// Concurrent slots.
    pub parallel: u32,
    /// Appended after everything else, so a user can override any of it.
    pub extra_args: Vec<String>,
    /// How long to wait for the first healthy answer. Loading a large model off
    /// a cold disk is genuinely slow, so this is generous by default.
    pub startup_timeout: Duration,
}

impl Config {
    pub fn new(binary: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            model: model.into(),
            alias: "local".into(),
            port: None,
            offload: Offload::Auto,
            context_length: 4096,
            parallel: 1,
            extra_args: Vec::new(),
            startup_timeout: Duration::from_secs(240),
        }
    }
}

/// The full command line, given a resolved port.
///
/// Separate from spawning so it can be asserted on without a binary present,
/// and so it can be logged verbatim: "what did Demido actually run" is the
/// first question every startup failure raises.
pub fn arguments(config: &Config, port: u16) -> Vec<String> {
    let mut args = vec![
        "--model".into(),
        config.model.display().to_string(),
        "--alias".into(),
        config.alias.clone(),
        // Bound to loopback. Nothing here is authenticated, and a local
        // inference server on a shared network is somebody else's GPU.
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--n-gpu-layers".into(),
        config.offload.argument(),
        // `--ctx-size` is the **whole** KV pool, shared out evenly between the
        // slots, so the window one generation gets is that number divided by
        // `--parallel`. Measured on the pinned build b10816: `-c 3072
        // --parallel 2` reports `n_ctx` 1536 per slot, and `-c 4096 --parallel
        // 1` reports 4096.
        //
        // So `context_length` is multiplied here. It is what the user asked a
        // generation to have, and handing `llama.cpp` that number raw gives
        // them a fraction of it, silently, with the fraction depending on a
        // setting about parallelism that has nothing to do with context.
        //
        // Both flags are always sent for the same reason: `--parallel` defaults
        // to `-1`, meaning auto, so omitting it makes the divisor a number
        // nobody chose. See `docs/rules/done.md`, which had this the other way
        // round until the contract test measured it.
        "--ctx-size".into(),
        config
            .context_length
            .saturating_mul(config.parallel.max(1))
            .to_string(),
        "--parallel".into(),
        config.parallel.max(1).to_string(),
        // Use the template embedded in the GGUF rather than a guess from the
        // model name. A guessed template is the single most common cause of a
        // model that answers, but badly.
        "--jinja".into(),
    ];
    args.extend(config.extra_args.iter().cloned());
    args
}

/// A running `llama.cpp` server.
///
/// Dropping this kills the process. That is the point: a supervisor that leaks
/// servers leaves a user's VRAM occupied by a model nothing is using, with no
/// UI anywhere that can see it to offer to stop it.
pub struct LlamaCpp {
    /// Behind a lock so a shared server can still be stopped. `Child::kill`
    /// needs `&mut`, and the supervisor hands the same server to every caller.
    child: tokio::sync::Mutex<Child>,
    port: u16,
    log: Log,
    alias: String,
    size: Option<u64>,
    http: reqwest::Client,
}

impl LlamaCpp {
    /// The loopback port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The last lines the server wrote to stderr.
    ///
    /// What Settings shows, and also how "verify by running" answers whether
    /// layers actually landed on the GPU: `llama.cpp` says so here, and nothing
    /// else can be trusted to.
    pub fn recent_log(&self) -> Vec<String> {
        self.log.tail()
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

#[async_trait::async_trait]
impl Backend for LlamaCpp {
    type Config = Config;

    fn name() -> &'static str {
        "llama.cpp"
    }

    async fn start(config: Config) -> Result<Self> {
        if !config.binary.exists() {
            return Err(did_not_start(format!(
                "no llama-server at {}. It is fetched from upstream at set-up rather than \
                 bundled, so this usually means set-up has not run yet",
                config.binary.display()
            )));
        }
        if !config.model.exists() {
            return Err(did_not_start(format!(
                "no model file at {}",
                config.model.display()
            )));
        }

        let port = match config.port {
            Some(port) => port,
            None => free_port()?,
        };
        let args = arguments(&config, port);
        tracing::info!(binary = %config.binary.display(), ?args, "starting llama.cpp");

        let mut child = Command::new(&config.binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            // A supervisor that outlives its process is a bug; a process that
            // outlives its supervisor is a stranded model in VRAM.
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                did_not_start(format!(
                    "{} could not run: {error}",
                    config.binary.display()
                ))
            })?;

        let log = Log::default();
        if let Some(stderr) = child.stderr.take() {
            let log = log.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "llama.cpp", "{line}");
                    log.push(line);
                }
            });
        }

        let http = reqwest::Client::new();
        let ready = wait_until_ready(
            &http,
            port,
            config.startup_timeout,
            || match child.try_wait() {
                Ok(Some(status)) => Some(format!("it exited with {status}")),
                Ok(None) => None,
                Err(error) => Some(format!("its state could not be read: {error}")),
            },
            &log,
        )
        .await;

        if let Err(error) = ready {
            // Killed first: a server still loading a model would otherwise hold
            // the card while nothing is left to talk to it.
            let _ = child.start_kill();
            return Err(error);
        }

        Ok(Self {
            child: tokio::sync::Mutex::new(child),
            port,
            log,
            alias: config.alias,
            // Read once, here: the file cannot change under a loaded model.
            size: tokio::fs::metadata(&config.model)
                .await
                .ok()
                .map(|meta| meta.len()),
            http,
        })
    }

    /// Asked of the process rather than of `/health`, because a busy server and
    /// a dead one can answer HTTP the same way and only one of them is worth
    /// restarting. A handle to a process that exited looks perfectly healthy
    /// from the outside, which is exactly the case this exists to catch.
    async fn ready(&self) -> bool {
        if !matches!(self.child.lock().await.try_wait(), Ok(None)) {
            return false;
        }
        is_healthy(&self.http, self.port).await
    }

    async fn loaded(&self) -> Result<Loaded> {
        Ok(Loaded {
            id: self.alias.clone(),
            size: self.size,
        })
    }

    /// Read from `/props`, which is the server's own account of what the slot
    /// got, rather than repeated back from the configuration. Repeating it back
    /// would make [`crate::contract`]'s case about it assert that this crate
    /// can remember a number.
    async fn context_length(&self) -> Result<u32> {
        #[derive(serde::Deserialize)]
        struct Props {
            default_generation_settings: Settings,
        }
        #[derive(serde::Deserialize)]
        struct Settings {
            n_ctx: u32,
        }

        let props: Props = self
            .http
            .get(self.url("/props"))
            .send()
            .await
            .map_err(|error| unreachable(error.to_string()))?
            .json()
            .await
            .map_err(|error| Error::Malformed(format!("/props: {error}")))?;
        Ok(props.default_generation_settings.n_ctx)
    }

    async fn generate(&self, request: Request, cancel: Cancel) -> Result<ChunkStream> {
        if request.model != self.alias {
            // A supervised server serves exactly the model it was started with.
            // Answering with that model under another name would put an answer
            // in the log against a model that never produced it.
            return Err(Error::Refused {
                backend: Self::name().to_owned(),
                detail: format!(
                    "this server is serving {}, and was asked for {}",
                    self.alias, request.model
                ),
            });
        }

        let response = self
            .http
            .post(self.url("/v1/chat/completions"))
            .json(&to_wire(&request))
            .send()
            .await
            .map_err(|error| unreachable(error.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            return Err(Error::Refused {
                backend: Self::name().to_owned(),
                detail: format!("{status}: {}", detail.trim()),
            });
        }

        Ok(Box::pin(decode(response.bytes_stream(), cancel)))
    }

    async fn stop(&self) {
        let _ = self.child.lock().await.kill().await;
    }
}

/// Translate a request into the wire shape.
///
/// `stream_options.include_usage` is what makes the token counts arrive at all
/// on a streamed completion, and the counts are what
/// [`docs/design/windows.md`]'s cost axis is built on.
fn to_wire(request: &Request) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = request
        .messages
        .iter()
        .map(|message| {
            serde_json::json!({
                "role": role_name(message.role),
                "content": message.content,
            })
        })
        .collect();

    // Built as a map rather than as a literal that is then unwrapped back into
    // one, so there is no `as_object_mut` that cannot fail but has to be
    // handled anyway.
    let mut body = serde_json::Map::new();
    body.insert("model".into(), serde_json::json!(request.model));
    body.insert("messages".into(), serde_json::json!(messages));
    body.insert("stream".into(), serde_json::json!(true));
    body.insert(
        "stream_options".into(),
        serde_json::json!({ "include_usage": true }),
    );

    let options = &request.options;
    if let Some(temperature) = options.temperature {
        body.insert("temperature".into(), serde_json::json!(temperature));
    }
    if let Some(max_tokens) = options.max_tokens {
        body.insert("max_tokens".into(), serde_json::json!(max_tokens));
    }
    if let Some(seed) = options.seed {
        body.insert("seed".into(), serde_json::json!(seed));
    }
    serde_json::Value::Object(body)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// Server-sent events into chunks.
///
/// The cancel is checked against the byte stream rather than after it, so
/// cancelling drops the HTTP response and the server sees the connection go
/// away. Waiting for the generation to finish and discarding it would leave the
/// card busy for as long as the answer nobody wanted takes.
fn decode(
    bytes: impl futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + 'static,
    cancel: Cancel,
) -> impl futures_core::Stream<Item = Result<Chunk>> + Send {
    async_stream::stream! {
        let mut bytes = Box::pin(bytes);
        let mut buffer = String::new();
        let mut usage = Usage::default();
        let mut reason: Option<FinishReason> = None;
        let mut finished = false;

        loop {
            let next = tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    // One Done, saying it was cancelled, so a caller that reads
                    // to Done needs no second way to learn a turn is over and
                    // the log records a stop rather than a completed answer.
                    yield Ok(Chunk::Done { reason: FinishReason::Cancelled, usage });
                    return;
                }
                next = bytes.next() => next,
            };

            let Some(next) = next else { break };
            let piece = match next {
                Ok(piece) => piece,
                Err(error) => {
                    yield Err(Error::Malformed(error.to_string()));
                    return;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&piece));

            // Frames are separated by a blank line. Anything after the last
            // separator is a partial frame and waits for more bytes.
            while let Some(end) = buffer.find("\n\n") {
                let frame: String = buffer.drain(..end + 2).collect();
                let Some(payload) = frame.lines().find_map(|line| line.strip_prefix("data: "))
                else {
                    continue;
                };
                let payload = payload.trim();

                if payload == "[DONE]" {
                    finished = true;
                    continue;
                }

                let parsed: Frame = match serde_json::from_str(payload) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        yield Err(Error::Malformed(format!("frame: {error}")));
                        return;
                    }
                };

                if let Some(counts) = parsed.usage {
                    usage = Usage {
                        prompt_tokens: counts.prompt_tokens.unwrap_or(0),
                        completion_tokens: counts.completion_tokens.unwrap_or(0),
                    };
                }

                for choice in parsed.choices {
                    if let Some(text) = choice.delta.content.filter(|t| !t.is_empty()) {
                        yield Ok(Chunk::Text { text });
                    }
                    if let Some(text) = choice.delta.reasoning_content.filter(|t| !t.is_empty()) {
                        yield Ok(Chunk::Thinking { text });
                    }
                    if let Some(said) = choice.finish_reason {
                        finished = true;
                        // Held until the stream really ends, so Done stays last.
                        reason = Some(match said.as_str() {
                            "length" => FinishReason::Length,
                            _ => FinishReason::Stop,
                        });
                    }
                }
            }
        }

        if !finished {
            yield Err(Error::Malformed("the stream ended without finishing".into()));
            return;
        }

        yield Ok(Chunk::Done { reason: reason.unwrap_or(FinishReason::Stop), usage });
    }
}

#[derive(serde::Deserialize)]
struct Frame {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Counts>,
}

#[derive(serde::Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Default, serde::Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(serde::Deserialize)]
struct Counts {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

fn did_not_start(detail: impl Into<String>) -> Error {
    Error::DidNotStart {
        backend: LlamaCpp::name().to_owned(),
        detail: detail.into(),
    }
}

fn unreachable(detail: impl Into<String>) -> Error {
    Error::Unreachable {
        backend: LlamaCpp::name().to_owned(),
        detail: detail.into(),
    }
}

/// The same error, with the last few things the server said attached.
///
/// A startup failure that does not quote the server is one the user has to go
/// and reproduce. Adds nothing when the server said nothing, so the message
/// never trails off into a blank quotation.
fn did_not_start_because(detail: String, log: &Log) -> Error {
    let lines = log.tail();
    let tail: Vec<String> = lines
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|line| format!("  {line}"))
        .collect();
    if tail.is_empty() {
        return did_not_start(detail);
    }
    did_not_start(format!("{detail}. It last said:\n{}", tail.join("\n")))
}

/// Whether the server is up *and* has finished loading its model. 200 means
/// loaded; 503 means still loading, which is normal for a long time.
async fn is_healthy(http: &reqwest::Client, port: u16) -> bool {
    match http
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

/// A port nothing is using, by asking the OS for one and letting it go.
///
/// There is a window between letting go and `llama.cpp` binding it. Losing that
/// race shows up as a server that exits immediately saying the address is in
/// use, which `start` reports with that message attached, so it is diagnosable
/// rather than mysterious. Pin [`Config::port`] to avoid it entirely.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|error| did_not_start(format!("no free port: {error}")))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| did_not_start(format!("the port could not be read: {error}")))
}

/// Poll until the server reports itself healthy.
///
/// `exited` is checked every round, so a process that died is reported as a
/// death with its own log rather than as a timeout. Those two failures have
/// nothing in common and telling them apart is most of the diagnosis.
async fn wait_until_ready(
    http: &reqwest::Client,
    port: u16,
    timeout: Duration,
    mut exited: impl FnMut() -> Option<String>,
    log: &Log,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;

    loop {
        if let Some(reason) = exited() {
            return Err(did_not_start_because(
                format!("the server stopped before it was ready: {reason}"),
                log,
            ));
        }

        if is_healthy(http, port).await {
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            return Err(did_not_start_because(
                format!("it did not answer within {}s", timeout.as_secs()),
                log,
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The tail of the server's stderr, shared between the reader task and whoever
/// needs to explain a failure.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<VecDeque<String>>>);

impl Log {
    fn push(&self, line: String) {
        let Ok(mut lines) = self.0.lock() else {
            return;
        };
        if lines.len() == LOG_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn tail(&self) -> Vec<String> {
        match self.0.lock() {
            Ok(lines) => lines.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// A message the wire shape is built from, for tests that do not need a server.
#[cfg(test)]
fn probe_request() -> Request {
    Request {
        model: "tiny".into(),
        messages: vec![
            crate::model::Message::system("You are terse."),
            crate::model::Message::user("Hello."),
        ],
        options: crate::model::Options::default(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    fn config() -> Config {
        let mut config = Config::new("llama-server", "S:/models/tiny.gguf");
        config.alias = "tiny".into();
        config.offload = Offload::Layers(99);
        config.context_length = 8192;
        config
    }

    #[test]
    fn the_command_line_says_what_was_asked_for() {
        let joined = arguments(&config(), 8123).join(" ");

        assert!(joined.contains("--model S:/models/tiny.gguf"), "{joined}");
        assert!(joined.contains("--alias tiny"), "{joined}");
        assert!(joined.contains("--port 8123"), "{joined}");
        assert!(joined.contains("--n-gpu-layers 99"), "{joined}");
        assert!(joined.contains("--ctx-size 8192"), "{joined}");
    }

    #[test]
    fn the_slot_count_is_always_stated() {
        // --parallel defaults to auto, and it is the divisor of --ctx-size, so
        // omitting it leaves the window one generation gets to a number nobody
        // chose.
        let joined = arguments(&config(), 1).join(" ");
        assert!(joined.contains("--parallel 1"), "{joined}");
    }

    #[test]
    fn the_pool_is_multiplied_so_that_one_slot_gets_what_was_asked_for() {
        // The user asked for 8192 of context. --ctx-size is the whole pool and
        // llama.cpp divides it by the slot count, so two slots need 16384 for
        // either of them to have the 8192 the user was shown.
        let mut config = config();
        config.parallel = 2;
        let joined = arguments(&config, 1).join(" ");
        assert!(joined.contains("--ctx-size 16384"), "{joined}");
        assert!(joined.contains("--parallel 2"), "{joined}");
    }

    #[test]
    fn offload_is_spelled_the_way_llama_cpp_reads_it() {
        assert_eq!(Offload::Auto.argument(), "auto");
        assert_eq!(Offload::All.argument(), "all");
        assert_eq!(Offload::Layers(0).argument(), "0");
    }

    #[test]
    fn nothing_offloads_by_a_number_nobody_chose() {
        // The default defers to llama.cpp, which is the only party that knows
        // how much of the card is free at load time.
        assert_eq!(
            Config::new("llama-server", "tiny.gguf").offload,
            Offload::Auto
        );
    }

    #[test]
    fn the_server_is_never_reachable_from_the_network() {
        let args = arguments(&config(), 8123);
        let host = args
            .iter()
            .position(|arg| arg == "--host")
            .and_then(|at| args.get(at + 1))
            .expect("a host");
        assert_eq!(
            host, "127.0.0.1",
            "an unauthenticated inference server must not leave this machine"
        );
    }

    #[test]
    fn a_user_override_wins_because_it_comes_last() {
        let mut config = config();
        config.extra_args = vec!["--n-gpu-layers".into(), "0".into()];
        let args = arguments(&config, 8123);

        assert_eq!(
            args.iter().filter(|arg| *arg == "--n-gpu-layers").count(),
            2,
            "the override is appended, not merged"
        );
        assert_eq!(args.last().expect("an argument"), "0");
    }

    #[test]
    fn no_slots_is_read_as_one_rather_than_as_a_broken_server() {
        let mut config = config();
        config.parallel = 0;
        let args = arguments(&config, 1);
        let at = args
            .iter()
            .position(|arg| arg == "--parallel")
            .expect("slots");
        assert_eq!(args[at + 1], "1");
    }

    #[test]
    fn the_wire_asks_for_the_counts_that_the_cost_axis_is_built_on() {
        let body = to_wire(&probe_request());
        assert_eq!(body["stream"], serde_json::json!(true));
        assert_eq!(
            body["stream_options"]["include_usage"],
            serde_json::json!(true),
            "a streamed completion carries no usage unless it is asked for"
        );
    }

    #[test]
    fn a_sampler_the_request_does_not_carry_is_not_on_the_wire() {
        let mut request = probe_request();
        request.options.seed = None;
        request.options.max_tokens = None;
        let body = to_wire(&request);

        assert!(body.get("seed").is_none(), "{body}");
        assert!(body.get("max_tokens").is_none(), "{body}");
        assert!(body.get("temperature").is_some(), "the default is a value");
    }

    #[tokio::test]
    async fn a_missing_binary_says_where_it_looked() {
        let config = Config::new("S:/nowhere/llama-server.exe", "S:/nowhere/tiny.gguf");
        let error = match LlamaCpp::start(config).await {
            Err(error) => error,
            Ok(_) => panic!("there is no server there"),
        };

        let message = error.to_string();
        assert!(matches!(error, Error::DidNotStart { .. }), "{message}");
        assert!(
            message.contains("S:/nowhere/llama-server.exe"),
            "the path is the whole diagnosis: {message}"
        );
    }

    #[tokio::test]
    async fn a_server_that_died_is_reported_as_a_death_with_its_own_words() {
        let log = Log::default();
        log.push("error: failed to load model 'tiny.gguf'".into());

        let error = wait_until_ready(
            &reqwest::Client::new(),
            1,
            Duration::from_secs(30),
            || Some("exit code: 1".into()),
            &log,
        )
        .await
        .expect_err("a dead process is not ready");

        let message = error.to_string();
        assert!(message.contains("stopped before it was ready"), "{message}");
        assert!(
            message.contains("failed to load model"),
            "the server already explained itself; the error must carry it: {message}"
        );
    }

    #[tokio::test]
    async fn a_silent_server_times_out_rather_than_waiting_forever() {
        let error = wait_until_ready(
            &reqwest::Client::new(),
            // Nothing listens on port 1, so every poll fails.
            1,
            Duration::from_millis(300),
            || None,
            &Log::default(),
        )
        .await
        .expect_err("nothing answered");

        assert!(
            error.to_string().contains("did not answer"),
            "a timeout and a crash are different failures: {error}"
        );
    }

    #[test]
    fn the_log_keeps_the_end_rather_than_the_beginning() {
        let log = Log::default();
        for line in 0..LOG_LINES + 10 {
            log.push(format!("line {line}"));
        }

        let tail = log.tail();
        assert_eq!(tail.len(), LOG_LINES);
        assert_eq!(
            tail.last().expect("a line"),
            &format!("line {}", LOG_LINES + 9),
            "the last thing a dying server said is the useful part"
        );
    }

    #[test]
    fn a_server_that_said_nothing_is_quoted_as_nothing() {
        let quiet = did_not_start_because("it just stopped".into(), &Log::default()).to_string();
        assert!(
            quiet.ends_with("it just stopped"),
            "an empty log must not leave a dangling quotation: {quiet}"
        );
    }
}
