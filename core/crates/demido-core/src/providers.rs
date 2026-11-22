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
//! The person may have **several accounts at one provider** (DECISIONS U32–U35): several keys at an API, several
//! Claude Code sign-ins, each with a name of the person's own. One is the default; a session may name another.
//! What each request cost is counted against the account it was paid from.
//!
//! Not built yet: a provider's own spending figures for Anthropic and Google (they have no way to ask with an
//! ordinary key), a limit of its own for each account, and OpenCode signed in to anything.

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
        note: "Runs Claude Code, Anthropic's own program, with its own tools and its own loop: Demido is the window onto it. Demido keeps its own copy, downloaded from Anthropic when you ask and run with a profile of its own, so nothing is installed on your system and none of your own settings come with it. Each of your Claude accounts signs in once, on Anthropic's page, and keeps its login in a profile of its own. It uses your plan's limits (its five-hour and weekly ones); Demido shows what it reports and does not count it against the limit you set here. Demido never sees or keeps the login. Not made or endorsed by Anthropic.",
    },
    Def {
        id: "opencode",
        name: "OpenCode",
        program: true,
        base: "",
        keys_at: "https://opencode.ai/docs/",
        note: "Runs OpenCode, with its own tools and its own loop: Demido is the window onto it. Demido keeps its own copy, downloaded from OpenCode's package when you ask and run with folders of its own, so nothing is installed on your system. Its free models need no account; several of them use what is sent to improve them, and each says so here. Used only for your own messages: OpenCode's terms do not allow its free models to be run with nobody there.",
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
    /// For a program: whether it is here, its version, how its default account is signed in.
    pub program: Option<crate::programs::Found>,
    /// For a program: Demido's own copy, when it keeps one.
    pub copy: Option<crate::programs::copies::Kept>,
    /// For a program: the download of Demido's copy under way.
    pub downloading: Option<crate::programs::copies::Downloading>,
    pub keys_at: String,
    pub note: String,
    /// The person's accounts here, the oldest first: keys for an API, sign-ins for Claude Code.
    pub accounts: Vec<ProviderAccount>,
    /// Whether accounts can be added here. OpenCode has one profile, and none.
    pub takes_accounts: bool,
    /// The models the person chose here.
    pub chosen: Vec<RemoteModel>,
    /// What Demido counted this month for this provider, in dollars.
    pub spent_usd: f64,
    /// Whether all of that is the provider's own figure.
    pub spent_exact: bool,
}

/// One of the person's accounts at a provider: a key for an API, a sign-in of its own for Claude Code, in a
/// profile of its own (DECISIONS U32–U34).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProviderAccount {
    /// Demido's name for it, which does not change: it names the account's key in the vault
    /// (`<provider>.<id>`) and its Claude Code profile.
    pub id: String,
    pub provider: String,
    /// The person's own name for it: "Personal", "Work".
    pub label: String,
    /// Sessions use this one unless they name another.
    pub is_default: bool,
    /// Its key, when it has one. Never its value.
    pub key: Option<KeyFacts>,
    /// For Claude Code: how this account is signed in, as Claude Code says.
    pub signed_in: Option<String>,
    /// For Claude Code: a sign-in under way.
    pub signing: Option<SignIn>,
    /// For Claude Code: the command that signs this account in from a terminal.
    pub sign_in_command: Option<String>,
    /// For Claude Code: its plan's limit, as it last said it while running.
    pub plan: Option<PlanLimit>,
    /// What Demido counted this month for it, in dollars.
    pub spent_usd: f64,
    pub created_at: i64,
}

/// An account as it is written down.
#[derive(Clone, Debug)]
pub(crate) struct AccountRow {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub is_default: bool,
    pub created_at: i64,
}

/// The profile OpenCode runs in: it has one, and no accounts.
const OPENCODE_PROFILE: &str = "default";

/// The longest name an account may have.
const LABEL_AT_MOST: usize = 40;

/// The name an account's key is kept under in the vault.
fn key_name(provider: &str, account: &str) -> String {
    format!("{provider}.{account}")
}

/// A provider Demido knows, by its id.
fn known(provider: &str) -> Result<&'static Def> {
    ALL.iter()
        .find(|d| d.id == provider)
        .ok_or_else(|| Error::NotFound(format!("Demido knows no provider called \"{provider}\". It knows: {}.", ALL.iter().map(|d| d.id).collect::<Vec<_>>().join(", "))))
}

