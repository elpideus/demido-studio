//! Models that run somewhere else, with the person's own key: Anthropic, Gemini (Google AI) and OpenRouter.
//!
//! A model elsewhere is named `provider/model` in a session (`anthropic/claude-opus-5-5`,
//! `openrouter/meta-llama/llama-3.3-70b-instruct:free`). A model on this computer is named by its file, which
//! cannot hold a `/`, so the two never mix. The chat asks a model elsewhere the same way it asks one here: the
//! request it builds for the runtime is turned into the provider's own (DECISIONS U4, U5).
//!
//! What it costs is the person's money, so (U6–U9):
//!
//! - **Nothing is sent to a provider that the person did not choose.** A model is used only once it is chosen
//!   on the Providers screen, and only in a session that names it. Demido never picks one by itself.
//! - **Demido counts what each answer cost** and keeps the count (`usage`): OpenRouter's own figure where it
//!   gives one, and otherwise the tokens the provider counted at the prices in `prices`, read on a stated day.
//! - **Demido stops at the month's limit** the person set ("Spend at most", default $10). Before each request
//!   it checks what it has counted this month, with what the request will at least cost. A model that costs
//!   nothing (OpenRouter's free ones) is not stopped.
//! - **What leaves the computer is said.** A session with a model elsewhere says so; a key Demido keeps is
//!   taken out of everything sent (`Vault::hide_in`), and a key is sent only to its own provider.
//!
//! Claude Code and OpenCode are here too, as providers of another kind: programs on this computer that run
//! their own loop (`programs`, `chat/program.rs`). Their models are listed and chosen the same way; they
//! have no key in Demido, and what they cost is shown and held by their own plans, not by the limit.
//!
//! Not built yet: several keys for one provider, and a provider's own spending figures for Anthropic and
//! Google (they have no way to ask with an ordinary key).

mod anthropic;
mod openai;
pub mod prices;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::db::Db;
use crate::engine::{Answer, Piece};
use crate::events::EventLog;
use crate::settings::Settings;
use crate::vault::{KeyFacts, Vault};
use crate::{Error, Result, VERSION, now_ms};

/// A provider Demido can send to.
pub struct Def {
    pub id: &'static str,
    pub name: &'static str,
    /// A program on this computer with its own loop and tools (Claude Code, OpenCode), not an API.
    pub program: bool,
    /// Where its API is.
    pub base: &'static str,
    /// Where the person gets a key.
    pub keys_at: &'static str,
    /// What to know before using it, for a person.
    pub note: &'static str,
}

pub const ALL: [Def; 5] = [
    Def {
        id: "anthropic",
        name: "Anthropic",
        program: false,
        base: "https://api.anthropic.com",
        keys_at: "https://console.anthropic.com/settings/keys",
        note: "Claude, through Anthropic's API with your own key. Billed by Anthropic per token. Demido counts the cost from the tokens Anthropic reports, at the prices it read.",
    },
    Def {
        id: "gemini",
        name: "Gemini (Google AI)",
        program: false,
        base: "https://generativelanguage.googleapis.com/v1beta/openai",
        keys_at: "https://aistudio.google.com/apikey",
        note: "Gemini, through Google's API with your own key from Google AI Studio. On Google's free tier nothing is billed, and Google may use what is sent to improve its products; Demido counts every request at the paid price, so its count is above a free tier's bill. Gemini's newer models always think a little: Demido asks for the least.",
    },
    Def {
        id: "openrouter",
        name: "OpenRouter",
        program: false,
        base: "https://openrouter.ai/api/v1",
        keys_at: "https://openrouter.ai/keys",
        note: "Hundreds of models from many companies, through one key. OpenRouter says what each answer cost, and Demido counts that. Models whose name ends in :free cost nothing and have a daily limit of requests.",
    },
    Def {
        id: "claude-code",
        name: "Claude Code",
        program: true,
        base: "",
        keys_at: "https://code.claude.com/docs/en/setup",
        note: "Runs the Claude Code you installed, signed in with your own Claude plan or key, with its own tools and its own loop: Demido is the window onto it. It uses your plan's limits (its five-hour and weekly ones); Demido shows what it reports and does not count it against the limit you set here. Demido never sees or keeps its login. Not made or endorsed by Anthropic.",
    },
    Def {
        id: "opencode",
        name: "OpenCode",
        program: true,
        base: "",
        keys_at: "https://opencode.ai/docs/",
        note: "Runs the OpenCode you installed, with its own tools and its own loop: Demido is the window onto it. Its free models need no account; several of them use what is sent to improve them, and each says so here. Other models bill through the providers you set up in OpenCode, not through Demido's limit. Used only for your own messages: OpenCode's terms do not allow its free models to be run with nobody there.",
    },
];

/// Claude Code's own names for its models (code.claude.com/docs/en/model-config, read on 2026-11-21).
const CLAUDE_CODE_MODELS: [(&str, &str); 5] =
    [("default", "Claude Code's default"), ("opus", "Opus, the newest"), ("sonnet", "Sonnet, the newest"), ("haiku", "Haiku, the newest"), ("fable", "Fable, the newest")];

/// What OpenCode's models page says each free model does with what is sent to it (opencode.ai/docs/console/
/// models, read on 2026-11-21). Free models come and go; one not here is said to be not known.
const OPENCODE_DATA_USE: [(&str, &str); 11] = [
    ("big-pickle", "it may use what is sent to improve the model, during its free period"),
    ("tidepool-alpha-free", "it keeps what is sent and uses it to improve the model, during its free period"),
    ("iceman-free", "it keeps what is sent and uses it to improve the model, during its free period"),
    ("flint-40b-free", "it may use what is sent to improve the model, and keeps it for 30 days"),
    ("nemotron-3.5-lightning-free", "it is a trial that logs what is sent to improve NVIDIA's products; send nothing personal or confidential"),
    ("nemotron-3.5-super-vl-free", "it is a trial that logs what is sent to improve NVIDIA's products; send nothing personal or confidential"),
    ("mimo-v2.6-flash-free", "it is free for a while so that its makers can collect feedback"),
    ("deepseek-v4.1-flash-free", "it keeps nothing and does not train on what is sent"),
    ("longcat-2.5-free", "it keeps nothing and does not train on what is sent; it is hosted in China, with 100 requests a day"),
    ("hy4-flash-free", "it keeps nothing and does not train on what is sent"),
    ("beam-free", "it does not train on what is sent, and keeps it for 30 days for safety"),
];

/// A model Claude Code offers.
fn claude_code_model(name: &str, label: &str) -> RemoteModel {
    RemoteModel {
        id: format!("claude-code/{name}"),
        provider: "claude-code".into(),
        name: label.into(),
        context_length: None,
        most_written: None,
        price_in: None,
        price_out: None,
        price_from: "plan".into(),
        free: false,
        thinking: ["low", "medium", "high", "xhigh", "max"].iter().map(|l| l.to_string()).collect(),
        always_thinks: false,
        takes: vec![],
        chosen: false,
        note: None,
    }
}

/// A model OpenCode offers, as its value and name.
fn opencode_model(value: &str, name: &str) -> RemoteModel {
    let own = value.strip_prefix("opencode/");
    let free = own.is_some_and(|m| m.ends_with("-free") || m == "big-pickle");
    let note = own.filter(|_| free).map(|m| match OPENCODE_DATA_USE.iter().find(|(id, _)| *id == m) {
        Some((_, said)) => format!("Free. OpenCode says {said}."),
        None => "Free. OpenCode's models page says nothing yet about what this model does with what is sent to it.".to_string(),
    });
    RemoteModel {
        id: format!("opencode/{value}"),
        provider: "opencode".into(),
        // "opencode/Big Pickle", "opencode/Beam Free": the provider and "Free" are said elsewhere.
        name: name.strip_prefix("opencode/").unwrap_or(name).trim_end_matches(" Free").to_string(),
        context_length: None,
        most_written: None,
        price_in: if free { Some(0.0) } else { None },
        price_out: if free { Some(0.0) } else { None },
        price_from: if free { "provider".into() } else { "program".into() },
        free,
        thinking: vec![],
        always_thinks: false,
        takes: vec![],
        chosen: false,
        note,
    }
}

/// The provider of a model elsewhere, and the model's name there. `None` for a model on this computer.
pub fn split(model: &str) -> Option<(&'static Def, &str)> {
    let (provider, name) = model.split_once('/')?;
    ALL.iter().find(|d| d.id == provider).map(|d| (d, name)).filter(|(_, name)| !name.is_empty())
}

/// A model at a provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RemoteModel {
    /// The name a session uses: `provider/model`.
    pub id: String,
    pub provider: String,
    /// The provider's own name for it, for a person.
    pub name: String,
    /// The most it can hold in mind, in tokens, as the provider says.
    pub context_length: Option<u64>,
    /// The most it writes in one answer, in tokens.
    pub most_written: Option<u64>,
    /// Dollars per million tokens read and written.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
    /// Where the prices come from: `provider` (it says so), `table` (Demido's table, read on a stated
    /// day), `highest` (not in the table: the provider's highest is counted), `plan` (billed by the
    /// person's plan with the program's maker), `program` (billed through the program's own providers), or
    /// `unknown`.
    pub price_from: String,
    /// It costs nothing.
    pub free: bool,
    /// What thinking can be set to besides `off`: its levels, or `["on"]`. Empty: it does not think.
    pub thinking: Vec<String>,
    /// It thinks however it is asked: `off` asks for the least it does.
    pub always_thinks: bool,
    /// What it takes in besides text: `images`.
    pub takes: Vec<String>,
    /// The person chose it: it may be used in a session.
    pub chosen: bool,
    /// What to know before using it: what a free model does with what is sent to it.
    #[serde(default)]
    pub note: Option<String>,
}

/// A provider as the person sees it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    /// `api` (Demido asks the model itself) or `program` (a program on this computer runs its own loop).
    pub kind: String,
    /// For a program: whether it is here, its version, how it is signed in.
    pub program: Option<crate::programs::Found>,
    /// For a program: its plan's limit, as it last said it while running.
    pub plan: Option<PlanLimit>,
    pub keys_at: String,
    pub note: String,
    /// The key, when one is kept. Never its value.
    pub key: Option<KeyFacts>,
    /// The models the person chose here.
    pub chosen: Vec<RemoteModel>,
    /// What Demido counted this month for this provider, in dollars.
    pub spent_usd: f64,
    /// Whether all of that is the provider's own figure.
    pub spent_exact: bool,
}