/// Why there is no account to use, for a person.
fn no_account(def: &Def) -> String {
    if def.program {
        format!("{} has no account in Demido yet. Sign in on the Providers screen: Anthropic's own page opens, and Demido never sees the login.", def.name)
    } else {
        format!("There is no {} key yet. Add yours on the Providers screen (you get one at {}).", def.name, def.keys_at)
    }
}

/// The person's name for an account, checked: one line, at most 40 characters, not the name of another of
/// the provider's accounts (`not`: the one being renamed). Nothing given: "Account 1", "Account 2".
fn label_for(all: &[AccountRow], given: Option<&str>, not: Option<&str>) -> Result<String> {
    let given = given.map(|l| l.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty());
    let taken = |label: &str| all.iter().any(|a| Some(a.id.as_str()) != not && a.label.to_lowercase() == label.to_lowercase());
    let label = match given {
        Some(label) => label,
        None => (1..).map(|n| format!("Account {n}")).find(|l| !taken(l)).expect("there is always a number not taken"),
    };
    if label.chars().count() > LABEL_AT_MOST {
        return Err(Error::Invalid(format!("An account's name is at most {LABEL_AT_MOST} characters.")));
    }
    if taken(&label) {
        return Err(Error::Invalid(format!("There is an account called \"{label}\" here already: give this one another name.")));
    }
    Ok(label)
}

/// A sign-in to Claude Code under way: Claude Code's own (`claude auth login`), on Anthropic's page.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SignIn {
    /// `started`, `done` or `failed`.
    pub state: String,
    /// The address of Anthropic's sign-in page, as Claude Code printed it, for when no browser opened.
    pub url: Option<String>,
    /// What Claude Code said, when it failed.
    pub said: Option<String>,
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