/// A program's plan limit, as it last said it (Claude Code's `rate_limit_event`).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PlanLimit {
    /// `allowed`, `allowed_warning` or `rejected`.
    pub status: String,
    /// When the limit resets, in milliseconds since 1970.
    pub resets_at: Option<i64>,
    /// How much of it is used, from 0 to 1, when it says.
    pub used: Option<f64>,
    /// When it said so.
    pub said_at: i64,
}

/// What Demido counted.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Spending {
    /// When this month began, in milliseconds since 1970 (this computer's time zone).
    pub since: i64,
    /// When it ends: the count starts again then.
    pub until: i64,
    /// The month's limit in dollars. `null`: no limit.
    pub limit_usd: Option<f64>,
    pub spent_usd: f64,
    /// Whether all of it is the providers' own figures. False when any of it is Demido's count from tokens.
    pub exact: bool,
    /// When the prices Demido counts with were read.
    pub prices_read_on: String,
    pub by_model: Vec<Spent>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Spent {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    /// Tokens read (cached ones included) and written.
    pub input: u64,
    pub output: u64,
    pub cost_usd: f64,
    pub exact: bool,
    /// Whether it counts toward the month's limit. A program's use does not: its plan holds it.
    pub counted: bool,
}

/// What a provider itself says about the key's account, line by line.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Account {
    pub provider: String,
    /// One line each: what is used, of what, when it resets.
    pub lines: Vec<AccountLine>,
    /// What the provider does not say, for a person.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AccountLine {
    pub label: String,
    pub used: f64,
    /// `null` when there is no limit.
    pub limit: Option<f64>,
    /// `$` or `requests`.
    pub unit: String,
    pub reset: Option<String>,
}

/// What a request is for, so its cost is counted against the right session and answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct For<'a> {
    pub session: Option<&'a str>,
    pub message: Option<&'a str>,
}

/// How much thinking the chat asked for, read from the request it built.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Think {
    Off,
    On,
    Level(String),
}

impl Think {
    fn of(body: &Value) -> Self {
        let effort = body["reasoning_effort"].as_str();
        if effort == Some("none") || body["chat_template_kwargs"]["enable_thinking"] == false {
            Think::Off
        } else if let Some(level) = effort {
            Think::Level(level.to_string())
        } else {
            Think::On
        }
    }
}

/// What a provider sent back for one request.
#[derive(Clone, Debug, Default)]
pub(crate) struct Heard {
    pub content: String,
    pub thought: String,
    pub tokens: prices::Tokens,
    /// The provider's own figure for what it cost, when it gives one.
    pub cost: Option<f64>,
    pub stopped: bool,
    /// Why the model stopped, as the provider said it.
    pub ended: Option<String>,
}

/// Why a request failed.
pub(crate) enum Failed {
    /// The provider did not take the request as it was (400). Asking without a part it may not know can help.
    Shape(String),
    Other(Error),
}

impl From<Error> for Failed {
    fn from(e: Error) -> Self {
        Failed::Other(e)
    }
}

/// What a request leaves out, after the provider said no to it as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Plainer {
    /// Without the shape the reply is held to: the instructions still ask for it.
    pub no_shape: bool,
    /// Without a way of thinking the provider may not take for this model.
    pub plain_thinking: bool,
}

/// One request to a provider, with all it needs.
pub(crate) struct Ask<'a> {
    pub http: &'a reqwest::Client,
    pub base: &'a str,
    pub key: &'a str,
    /// The model's name at the provider.
    pub name: &'a str,
    pub facts: &'a RemoteModel,
    /// The request as the chat built it for the runtime on this computer.
    pub body: &'a Value,
    pub think: &'a Think,
    pub plain: Plainer,
}

/// Lists are asked of a provider again after this long.
const LIST_KEPT_FOR: Duration = Duration::from_secs(3600);

/// Words per token, roughly, for a request no one has counted yet. On the generous side for a cost.
fn roughly(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

pub struct Providers {
    db: Arc<Db>,
    vault: Arc<Vault>,
    settings: Arc<Settings>,
    events: Arc<EventLog>,
    http: reqwest::Client,
    /// Where each provider's API is. A test points them at a stand-in.
    bases: HashMap<&'static str, String>,
    /// Each provider's list of models, as it was last asked.
    listed: Mutex<HashMap<String, (Instant, Vec<RemoteModel>)>>,
    /// Models whose provider said no to a way of thinking: asked the plain way from then on.
    plain: Mutex<HashMap<String, Plainer>>,
    /// Starts the programs. A test gives a stand-in.
    spawner: Arc<dyn crate::programs::Spawner>,
    /// Where OpenCode is asked for its list of models: a folder of Demido's own.
    listing_in: std::path::PathBuf,
    /// What each program said about itself, and when.
    found: Mutex<HashMap<String, (Instant, crate::programs::Found)>>,
    /// Each program's plan limit, as it last said it.
    plans: Mutex<HashMap<String, PlanLimit>>,
}

/// A program is looked at again after this long.
const PROGRAM_LOOKED_AT_FOR: Duration = Duration::from_secs(60);

/// The first moment of this month and of the next, in this computer's time zone.
pub fn this_month() -> (i64, i64) {
    let now = jiff::ZonedDateTime::now();
    let start = now.first_of_month().and_then(|d| d.start_of_day()).unwrap_or(now.clone());
    let next = start.checked_add(jiff::Span::new().months(1)).unwrap_or(now);
    (start.timestamp().as_millisecond(), next.timestamp().as_millisecond())
}

impl Providers {
    pub fn new(db: Arc<Db>, vault: Arc<Vault>, settings: Arc<Settings>, events: Arc<EventLog>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("DemidoStudio/{VERSION} (+https://github.com/elpideus/demido-studio)"))
            .connect_timeout(Duration::from_secs(20))
            // A long answer is not a failure; two minutes of silence is.
            .read_timeout(Duration::from_secs(180))
            .build()
            .expect("an HTTP client with these settings can always be made");
        #[allow(unused_mut)]
        let mut bases: HashMap<&'static str, String> = ALL.iter().map(|d| (d.id, d.base.to_string())).collect();
        // A development build can be pointed at stand-ins on this computer (`core/dev/providers-stand-in.py`),
        // to try the whole of it without a key or money. A release build has no such switch: where a key is
        // sent is not something to leave open.
        #[cfg(debug_assertions)]
        if let Ok(base) = std::env::var("DEMIDO_DEV_PROVIDERS") {
            let base = base.trim_end_matches('/');
            tracing::warn!("Development build: providers are stand-ins at {base}, not the real ones.");
            for d in &ALL {
                bases.insert(d.id, format!("{base}/{}", d.id));
            }
        }
        Self {
            db,
            vault,
            settings,
            events,
            http,
            bases,
            listed: Mutex::default(),
            plain: Mutex::default(),
            spawner: Arc::new(crate::programs::ProcessSpawner),
            listing_in: std::env::temp_dir().join("demido-program-listing"),
            found: Mutex::default(),
            plans: Mutex::default(),
        }
    }

    /// The same, asking OpenCode for its models in a folder under `home`, and starting programs with `spawner`.
    pub fn with_programs(mut self, home: &std::path::Path, spawner: Arc<dyn crate::programs::Spawner>) -> Self {
        self.listing_in = home.join("programs").join("listing");
        self.spawner = spawner;
        self
    }

    pub fn spawner(&self) -> &Arc<dyn crate::programs::Spawner> {
        &self.spawner
    }

    /// The path the person set for a program, if they set one.
    pub fn program_path_set(&self, provider: &str) -> Option<String> {
        let id = if provider == "claude-code" { "providers.claudeCodePath" } else { "providers.opencodePath" };
        self.settings.value::<String>(id).ok().filter(|s| !s.trim().is_empty())
    }

    /// Where a program is, or why it cannot be used.
    pub fn program(&self, provider: &str) -> Result<std::path::PathBuf> {
        let set = self.program_path_set(provider);
        crate::programs::find(provider, set.as_deref()).ok_or_else(|| {
            let name = if provider == "claude-code" { "Claude Code" } else { "OpenCode" };
            Error::Invalid(format!("{name} is not on this computer, or not where Demido looks. Install it from its makers, or set its path in Settings › Providers and spending."))
        })
    }

    /// What a program says about itself, looked at again after a minute.
    pub async fn look_at(&self, provider: &str) -> crate::programs::Found {
        let kept = self.found.lock().unwrap_or_else(|e| e.into_inner()).get(provider).filter(|(at, _)| at.elapsed() < PROGRAM_LOOKED_AT_FOR).map(|(_, f)| f.clone());
        if let Some(found) = kept {
            return found;
        }
        let found = crate::programs::look(provider, self.program_path_set(provider).as_deref()).await;
        self.found.lock().unwrap_or_else(|e| e.into_inner()).insert(provider.to_string(), (Instant::now(), found.clone()));
        found
    }

    /// A program's plan limit, as it said it while it ran.
    pub fn note_plan(&self, provider: &str, status: &str, resets_at: Option<i64>, used: Option<f64>) {
        let limit = PlanLimit { status: status.into(), resets_at: resets_at.map(|s| if s < 100_000_000_000 { s * 1000 } else { s }), used, said_at: now_ms() };
        self.plans.lock().unwrap_or_else(|e| e.into_inner()).insert(provider.to_string(), limit);
        self.events.tell("provider.plan", Some(provider), json!({ "status": status, "resets_at": resets_at, "used": used }));
    }

    /// Writes down what a request to a program cost, as the program said. It is shown, and does not count
    /// toward the month's limit.
    pub fn record(&self, provider: &str, model: &str, asking: For<'_>, tokens: prices::Tokens, cost: f64) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO usage (at, provider, model, session_id, message_id, input, cached, cache_written, output, cost_usd, exact) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
                rusqlite::params![now_ms(), provider, model, asking.session, asking.message, tokens.input as i64, tokens.cached as i64, tokens.cache_written as i64, tokens.output as i64, cost],
            )
        })?;
        self.events.tell("provider.used", Some(provider), json!({ "provider": provider, "cost_usd": cost, "exact": false, "session": asking.session }));
        Ok(())
    }

    /// The same, with every provider's API at `base` (`base/anthropic`, `base/gemini`, …). For tests.
    #[cfg(test)]
    pub(crate) fn at(mut self, base: &str) -> Self {
        for d in &ALL {
            self.bases.insert(d.id, format!("{base}/{}", d.id));
        }
        self
    }

    pub fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }

    fn base(&self, provider: &str) -> &str {
        self.bases.get(provider).map(String::as_str).unwrap_or_default()
    }

    /// The month's limit in dollars. `None`: no limit.
    pub fn limit(&self) -> Option<f64> {
        let chosen = self.settings.value::<String>("providers.monthlyLimit").unwrap_or_else(|_| "10".into());
        if chosen == "none" { None } else { Some(chosen.parse().unwrap_or(10.0)) }
    }

    fn key(&self, def: &Def) -> Result<String> {
        self.vault.get(def.id)?.ok_or_else(|| Error::Invalid(format!("There is no {} key yet. Add yours on the Providers screen (you get one at {}).", def.name, def.keys_at)))
    }

    // ───────── the models ─────────

    /// The models a provider has, as it says. Asked again after an hour, or when `fresh`.
    pub async fn models(&self, provider: &str, fresh: bool) -> Result<Vec<RemoteModel>> {
        let Some(def) = ALL.iter().find(|d| d.id == provider) else {
            return Err(Error::NotFound(format!("Demido knows no provider called \"{provider}\". It knows: {}.", ALL.iter().map(|d| d.id).collect::<Vec<_>>().join(", "))));
        };
        let kept = self.listed.lock().unwrap_or_else(|e| e.into_inner()).get(provider).filter(|(at, _)| !fresh && at.elapsed() < LIST_KEPT_FOR).map(|(_, list)| list.clone());
        let mut list = match kept {
            Some(list) => list,
            None if def.id == "claude-code" => CLAUDE_CODE_MODELS.iter().map(|(name, label)| claude_code_model(name, label)).collect(),
            None if def.id == "opencode" => {
                // OpenCode says which models it offers when a session is made; one is made in a folder of
                // Demido's own, read and deleted. Nothing is sent to a model.
                let program = self.program("opencode")?;
                std::fs::create_dir_all(&self.listing_in)?;
                let started = self.spawner.spawn(&program, &["acp".to_string()], &self.listing_in, &[])?;
                let list: Vec<RemoteModel> = crate::programs::opencode_models(started, &self.listing_in).await?.iter().map(|(value, name)| opencode_model(value, name)).collect();
                self.listed.lock().unwrap_or_else(|e| e.into_inner()).insert(provider.to_string(), (Instant::now(), list.clone()));
                list
            }
            None => {
                // OpenRouter lists its models to anyone; the others want the key.
                let key = if def.id == "openrouter" { self.vault.get(def.id)?.unwrap_or_default() } else { self.key(def)? };
                let list = match def.id {
                    "anthropic" => anthropic::list(&self.http, self.base(def.id), &key).await?,
                    _ => openai::list(def.id, &self.http, self.base(def.id), &key).await?,
                };
                self.listed.lock().unwrap_or_else(|e| e.into_inner()).insert(provider.to_string(), (Instant::now(), list.clone()));
                list
            }
        };
        let chosen: Vec<String> = self.chosen()?.into_iter().map(|m| m.id).collect();
        for m in &mut list {
            m.chosen = chosen.contains(&m.id);
        }
        Ok(list)
    }

    /// The models the person chose, as they were when chosen.
    pub fn chosen(&self) -> Result<Vec<RemoteModel>> {
        let rows: Vec<String> = self.db.with(|c| c.prepare("SELECT facts FROM remote_models ORDER BY id")?.query_map([], |r| r.get(0))?.collect())?;
        Ok(rows
            .into_iter()
            .filter_map(|facts| serde_json::from_str::<RemoteModel>(&facts).ok())
            .map(|mut m| {
                m.chosen = true;
                m
            })
            .collect())
    }

    /// One chosen model, by the name a session uses.
    pub fn chosen_one(&self, id: &str) -> Result<Option<RemoteModel>> {
        Ok(self.chosen()?.into_iter().find(|m| m.id == id))
    }

    /// Chooses a model of a provider, so that sessions may use it; or lets it go (`chosen` false).
    pub async fn choose(&self, provider: &str, model: &str, chosen: bool) -> Result<RemoteModel> {
        // A model may be named as a session names it (`provider/model`) or by its name at the provider, which
        // can itself begin with the provider's name (OpenCode's own models are `opencode/…`): both are tried.
        let named = [format!("{provider}/{model}"), model.to_string()];
        if !chosen {
            let mut gone = None;
            for id in &named {
                if let Some(found) = self.chosen_one(id)? {
                    gone = Some(found);
                    break;
                }
            }
            let Some(mut gone) = gone else {
                return Err(Error::NotFound(format!("{model} is not chosen.")));
            };
            let id = gone.id.clone();
            self.db.with(|c| c.execute("DELETE FROM remote_models WHERE id = ?1", [&id]))?;
            self.events.append("provider.model.dropped", Some(&id), json!({ "provider": provider }))?;
            gone.chosen = false;
            return Ok(gone);
        }
        let list = self.models(provider, false).await?;
        let Some(mut found) = named.iter().find_map(|id| list.iter().find(|m| &m.id == id).cloned()) else {
            let name = ALL.iter().find(|d| d.id == provider).map_or(provider, |d| d.name);
            return Err(Error::NotFound(format!("{name} has no model called \"{model}\", as far as its list says.")));
        };
        let id = found.id.clone();
        found.chosen = true;
        let facts = serde_json::to_string(&found)?;
        self.db.with(|c| {
            c.execute("INSERT INTO remote_models (id, provider, facts, chosen_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (id) DO UPDATE SET facts = ?3", rusqlite::params![id, provider, facts, now_ms()])
        })?;
        self.events.append("provider.model.chosen", Some(&id), json!({ "provider": provider, "free": found.free, "price_in": found.price_in, "price_out": found.price_out }))?;
        Ok(found)
    }

    // ───────── what it cost ─────────

    /// What Demido counted this month, in all and by model.
    pub fn spending(&self) -> Result<Spending> {
        let (since, until) = this_month();
        let by_model: Vec<Spent> = self.db.with(|c| {
            c.prepare("SELECT provider, model, count(*), sum(input + cached), sum(output), sum(cost_usd), min(exact) FROM usage WHERE at >= ?1 GROUP BY provider, model ORDER BY sum(cost_usd) DESC")?
                .query_map([since], |r| {
                    let provider: String = r.get(0)?;
                    let counted = !ALL.iter().any(|d| d.id == provider && d.program);
                    Ok(Spent {
                        provider,
                        model: r.get(1)?,
                        requests: r.get::<_, i64>(2)? as u64,
                        input: r.get::<_, i64>(3)? as u64,
                        output: r.get::<_, i64>(4)? as u64,
                        cost_usd: r.get(5)?,
                        exact: r.get::<_, i64>(6)? == 1,
                        counted,
                    })
                })?
                .collect()
        })?;
        Ok(Spending {
            since,
            until,
            limit_usd: self.limit(),
            // A sum of nothing is -0.0 in Rust; + 0.0 makes it the zero a person reads.
            spent_usd: by_model.iter().filter(|s| s.counted).map(|s| s.cost_usd).sum::<f64>() + 0.0,
            exact: by_model.iter().filter(|s| s.counted).all(|s| s.exact),
            prices_read_on: prices::READ_ON.into(),
            by_model,
        })
    }

    /// What the requests made for one answer cost, and whether all of it is the providers' own figure.
    pub fn cost_of(&self, message: &str) -> Result<Option<(f64, bool)>> {
        self.db.with(|c| {
            c.query_row("SELECT sum(cost_usd), min(exact) FROM usage WHERE message_id = ?1", [message], |r| {
                Ok(r.get::<_, Option<f64>>(0)?.map(|cost| (cost, r.get::<_, Option<i64>>(1).ok().flatten() == Some(1))))
            })
        })
    }

    /// The providers, each with its key (never its value), its chosen models and what it cost this month.
    pub async fn view(&self) -> Result<Vec<ProviderView>> {
        let spending = self.spending()?;
        let chosen = self.chosen()?;
        let mut programs = HashMap::new();
        for d in ALL.iter().filter(|d| d.program) {
            programs.insert(d.id, self.look_at(d.id).await);
        }
        let plans = self.plans.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Ok(ALL
            .iter()
            .map(|d| {
                let mine: Vec<&Spent> = spending.by_model.iter().filter(|s| s.provider == d.id).collect();
                ProviderView {
                    id: d.id.into(),
                    name: d.name.into(),
                    kind: if d.program { "program" } else { "api" }.into(),
                    program: programs.get(d.id).cloned(),
                    plan: plans.get(d.id).cloned(),
                    keys_at: d.keys_at.into(),
                    note: d.note.into(),
                    key: self.vault.facts(d.id),
                    chosen: chosen.iter().filter(|m| m.provider == d.id).cloned().collect(),
                    spent_usd: mine.iter().map(|s| s.cost_usd).sum::<f64>() + 0.0,
                    spent_exact: mine.iter().all(|s| s.exact),
                }
            })
            .collect())
    }

    /// What a provider itself says about the key's account. Only OpenRouter can be asked this with an
    /// ordinary key; for the others it says where to look.
    pub async fn account(&self, provider: &str) -> Result<Account> {
        let Some(def) = ALL.iter().find(|d| d.id == provider) else {
            return Err(Error::NotFound(format!("Demido knows no provider called \"{provider}\".")));
        };
        match def.id {
            "openrouter" => openai::account(&self.http, self.base(def.id), &self.key(def)?).await,
            "claude-code" => Ok(Account {
                provider: provider.into(),
                lines: vec![],
                note: Some(
                    "Claude Code says how much of your plan's limit is used while it runs, and Demido shows the last it said. Your plan's usage page shows the rest: claude.ai/settings/usage.".into(),
                ),
            }),
            "opencode" => Ok(Account {
                provider: provider.into(),
                lines: vec![],
                note: Some("OpenCode's free models have limits of their own that OpenCode does not tell. `opencode stats` in a terminal shows what OpenCode itself counted.".into()),
            }),
            "anthropic" => Ok(Account {
                provider: provider.into(),
                lines: vec![],
                note: Some("Anthropic does not tell an ordinary key what it has spent. Its console shows your balance and limits: console.anthropic.com/settings/billing.".into()),
            }),
            _ => Ok(Account {
                provider: provider.into(),
                lines: vec![],
                note: Some("Google does not tell a key what it has spent. Google AI Studio shows your tier, limits and billing: aistudio.google.com/usage.".into()),
            }),
        }
    }

    // ───────── asking ─────────

    /// Whether a model elsewhere may be asked now: chosen, its key kept, and the month's limit not reached.
    pub fn ready(&self, model: &str) -> Result<RemoteModel> {
        let Some((def, _)) = split(model) else {
            return Err(Error::NotFound(format!("\"{model}\" is not a model of a provider.")));
        };
        let Some(facts) = self.chosen_one(model)? else {
            return Err(Error::Invalid(format!("{model} is not chosen. Choose it on the Providers screen first: nothing is sent to a model you did not choose.")));
        };
        // A program has no key in Demido (its login is its own) and is held by its own plan, not by the limit.
        if def.program {
            self.program(def.id)?;
            return Ok(facts);
        }
        if self.vault.facts(def.id).is_none() {
            return Err(Error::Invalid(format!("There is no {} key yet. Add yours on the Providers screen (you get one at {}).", def.name, def.keys_at)));
        }
        self.within_limit(&facts, 0)?;
        Ok(facts)
    }

    /// Says no when what was counted this month, and what this request costs at least, is past the limit.
    fn within_limit(&self, model: &RemoteModel, tokens_in: u64) -> Result<()> {
        if model.free {
            return Ok(());
        }
        let Some(limit) = self.limit() else { return Ok(()) };
        let spending = self.spending()?;
        let at_least = model.price_in.unwrap_or(0.0) * tokens_in as f64 / 1_000_000.0;
        if spending.spent_usd + at_least >= limit {
            let next = jiff::Timestamp::from_millisecond(spending.until).map(|t| t.to_zoned_datetime(jiff::tz::TimeZone::system()).strftime("%-d %B").to_string()).unwrap_or_default();
            return Err(Error::Invalid(format!(
                "Demido has counted ${:.2} spent this month on models elsewhere{}, and the limit is ${limit:.2}. Nothing more is sent until {next}, or until you raise \"Spend at most\" in Settings.",
                spending.spent_usd,
                if at_least > 0.005 { format!(" and this request would cost at least ${at_least:.2} more") } else { String::new() }
            )));
        }
        Ok(())
    }

    /// Asks a model elsewhere, and passes its answer on piece by piece. `body` is the request as the
    /// runtime on this computer takes it: it is turned into the provider's own.
    pub async fn answer(&self, model: &str, body: Value, stop: &AtomicBool, heard: &mut (dyn FnMut(Piece) + Send), asking: For<'_>) -> Result<Answer> {
        let facts = self.ready(model)?;
        let (def, name) = split(model).expect("ready checked it");
        let key = self.key(def)?;
        // What is sent, with every kept key taken out: a key goes only to its own provider, in its header.
        let mut body = body;
        if let Some(messages) = body["messages"].as_array_mut() {
            for m in messages {
                if let Some(text) = m["content"].as_str() {
                    m["content"] = json!(self.vault.hide_in(text));
                }
            }
        }
        let tokens_in: u64 = body["messages"].as_array().into_iter().flatten().filter_map(|m| m["content"].as_str()).map(roughly).sum();
        self.within_limit(&facts, tokens_in)?;
        let think = Think::of(&body);
        let mut plain = self.plain.lock().unwrap_or_else(|e| e.into_inner()).get(model).copied().unwrap_or_default();
        let started = Instant::now();
        let mut said = None;
        for _ in 0..3 {
            let ask = Ask { http: &self.http, base: self.base(def.id), key: &key, name, facts: &facts, body: &body, think: &think, plain };
            let tried = match def.id {
                "anthropic" => anthropic::ask(&ask, stop, heard).await,
                _ => openai::ask(def.id, &ask, stop, heard).await,
            };
            match tried {
                Ok(h) => {
                    said = Some(h);
                    break;
                }
                // The provider did not take the request as it was. Without the way of thinking first, then
                // without the shape of the reply; what was learned is kept for the next request.
                Err(Failed::Shape(why)) if !plain.plain_thinking && (why.contains("think") || why.contains("reasoning") || why.contains("effort")) => {
                    tracing::info!("{model} did not take the way of thinking it was asked for, and is asked again without it: {why}");
                    plain.plain_thinking = true;
                }
                Err(Failed::Shape(why)) if !plain.no_shape && body.get("response_format").is_some() => {
                    tracing::info!("{model} did not take the shape of the reply, and is asked again without it: {why}");
                    plain.no_shape = true;
                }
                Err(Failed::Shape(why)) => return Err(Error::Invalid(format!("{} did not take the request: {why}", def.name))),
                Err(Failed::Other(e)) => return Err(e),
            }
        }
        let Some(said) = said else {
            return Err(Error::Other(format!("{} did not take the request, however it was asked.", def.name)));
        };
        if plain != Plainer::default() {
            self.plain.lock().unwrap_or_else(|e| e.into_inner()).insert(model.to_string(), plain);
        }
        let took_ms = started.elapsed().as_millis() as u64;
        // What it cost: the provider's own figure, or the tokens at the table's price.
        let (cost, exact) = match said.cost {
            Some(cost) => (cost, true),
            None if facts.free => (0.0, true),
            None => match prices::of(def.id, name) {
                Some((price, _)) => (prices::cost(&price, &said.tokens, prices::ANTHROPIC_CACHE_WRITE), false),
                None => (prices::cost(&prices::Price { input: facts.price_in.unwrap_or(0.0), output: facts.price_out.unwrap_or(0.0), cached: 1.0, long_after: None }, &said.tokens, 1.0), false),
            },
        };
        let t = said.tokens;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO usage (at, provider, model, session_id, message_id, input, cached, cache_written, output, cost_usd, exact) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![now_ms(), def.id, name, asking.session, asking.message, t.input as i64, t.cached as i64, t.cache_written as i64, t.output as i64, cost, exact as i64],
            )
        })?;
        self.events.tell("provider.used", Some(model), json!({ "provider": def.id, "cost_usd": cost, "exact": exact, "session": asking.session }));
        if said.ended.as_deref() == Some("refusal") && said.content.trim().is_empty() {
            return Err(Error::Invalid(format!("{} declined to answer this: its own safety rules stopped it.", facts.name)));
        }
        let per_second = if took_ms > 0 { t.output as f64 / (took_ms as f64 / 1000.0) } else { 0.0 };
        let timings = crate::sessions::Timings { reused: t.cached, read: t.input + t.cache_written, written: t.output, per_second, took_ms, ..Default::default() };
        Ok(Answer { content: said.content, thought: said.thought, timings, stopped: said.stopped, ..Answer::default() })
    }
}