/// What a request is for, so its cost is counted against the right session, answer and account.
#[derive(Clone, Copy, Debug, Default)]
pub struct For<'a> {
    pub session: Option<&'a str>,
    pub message: Option<&'a str>,
    /// The account the session names. `None`: the provider's default.
    pub account: Option<&'a str>,
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
    /// Each account's plan limit, as its program last said it (`<provider>.<account>`).
    plans: Mutex<HashMap<String, PlanLimit>>,
    /// Demido's own copies of the programs.
    copies: Arc<crate::programs::copies::Copies>,
    /// Sign-ins under way, by account (`<provider>.<account>`).
    signing: Mutex<HashMap<String, SignIn>>,
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
        let copies = Arc::new(crate::programs::copies::Copies::new(&std::env::temp_dir().join("demido-copies"), events.clone()));
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
            copies,
            signing: Mutex::default(),
        }
    }

    /// The same, keeping Demido's copies of the programs under `home`, asking OpenCode for its models in a
    /// folder there, and starting programs with `spawner`.
    pub fn with_programs(mut self, home: &std::path::Path, spawner: Arc<dyn crate::programs::Spawner>) -> Self {
        self.listing_in = home.join("programs").join("listing");
        self.copies = Arc::new(crate::programs::copies::Copies::new(home, self.events.clone()));
        self.spawner = spawner;
        self.adopt_old();
        self
    }

    /// What was kept before there could be several accounts becomes each provider's first account: a key
    /// kept under the provider's own name, and the one profile Claude Code and OpenCode had (`profile`).
    fn adopt_old(&self) {
        for def in ALL.iter().filter(|d| !d.program) {
            if self.vault.facts(def.id).is_none() {
                continue;
            }
            let moved = (|| -> Result<()> {
                let value = self.vault.get(def.id)?.unwrap_or_default();
                self.add_account(def.id, None, Some(&value))?;
                self.vault.remove(def.id)?;
                Ok(())
            })();
            match moved {
                Ok(()) => tracing::info!("The {} key kept before accounts is now an account of its own.", def.name),
                Err(e) => tracing::warn!("The {} key kept before accounts could not be made an account: {e}", def.name),
            }
        }
        for def in ALL.iter().filter(|d| d.program) {
            let old = self.copies.old_profile(def.id);
            if !old.is_dir() {
                continue;
            }
            let into = if def.id == "claude-code" {
                match self.add_account(def.id, None, None) {
                    Ok(account) => account.id,
                    Err(e) => {
                        tracing::warn!("Claude Code's profile kept before accounts could not be made an account: {e}");
                        continue;
                    }
                }
            } else {
                OPENCODE_PROFILE.to_string()
            };
            let new = self.copies.profile(def.id, &into);
            if new.exists() {
                continue;
            }
            let moved = new.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::rename(&old, &new));
            if let Err(e) = moved {
                tracing::warn!("{}'s profile could not be moved to {}: {e}", def.name, new.display());
            }
        }
    }

    /// The same, with these copies. For tests.
    #[cfg(test)]
    pub(crate) fn with_copies(mut self, copies: Arc<crate::programs::copies::Copies>) -> Self {
        self.copies = copies;
        self
    }

    pub fn copies(&self) -> &Arc<crate::programs::copies::Copies> {
        &self.copies
    }

    /// What Demido's copy of a program is run with: the account's own profile (OpenCode has one), and no
    /// self-update. Claude Code is always run with an account: without one, it would make a profile called
    /// `default` that belongs to none.
    pub fn program_env(&self, provider: &str, account: Option<&str>) -> Vec<(&'static str, String)> {
        self.copies.env(provider, account.unwrap_or(OPENCODE_PROFILE))
    }

    /// Downloads Demido's copy of a program, in the background. Told as it goes.
    pub fn start_download(self: &Arc<Self>, provider: &str) -> Result<()> {
        if !ALL.iter().any(|d| d.id == provider && d.program) {
            return Err(Error::NotFound(format!("\"{provider}\" is not a program Demido runs.")));
        }
        if self.copies.downloading(provider).is_some() {
            return Err(Error::Busy("It is being downloaded already.".into()));
        }
        let (me, provider) = (self.clone(), provider.to_string());
        tokio::spawn(async move {
            let _ = me.copies.download(&provider).await;
            me.forget_look(&provider);
        });
        Ok(())
    }

    /// Forgets what a program said about itself, so it is asked again.
    fn forget_look(&self, provider: &str) {
        self.found.lock().unwrap_or_else(|e| e.into_inner()).retain(|key, _| !key.starts_with(&format!("{provider}:")));
    }

    /// Deletes Demido's copy of a program. Its profiles, with their settings and logins, stay.
    pub fn remove_copy(&self, provider: &str) -> Result<crate::programs::copies::Kept> {
        let gone = self.copies.remove(provider)?;
        self.forget_look(provider);
        Ok(gone)
    }

    /// The command that signs an account of Claude Code in from a terminal: for when Anthropic's page shows a
    /// code to paste, which Demido does not carry.
    fn sign_in_command(&self, account: &str) -> Option<String> {
        let kept = self.copies.kept("claude-code")?;
        let profile = self.copies.profile("claude-code", account).display().to_string();
        Some(if cfg!(windows) { format!("$env:CLAUDE_CONFIG_DIR=\"{profile}\"; & \"{}\" auth login", kept.path) } else { format!("CLAUDE_CONFIG_DIR='{profile}' '{}' auth login", kept.path) })
    }

    /// Starts Claude Code's own sign-in for one of its accounts: it opens Anthropic's page in the browser, and
    /// Claude Code keeps the login in the account's profile. Demido never reads it, asks for it or passes it
    /// on; it only shows the page's address, for when no browser opened.
    pub fn sign_in(self: &Arc<Self>, provider: &str, account: &str) -> Result<()> {
        if provider != "claude-code" {
            return Err(Error::Invalid("Only Claude Code is signed in from here. OpenCode's free models need no account.".into()));
        }
        let def = known(provider)?;
        self.account_row(def, account)?;
        let program = self.program(provider)?;
        let env = self.program_env(provider, Some(account));
        let signing_key = key_name(provider, account);
        let mut command = tokio::process::Command::new(&program);
        command.args(["auth", "login"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        for (name, value) in &env {
            command.env(name, value);
        }
        let mut child = command.spawn().map_err(|e| Error::Invalid(format!("Claude Code's sign-in could not be started: {e}")))?;
        self.signing.lock().unwrap_or_else(|e| e.into_inner()).insert(signing_key.clone(), SignIn { state: "started".into(), url: None, said: None });
        self.events.tell("program.signin", Some(provider), json!({ "account": account, "state": "started" }));
        let (me, account) = (self.clone(), account.to_string());
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut said = String::new();
            let mut lines = tokio::io::BufReader::new(child.stdout.take().expect("piped")).lines();
            let mut errors = tokio::io::BufReader::new(child.stderr.take().expect("piped")).lines();
            // Ten minutes for the person to sign in on Anthropic's page.
            let ended = tokio::time::timeout(std::time::Duration::from_secs(600), async {
                loop {
                    let line = tokio::select! {
                        l = lines.next_line() => l,
                        l = errors.next_line() => l,
                    };
                    let Ok(Some(line)) = line else { break };
                    let line = crate::programs::plain(&line);
                    if let Some(url) = line.split_whitespace().find(|w| w.starts_with("https://")) {
                        let mut signing = me.signing.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(s) = signing.get_mut(&signing_key)
                            && s.url.is_none()
                        {
                            s.url = Some(url.to_string());
                            me.events.tell("program.signin", Some("claude-code"), json!({ "account": account, "state": "started", "url": url }));
                        }
                    }
                    said.push_str(&line);
                    said.push('\n');
                }
                child.wait().await
            })
            .await;
            let ok = matches!(ended, Ok(Ok(status)) if status.success());
            let tail: String = said.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
            me.signing.lock().unwrap_or_else(|e| e.into_inner()).insert(
                signing_key,
                SignIn {
                    state: if ok { "done" } else { "failed" }.into(),
                    url: None,
                    said: (!ok).then(|| if tail.trim().is_empty() { "Claude Code ended the sign-in without saying why.".into() } else { tail }),
                },
            );
            me.forget_look("claude-code");
            me.events.tell("program.signin", Some("claude-code"), json!({ "account": account, "state": if ok { "done" } else { "failed" } }));
        });
        Ok(())
    }

    /// Signs an account of Claude Code out, as Claude Code does it (`claude auth logout`), before its profile
    /// is deleted: on macOS the login is in the Keychain, not in the profile. What it says is not kept.
    async fn sign_out(&self, account: &str) {
        let Ok(program) = self.program("claude-code") else { return };
        let mut command = tokio::process::Command::new(&program);
        command.args(["auth", "logout"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
        for (name, value) in self.program_env("claude-code", Some(account)) {
            command.env(name, value);
        }
        if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(20), command.status()).await
            && !status.success()
        {
            tracing::info!("Claude Code's sign-out ended with {status}; the profile is deleted all the same.");
        }
    }

    pub fn spawner(&self) -> &Arc<dyn crate::programs::Spawner> {
        &self.spawner
    }

    /// Where Demido's copy of a program is. Only Demido's own copy is run, never one installed on the system
    /// (the owner's direction, DECISIONS U26): everything stays in Demido's folder.
    fn program_path(&self, provider: &str) -> Option<std::path::PathBuf> {
        self.copies.kept(provider).map(|k| std::path::PathBuf::from(k.path))
    }

    /// Why there is no program to run, for a person.
    fn missing(&self, provider: &str) -> String {
        let name = if provider == "claude-code" { "Claude Code" } else { "OpenCode" };
        format!("Demido has no copy of {name} yet. Download it on the Providers screen: it comes from its makers and stays in Demido's own folder; nothing is installed on your system.")
    }

    /// Where a program is, or why it cannot be used.
    pub fn program(&self, provider: &str) -> Result<std::path::PathBuf> {
        self.program_path(provider).ok_or_else(|| Error::Invalid(self.missing(provider)))
    }

    /// What a program says about itself, with an account's profile, looked at again after a minute. Claude
    /// Code with no account is not run: there is no profile to run it in, and it is never run in the
    /// person's own.
    pub async fn look_at(&self, provider: &str, account: Option<&str>) -> crate::programs::Found {
        let key = format!("{provider}:{}", account.unwrap_or_default());
        let kept = self.found.lock().unwrap_or_else(|e| e.into_inner()).get(&key).filter(|(at, _)| at.elapsed() < PROGRAM_LOOKED_AT_FOR).map(|(_, f)| f.clone());
        if let Some(found) = kept {
            return found;
        }
        let found = match (provider, account) {
            // No profile is made for it, and none of the person's own is read.
            ("claude-code", None) => match self.copies.kept(provider) {
                Some(kept) => crate::programs::Found {
                    provider: provider.into(),
                    path: Some(kept.path),
                    version: Some(kept.version),
                    signed_in: None,
                    problem: Some("Claude Code has no account in Demido yet. Sign in with the button here: it opens Anthropic's own page. Demido never sees or keeps the login.".into()),
                },
                None => crate::programs::Found { provider: provider.into(), path: None, version: None, signed_in: None, problem: Some(self.missing(provider)) },
            },
            _ => crate::programs::look(provider, self.program_path(provider), &self.program_env(provider, account), &self.missing(provider)).await,
        };
        self.found.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), found.clone()));
        found
    }

    /// An account's plan limit, as its program said it while it ran.
    pub fn note_plan(&self, provider: &str, account: Option<&str>, status: &str, resets_at: Option<i64>, used: Option<f64>) {
        let limit = PlanLimit { status: status.into(), resets_at: resets_at.map(|s| if s < 100_000_000_000 { s * 1000 } else { s }), used, said_at: now_ms() };
        self.plans.lock().unwrap_or_else(|e| e.into_inner()).insert(key_name(provider, account.unwrap_or(OPENCODE_PROFILE)), limit);
        self.events.tell("provider.plan", Some(provider), json!({ "account": account, "status": status, "resets_at": resets_at, "used": used }));
    }

    /// Writes down what a request to a program cost, as the program said. It is shown, and does not count
    /// toward the month's limit.
    pub fn record(&self, provider: &str, model: &str, asking: For<'_>, tokens: prices::Tokens, cost: f64) -> Result<()> {
        self.db.with(|c| {
            c.execute(
                "INSERT INTO usage (at, provider, model, session_id, message_id, input, cached, cache_written, output, cost_usd, exact, account) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
                rusqlite::params![now_ms(), provider, model, asking.session, asking.message, tokens.input as i64, tokens.cached as i64, tokens.cache_written as i64, tokens.output as i64, cost, asking.account],
            )
        })?;
        self.events.tell("provider.used", Some(provider), json!({ "provider": provider, "account": asking.account, "cost_usd": cost, "exact": false, "session": asking.session }));
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

    fn key(&self, def: &Def, account: &AccountRow) -> Result<String> {
        self.vault
            .get(&key_name(def.id, &account.id))?
            .ok_or_else(|| Error::Invalid(format!("The {} account \"{}\" has no key any more. Add it again on the Providers screen.", def.name, account.label)))
    }

    // ───────── accounts ─────────

    /// The accounts at a provider, the oldest first; at all providers when `provider` is `None`.
    pub(crate) fn account_rows(&self, provider: Option<&str>) -> Result<Vec<AccountRow>> {
        self.db.with(|c| {
            c.prepare("SELECT id, provider, label, is_default, created_at FROM accounts WHERE ?1 IS NULL OR provider = ?1 ORDER BY created_at, rowid")?
                .query_map([provider], |r| Ok(AccountRow { id: r.get(0)?, provider: r.get(1)?, label: r.get(2)?, is_default: r.get::<_, i64>(3)? == 1, created_at: r.get(4)? }))?
                .collect()
        })
    }

    fn account_row(&self, def: &Def, account: &str) -> Result<AccountRow> {
        self.account_rows(Some(def.id))?.into_iter().find(|a| a.id == account).ok_or_else(|| Error::NotFound(format!("{} has no account \"{account}\" in Demido.", def.name)))
    }

    /// The account a request goes through: the one named, when it is one of this provider's, or else the
    /// provider's default.
    pub(crate) fn account_for(&self, def: &Def, named: Option<&str>) -> Result<AccountRow> {
        let all = self.account_rows(Some(def.id))?;
        if let Some(found) = named.and_then(|n| all.iter().find(|a| a.id == n)) {
            return Ok(found.clone());
        }
        all.iter().find(|a| a.is_default).or(all.first()).cloned().ok_or_else(|| Error::Invalid(no_account(def)))
    }

    /// An account as the person sees it, without what its program says (`view` adds that).
    fn account_view(&self, row: &AccountRow, spent_usd: f64) -> ProviderAccount {
        let claude_code = row.provider == "claude-code";
        ProviderAccount {
            id: row.id.clone(),
            provider: row.provider.clone(),
            label: row.label.clone(),
            is_default: row.is_default,
            key: if claude_code { None } else { self.vault.facts(&key_name(&row.provider, &row.id)) },
            signed_in: None,
            signing: self.signing.lock().unwrap_or_else(|e| e.into_inner()).get(&key_name(&row.provider, &row.id)).cloned(),
            sign_in_command: claude_code.then(|| self.sign_in_command(&row.id)).flatten(),
            plan: self.plans.lock().unwrap_or_else(|e| e.into_inner()).get(&key_name(&row.provider, &row.id)).cloned(),
            spent_usd,
            created_at: row.created_at,
        }
    }

    /// Adds an account at a provider: an API's with its key; Claude Code's to be signed in afterwards, on
    /// Anthropic's page, in a profile of its own. The first one is the default. `label` is the person's own
    /// name for it. The key goes to the keyring, which can take a moment: not for an async thread.
    pub fn add_account(&self, provider: &str, label: Option<&str>, key: Option<&str>) -> Result<ProviderAccount> {
        let def = known(provider)?;
        if def.program && def.id != "claude-code" {
            return Err(Error::Invalid("OpenCode has one profile in Demido and no accounts: its free models need none. Signing it in to other providers is not built yet.".into()));
        }
        let key = key.map(str::trim).filter(|k| !k.is_empty());
        match (def.program, key) {
            (false, None) => return Err(Error::Invalid(format!("An account at {} is its key: paste it (you get one at {}).", def.name, def.keys_at))),
            (true, Some(_)) => return Err(Error::Invalid("Claude Code is not given a key here: each account signs in on Anthropic's own page.".into())),
            _ => {}
        }
        let all = self.account_rows(Some(def.id))?;
        let label = label_for(&all, label, None)?;
        let id = loop {
            let id: String = uuid::Uuid::new_v4().simple().to_string().chars().take(8).collect();
            if !all.iter().any(|a| a.id == id) {
                break id;
            }
        };
        if let Some(key) = key {
            self.vault.put(&key_name(def.id, &id), key)?;
        }
        let row = AccountRow { id, provider: def.id.into(), label, is_default: all.is_empty(), created_at: now_ms() };
        self.db.with(|c| {
            c.execute(
                "INSERT INTO accounts (id, provider, label, is_default, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![row.id, row.provider, row.label, row.is_default as i64, row.created_at],
            )
        })?;
        self.events.append("provider.account.added", Some(def.id), json!({ "account": row.id, "label": row.label, "default": row.is_default }))?;
        Ok(self.account_view(&row, 0.0))
    }

    /// Renames an account, or makes it the one sessions use unless they name another.
    pub fn change_account(&self, provider: &str, account: &str, label: Option<&str>, default: bool) -> Result<ProviderAccount> {
        let def = known(provider)?;
        let all = self.account_rows(Some(def.id))?;
        let Some(mut row) = all.iter().find(|a| a.id == account).cloned() else {
            return Err(Error::NotFound(format!("{} has no account \"{account}\" in Demido.", def.name)));
        };
        if let Some(label) = label {
            if label.trim().is_empty() {
                return Err(Error::Invalid("An account needs a name.".into()));
            }
            row.label = label_for(&all, Some(label), Some(account))?;
            self.db.with(|c| c.execute("UPDATE accounts SET label = ?2 WHERE id = ?1", rusqlite::params![account, row.label]))?;
        }
        if default {
            self.db.with(|c| c.execute("UPDATE accounts SET is_default = (id = ?2) WHERE provider = ?1", [def.id, account]))?;
            row.is_default = true;
        }
        self.events.append("provider.account.changed", Some(def.id), json!({ "account": account, "label": row.label, "default": row.is_default }))?;
        Ok(self.account_view(&row, self.spent_by_account()?.get(account).copied().unwrap_or(0.0)))
    }

    /// Gives an account of an API another key, in place of the one it had. Not for an async thread either.
    pub fn set_account_key(&self, provider: &str, account: &str, key: &str) -> Result<ProviderAccount> {
        let def = known(provider)?;
        if def.program {
            return Err(Error::Invalid(format!("{} is not given a key here.", def.name)));
        }
        let row = self.account_row(def, account)?;
        self.vault.put(&key_name(def.id, account), key)?;
        self.events.append("provider.account.key", Some(def.id), json!({ "account": account }))?;
        Ok(self.account_view(&row, self.spent_by_account()?.get(account).copied().unwrap_or(0.0)))
    }

    /// Deletes an account: its key, or Claude Code's profile for it with its login and history (signed out
    /// first). Sessions that named it use the default from then on, and the oldest account left becomes the
    /// default if this one was. What it cost stays counted.
    pub async fn remove_account(self: &Arc<Self>, provider: &str, account: &str) -> Result<ProviderAccount> {
        let def = known(provider)?;
        let row = self.account_row(def, account)?;
        let gone = self.account_view(&row, self.spent_by_account()?.get(account).copied().unwrap_or(0.0));
        if def.id == "claude-code" {
            self.sign_out(account).await;
            self.copies.remove_profile(def.id, account)?;
        } else {
            let (me, name) = (self.clone(), key_name(def.id, account));
            match tokio::task::spawn_blocking(move || me.vault.remove(&name)).await.map_err(|e| Error::Other(e.to_string()))? {
                Ok(_) | Err(Error::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.db.with(|c| {
            c.execute("DELETE FROM accounts WHERE id = ?1", [account])?;
            c.execute("UPDATE sessions SET account = NULL WHERE account = ?1", [account])?;
            c.execute(
                "UPDATE accounts SET is_default = 1 WHERE id = (SELECT id FROM accounts WHERE provider = ?1 ORDER BY created_at, rowid LIMIT 1)
                 AND NOT EXISTS (SELECT 1 FROM accounts WHERE provider = ?1 AND is_default = 1)",
                [def.id],
            )
        })?;
        self.signing.lock().unwrap_or_else(|e| e.into_inner()).remove(&key_name(def.id, account));
        self.plans.lock().unwrap_or_else(|e| e.into_inner()).remove(&key_name(def.id, account));
        self.forget_look(def.id);
        self.events.append("provider.account.removed", Some(def.id), json!({ "account": account, "label": row.label }))?;
        Ok(gone)
    }

    /// The accounts at a provider (at all when `None`), the oldest first, without what a program says about
    /// them. For the command line, which does not run programs to list keys.
    pub fn accounts(&self, provider: Option<&str>) -> Result<Vec<ProviderAccount>> {
        let spent = self.spent_by_account()?;
        Ok(self.account_rows(provider)?.iter().map(|r| self.account_view(r, spent.get(&r.id).copied().unwrap_or(0.0))).collect())
    }

    /// What Demido counted this month for each account, in dollars.
    fn spent_by_account(&self) -> Result<HashMap<String, f64>> {
        let (since, _) = this_month();
        let rows: Vec<(String, f64)> = self
            .db
            .with(|c| c.prepare("SELECT account, sum(cost_usd) FROM usage WHERE at >= ?1 AND account IS NOT NULL GROUP BY account")?.query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())?;
        Ok(rows.into_iter().map(|(account, spent)| (account, spent + 0.0)).collect())
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
                let started = self.spawner.spawn(&program, &["acp".to_string()], &self.listing_in, &self.program_env("opencode", None))?;
                let list: Vec<RemoteModel> = crate::programs::opencode_models(started, &self.listing_in).await?.iter().map(|(value, name)| opencode_model(value, name)).collect();
                self.listed.lock().unwrap_or_else(|e| e.into_inner()).insert(provider.to_string(), (Instant::now(), list.clone()));
                list
            }
            None => {
                // OpenRouter lists its models to anyone; the others want a key: the default account's.
                let key = match (def.id, self.account_for(def, None)) {
                    ("openrouter", Err(_)) => String::new(),
                    (_, account) => self.key(def, &account?)?,
                };
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

    /// The providers, each with its accounts (never a key's value), its chosen models and what it cost this
    /// month.
    pub async fn view(&self) -> Result<Vec<ProviderView>> {
        let spending = self.spending()?;
        let chosen = self.chosen()?;
        let rows = self.account_rows(None)?;
        let spent = self.spent_by_account()?;
        let mut accounts: Vec<ProviderAccount> = rows.iter().map(|r| self.account_view(r, spent.get(&r.id).copied().unwrap_or(0.0))).collect();
        for a in accounts.iter_mut().filter(|a| a.provider == "claude-code") {
            a.signed_in = self.look_at("claude-code", Some(&a.id)).await.signed_in;
        }
        // What each program says about itself: Claude Code with its default account.
        let mut programs = HashMap::new();
        for d in ALL.iter().filter(|d| d.program) {
            let default = rows.iter().find(|r| r.provider == d.id && r.is_default).map(|r| r.id.clone());
            programs.insert(d.id, self.look_at(d.id, default.as_deref()).await);
        }
        Ok(ALL
            .iter()
            .map(|d| {
                let mine: Vec<&Spent> = spending.by_model.iter().filter(|s| s.provider == d.id).collect();
                ProviderView {
                    id: d.id.into(),
                    name: d.name.into(),
                    kind: if d.program { "program" } else { "api" }.into(),
                    program: programs.get(d.id).cloned(),
                    copy: if d.program { self.copies.kept(d.id) } else { None },
                    downloading: if d.program { self.copies.downloading(d.id) } else { None },
                    keys_at: d.keys_at.into(),
                    note: d.note.into(),
                    accounts: accounts.iter().filter(|a| a.provider == d.id).cloned().collect(),
                    takes_accounts: !d.program || d.id == "claude-code",
                    chosen: chosen.iter().filter(|m| m.provider == d.id).cloned().collect(),
                    spent_usd: mine.iter().map(|s| s.cost_usd).sum::<f64>() + 0.0,
                    spent_exact: mine.iter().all(|s| s.exact),
                }
            })
            .collect())
    }

    /// What a provider itself says about an account (the default when none is named). Only OpenRouter can be
    /// asked this with an ordinary key; for the others it says where to look.
    pub async fn account(&self, provider: &str, account: Option<&str>) -> Result<Account> {
        let def = known(provider)?;
        match def.id {
            "openrouter" => {
                let row = match account {
                    Some(named) => self.account_row(def, named)?,
                    None => self.account_for(def, None)?,
                };
                openai::account(&self.http, self.base(def.id), &self.key(def, &row)?).await
            }
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

    /// Whether a model elsewhere may be asked now: chosen, an account to ask it through (the one named, or
    /// the default) with its key kept, and the month's limit not reached. With the account it goes through;
    /// OpenCode has none.
    pub(crate) fn ready(&self, model: &str, account: Option<&str>) -> Result<(RemoteModel, Option<AccountRow>)> {
        let Some((def, _)) = split(model) else {
            return Err(Error::NotFound(format!("\"{model}\" is not a model of a provider.")));
        };
        let Some(facts) = self.chosen_one(model)? else {
            return Err(Error::Invalid(format!("{model} is not chosen. Choose it on the Providers screen first: nothing is sent to a model you did not choose.")));
        };
        // A program has no key in Demido (its login is its own) and is held by its own plan, not by the limit.
        if def.program {
            self.program(def.id)?;
            let account = if def.id == "claude-code" { Some(self.account_for(def, account)?) } else { None };
            return Ok((facts, account));
        }
        let account = self.account_for(def, account)?;
        if self.vault.facts(&key_name(def.id, &account.id)).is_none() {
            return Err(Error::Invalid(format!("The {} account \"{}\" has no key any more. Add it again on the Providers screen.", def.name, account.label)));
        }
        self.within_limit(&facts, 0)?;
        Ok((facts, Some(account)))
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
        let (facts, account) = self.ready(model, asking.account)?;
        let account = account.expect("an API's request always has an account");
        let (def, name) = split(model).expect("ready checked it");
        let key = self.key(def, &account)?;
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
                "INSERT INTO usage (at, provider, model, session_id, message_id, input, cached, cache_written, output, cost_usd, exact, account) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![now_ms(), def.id, name, asking.session, asking.message, t.input as i64, t.cached as i64, t.cache_written as i64, t.output as i64, cost, exact as i64, account.id],
            )
        })?;
        self.events.tell("provider.used", Some(model), json!({ "provider": def.id, "account": account.id, "cost_usd": cost, "exact": exact, "session": asking.session }));
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