/// Reads a stream of events (`data: {…}` lines) until it ends, `stop` is set, or `each` says it failed.
/// True when it was stopped.
async fn read_events(mut res: reqwest::Response, stop: &AtomicBool, mut each: impl FnMut(&Value) -> std::result::Result<(), Failed>) -> std::result::Result<bool, Failed> {
    let mut kept = String::new();
    loop {
        let piece = tokio::select! {
            piece = res.chunk() => piece,
            _ = async { while !stop.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(40)).await } } => return Ok(true),
        };
        let Some(piece) = piece.map_err(|e| Failed::Other(Error::Net(format!("The answer broke off: {e}"))))? else { return Ok(false) };
        kept.push_str(&String::from_utf8_lossy(&piece));
        while let Some(end) = kept.find('\n') {
            let line: String = kept.drain(..=end).collect();
            let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else { continue };
            if data == "[DONE]" {
                return Ok(false);
            }
            if let Ok(said) = serde_json::from_str::<Value>(data) {
                each(&said)?;
            }
        }
    }
}

/// A provider's error, in words: what its answer said, or its status.
fn error_words(said: &Value, status: reqwest::StatusCode) -> String {
    // Google sends its error in a list of one; the others as one object.
    let error = if said.is_array() { &said[0]["error"] } else { &said["error"] };
    error["message"].as_str().or_else(|| error.as_str()).map(str::to_string).unwrap_or_else(|| format!("it answered {status}"))
}

/// The usual refusals, for a person, the same for every provider.
fn refused(provider: &str, status: reqwest::StatusCode, said: &Value, retry_after: Option<&str>) -> Failed {
    let words = error_words(said, status);
    let name = ALL.iter().find(|d| d.id == provider).map_or(provider, |d| d.name);
    let lower = words.to_lowercase();
    let e = match status.as_u16() {
        401 => Error::Invalid(format!("{name} did not accept the key. Check it, or add it again, on the Providers screen. ({words})")),
        400 if lower.contains("api key") || lower.contains("api_key") => Error::Invalid(format!("{name} did not accept the key. Check it, or add it again, on the Providers screen. ({words})")),
        400 if lower.contains("credit balance") => Error::Invalid(format!("Your {name} credit is too low for this request. Add credit with {name}. ({words})")),
        400 | 422 => return Failed::Shape(words),
        402 => Error::Invalid(format!("{name} says there is not enough credit (or the key's own limit is reached). ({words})")),
        403 => Error::Invalid(format!("{name} does not let this key do that: {words}")),
        404 => Error::Invalid(format!("{name} does not know that model, or this key cannot use it. ({words})")),
        413 => Error::Invalid(format!("The request is too large for {name}. Start a new session, or make this one shorter. ({words})")),
        429 => Error::Invalid(format!("{name} says too many requests for now{}. ({words})", retry_after.map(|s| format!(": try again in {s} seconds")).unwrap_or_default())),
        500..=599 => Error::Net(format!("{name} had a problem of its own ({status}). Try again in a minute. ({words})")),
        _ => Error::Other(format!("{name} refused: {words}")),
    };
    Failed::Other(e)
}

/// Sends a request, and turns a refusal into words.
async fn send(provider: &str, request: reqwest::RequestBuilder) -> std::result::Result<reqwest::Response, Failed> {
    let name = ALL.iter().find(|d| d.id == provider).map_or(provider, |d| d.name);
    let res = request.send().await.map_err(|e| Failed::Other(Error::Net(format!("{name} could not be reached: {}", crate::runtime::root_cause(&e)))))?;
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let retry_after = res.headers().get("retry-after").and_then(|v| v.to_str().ok()).map(str::to_string);
    let said: Value = res.json().await.unwrap_or(Value::Null);
    Err(refused(provider, status, &said, retry_after.as_deref()))
}

#[cfg(test)]
mod tests;
