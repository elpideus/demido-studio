//! Running a turn: what the person wrote goes to the model, and its answer comes back piece by piece.
//!
//! The core runs the turn, not the window (DECISIONS L2). `say` writes down what was said, starts the answer
//! and returns at once. The answer is then written in the background: told piece by piece as news that is
//! not kept (`message.delta`), and stored whole when it is complete (`message.done`). A window that closes
//! mid-answer loses nothing, and one that opens late reads the stored message.
//!
//! The prompt always begins with the same instructions and then has the conversation, oldest first, only
//! ever added to (L4). The runtime keeps what it has read of a prompt, so an unchanged beginning is not read
//! again: each turn costs the new words, not the whole conversation.
//!
//! Thinking (brief 4.9) is a choice of the session: off, on, or one of the model's own levels, for the models
//! the runtime says can think. It is off unless the person turns it on (L6): measured, it costs seconds and
//! on easy requests buys nothing. A thought that runs long can be ended from outside ("Answer now"), and
//! the model then goes on to its answer.
//!
//! A conversation that nears what the model can hold is made shorter (brief 4.15): the model writes a short
//! account of the older part, and from then on it is given that account in place of those messages. The
//! messages stay in the session for the person to read. It happens by itself before a turn that would not
//! fit comfortably, and by hand whenever the person asks. How long a conversation is, is not asked of
//! anyone: the runtime said how many tokens the last turn was, and that is counted on from.
//!
//! Before an answer is written, the model is asked whether it needs a tool, and work can be split into
//! tasks that Demido walks one by one. That part is in `turn.rs`, with what was measured to shape it.
//!
//! A session's model may run elsewhere (`provider/model`, see `providers`): the turn is the same, and only
//! the asking goes to the provider. A model elsewhere is not loaded; it is checked (chosen, key kept, the
//! month's limit not reached) instead. A model run by a program on this computer (Claude Code, OpenCode) is
//! given the message and watched: its turn is in `program.rs`.
//!
//! Not built yet: settings per model.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::engine::{Answer, Engine, Piece};
use crate::events::EventLog;
use crate::permissions::{self, Level, Said};
use crate::sessions::{Change, Message, Session, Sessions, Summary, title_from};
use crate::settings::Settings;
use crate::tools;
use crate::{Error, Result};

mod program;
mod turn;

/// What every model is told first, in every session. The answer style of the brief (4.14): the answer
/// first, short, like a manual. Kept short itself, and it never changes within a session.
pub const INSTRUCTIONS: &str = "Answer first. Then numbered steps, only if steps are needed. One idea per sentence. Exact values with units. No filler, no praise. Do not repeat the question.";

/// What the model is told when it writes the account that will stand for the older part of a conversation.
///
/// The order is the point. The conversation comes first, fenced off as material; what to do with it comes
/// after it. Measured on the two test models (`core/dev/account-trial.py`, research note 13): told what to do
/// first and then shown the conversation, they went on with the conversation and kept 1 and 3 of 15 facts.
/// Told last, they kept 13 and 14.
pub const ACCOUNT_ROLE: &str = "You write notes on conversations. You never continue or answer them.";
pub const ACCOUNT_INSTRUCTIONS: &str = "Above is a conversation between a person and a model. Do not continue it and do not answer it. Write notes on it for someone who takes over: who the person is, what they want, every fact and number they gave, what was decided, and what is still open. Short lines.";

/// An account may be this long, in tokens.
const ACCOUNT_AT_MOST: u64 = 400;
/// When a conversation is made shorter, the newest messages stay word for word: about this share of the
/// size at which it is made shorter.
const KEPT_WORD_FOR_WORD: f64 = 0.4;

/// How much of what the model can hold a session is using.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ContextUse {
    /// About how many tokens the next prompt will be. From the runtime's own count of the last turn, plus
    /// an estimate for what was said since.
    pub used: u64,
    /// How many tokens the model is loaded with room for.
    pub limit: u64,
    /// At this many, the older part of the conversation is replaced by an account.
    pub compact_at: u64,
    /// Whether that happens by itself.
    pub auto: bool,
}

/// About how many tokens a text is, when nothing has counted it. On the generous side.
fn roughly(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(3)
}

/// Pieces of an answer are gathered and passed on about this often: often enough to read as writing,
/// not so often that the window does nothing else.
const TELL_EVERY: Duration = Duration::from_millis(33);

/// What `say` gives back at once: what the person said, and the answer that is now being written.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Turn {
    pub said: Message,
    /// Empty and `answering` at this moment. Its text arrives as `message.delta` news and is stored at `message.done`.
    pub answer: Message,
}

pub struct Chat {
    sessions: Arc<Sessions>,
    engine: Arc<Engine>,
    events: Arc<EventLog>,
    settings: Arc<Settings>,
    /// What time and what day it is. A test gives its own.
    clock: tools::Clock,
    /// The sessions that are being answered right now.
    turns: Mutex<HashMap<String, Running>>,
    /// The uses of a tool that are waiting for the person's yes, by session. One at a time in a session.
    asking: Mutex<HashMap<String, Waiting>>,
    /// The tools the person said need not ask again, by session. Kept while Demido runs, and no longer.
    granted: Mutex<HashMap<String, HashSet<String>>>,
    /// The sandbox this computer has for running programs, found once.
    /// Replaced when the person sets one up (Windows).
    sandbox: std::sync::RwLock<crate::sandbox::Sandbox>,
    /// Where folders are saved before a program runs in them.
    kept_in: std::path::PathBuf,
    /// The one way a model reads from the internet.
    web: Arc<crate::web::Web>,
    /// The browser a model drives (DECISIONS Z), when Demido has one.
    browser: Option<Arc<crate::browser::Browser>>,
    /// The person's mail (DECISIONS Y), when Demido has it.
    mail: Option<Arc<crate::email::Mail>>,
    /// The tools the person installed (DECISIONS C6).
    packages: Option<Arc<crate::packages::Packages>>,
    /// Where the whole of what programs printed is kept, a folder for each session.
    outputs: std::path::PathBuf,
    /// Models elsewhere, and the keys for them. `None` in a test that has none.
    providers: Option<Arc<crate::providers::Providers>>,
    /// Where this server answers, for a program's approval tool. Set when the server starts.
    address: std::sync::OnceLock<String>,
    /// The turns programs are running now, by the token their approval tool carries.
    programs_live: Mutex<HashMap<String, program::Live>>,
    /// Where a program works for a session without a folder: a folder of its own for each session.
    programs_in: std::path::PathBuf,
}

/// A conversation with a model elsewhere is given at most this many tokens of it, however much the model
/// can hold: each turn sends the whole of it again, and is paid for.
const REMOTE_MOST_IN_VIEW: u64 = 200_000;

/// Of the outputs a session's programs printed, the newest this many are kept.
const OUTPUTS_KEPT: usize = 50;

/// The saved folders of a session that are kept: the newest ones. An older run of a program can then no
/// longer be undone.
const SAVED_FOLDERS_KEPT: usize = 10;

/// A use of a tool that is waiting, and the way to answer it.
struct Waiting {
    request: permissions::Request,
    answer: Option<tokio::sync::oneshot::Sender<Said>>,
}

/// An answer that is being written.
#[derive(Clone)]
struct Running {
    /// Set to stop the answer.
    stop: Arc<AtomicBool>,
    /// The runtime's own name for the answer, once it has said it. A thought is ended by it.
    named: Arc<Mutex<Option<String>>>,
    /// The answer being written, once it is stored: what a provider's cost is counted against.
    answer: Option<String>,
}

impl Chat {
    pub fn new(sessions: Arc<Sessions>, engine: Arc<Engine>, events: Arc<EventLog>, settings: Arc<Settings>) -> Self {
        Self {
            sessions,
            engine,
            events,
            settings,
            clock: tools::here,
            turns: Mutex::default(),
            asking: Mutex::default(),
            granted: Mutex::default(),
            // Looked for where the server starts, once (`with_sandbox`): looking starts a program.
            sandbox: std::sync::RwLock::new(crate::sandbox::Sandbox::not_looked_for()),
            kept_in: std::env::temp_dir().join("demido-saved-folders"),
            // Keeps nothing on disk until the server says where (`with_web`).
            web: Arc::new(crate::web::Web::new(None)),
            browser: None,
            mail: None,
            packages: None,
            outputs: std::env::temp_dir().join("demido-outputs"),
            providers: None,
            address: std::sync::OnceLock::new(),
            programs_live: Mutex::default(),
            programs_in: std::env::temp_dir().join("demido-programs"),
        }
    }

    /// The same, giving a program a folder under `place` for each session that has none.
    pub fn keeping_programs_in(mut self, place: std::path::PathBuf) -> Self {
        self.programs_in = place;
        self
    }

    /// The same, able to ask models elsewhere, with the keys their vault keeps.
    pub fn with_providers(mut self, providers: Arc<crate::providers::Providers>) -> Self {
        self.providers = Some(providers);
        self
    }

    fn providers(&self) -> Result<&Arc<crate::providers::Providers>> {
        self.providers.as_ref().ok_or_else(|| Error::Invalid("This Demido cannot ask models elsewhere.".into()))
    }

    /// The text with every key Demido keeps taken out.
    pub(crate) fn hide(&self, text: &str) -> String {
        match &self.providers {
            Some(p) => p.vault().hide_in(text),
            None => text.to_string(),
        }
    }

    /// Whether the text holds a key Demido keeps.
    pub(crate) fn holds_a_key(&self, text: &str) -> bool {
        self.providers.as_ref().is_some_and(|p| p.vault().holds_one(text))
    }

    /// Makes a session's model ready: loads one on this computer, checks one elsewhere and its account.
    async fn ready(&self, session: &Session) -> Result<()> {
        if crate::providers::split(&session.model).is_some() {
            self.providers()?.ready(&session.model, session.account.as_deref()).map(|_| ())
        } else {
            self.engine.load(&session.model).await.map(|_| ())
        }
    }

    /// Asks a session's model, wherever it runs. A provider's cost is counted against the answer being written.
    pub(crate) async fn ask(&self, session: &Session, body: Value, stop: &AtomicBool, heard: &mut (dyn FnMut(Piece) + Send)) -> Result<Answer> {
        if crate::providers::split(&session.model).is_none() {
            return self.engine.answer(&session.model, body, stop, heard).await;
        }
        let answer = self.turns().get(&session.id).and_then(|r| r.answer.clone());
        let asking = crate::providers::For { session: Some(&session.id), message: answer.as_deref(), account: session.account.as_deref() };
        self.providers()?.answer(&session.model, body, stop, heard, asking).await
    }

    /// What thinking can be set to for a model, besides off: as its runtime or its provider says. `None`
    /// until a model on this computer has been loaded once.
    pub async fn thinking_choices(&self, model: &str) -> Result<Option<Vec<String>>> {
        if crate::providers::split(model).is_some() {
            return Ok(self.providers()?.chosen_one(model)?.map(|m| m.thinking));
        }
        Ok(self.engine.list().await?.models.into_iter().find(|m| m.id == model).and_then(|m| m.thinking))
    }

    /// The same, keeping the whole of what programs printed under `place`.
    pub fn keeping_outputs_in(mut self, place: std::path::PathBuf) -> Self {
        self.outputs = place;
        self
    }

    /// Keeps the whole of what a program printed, and gives back its id. `None` when it printed nothing, or
    /// when it could not be written down. Only the newest `OUTPUTS_KEPT` of a session are kept.
    pub(crate) fn keep_output(&self, session_id: &str, whole: &str) -> Option<String> {
        if whole.trim().is_empty() {
            return None;
        }
        let (id, dir) = (uuid::Uuid::now_v7().to_string(), self.outputs.join(session_id));
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(dir.join(format!("{id}.txt")), whole).ok()?;
        // The ids are in the order they were made, so the oldest sort first.
        let mut kept: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        kept.sort();
        for old in kept.iter().take(kept.len().saturating_sub(OUTPUTS_KEPT)) {
            let _ = std::fs::remove_file(old);
        }
        Some(id)
    }

    /// The whole of what a program printed in a session, by the id its step holds.
    pub fn output(&self, session_id: &str, id: &str) -> Result<String> {
        // An id is letters, digits and dashes: nothing that could name another file.
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || !session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(crate::Error::NotFound(format!("There is no output \"{id}\".")));
        }
        std::fs::read_to_string(self.outputs.join(session_id).join(format!("{id}.txt")))
            .map_err(|_| crate::Error::NotFound(format!("That output is no longer kept: only the {OUTPUTS_KEPT} newest of a session are.")))
    }

    /// What a change of a session changed, as a diff.
    pub fn change_diff(&self, session_id: &str, change_id: &str) -> Result<String> {
        self.sessions.diff_of(session_id, change_id)?.ok_or_else(|| crate::Error::NotFound("This change was made before Demido kept what changes changed.".into()))
    }

    /// The same, reading from the internet through `web`: the one that keeps what it read, or, in a test,
    /// one that asks a stand-in.
    pub fn with_web(mut self, web: Arc<crate::web::Web>) -> Self {
        self.web = web;
        self
    }

    /// Where sessions and their messages are kept.
    pub fn sessions(&self) -> &Arc<Sessions> {
        &self.sessions
    }

    /// The client that reads from the internet.
    pub fn web(&self) -> &Arc<crate::web::Web> {
        &self.web
    }

    /// The same, with the browser a model drives.
    pub fn with_browser(mut self, browser: Arc<crate::browser::Browser>) -> Self {
        self.browser = Some(browser);
        self
    }

    pub(crate) fn browser(&self) -> Option<&Arc<crate::browser::Browser>> {
        self.browser.as_ref()
    }

    /// The same, with the tools the person installed.
    pub fn with_packages(mut self, packages: Arc<crate::packages::Packages>) -> Self {
        self.packages = Some(packages);
        self
    }

    pub(crate) fn packages(&self) -> Option<&Arc<crate::packages::Packages>> {
        self.packages.as_ref()
    }

    /// A tool by its name: one of Demido's own, or one the person installed.
    pub(crate) fn named(&self, name: &str) -> Option<&'static tools::Tool> {
        tools::named(name).or_else(|| self.packages.as_ref().and_then(|p| p.tool(name)))
    }

    /// Every tool a session is offered: Demido's own as the settings and the session allow, and the installed
    /// ones whose reach the session has (a folder for those that ask for one, the internet for those that read it).
    pub(crate) fn offer(&self, session: &Session, may_plan: bool) -> Vec<&'static tools::Tool> {
        let has_folder = session.folder.is_some();
        let mut all = tools::offered_all(has_folder, may_plan, self.internet(), self.artifacts(has_folder), self.browsing(), self.mail_account().is_some(), !self.others_in_folder(session).is_empty());
        if let Some(p) = &self.packages {
            all.extend(p.tools().into_iter().filter(|t| match t.reach {
                tools::Reach::Reads | tools::Reach::Changes => has_folder,
                tools::Reach::Network => self.internet(),
                _ => true,
            }));
        }
        all
    }

    /// The same, with the person's mail.
    pub fn with_mail(mut self, mail: Arc<crate::email::Mail>) -> Self {
        self.mail = Some(mail);
        self
    }

    /// The other sessions that have the same folder as this one (DECISIONS S5): those a model may message, and
    /// those told when this one changes a file there. Schedules' sessions are among them.
    pub(crate) fn others_in_folder(&self, session: &Session) -> Vec<Session> {
        let Some(folder) = session.folder.as_deref() else { return Vec::new() };
        self.sessions.list().unwrap_or_default().into_iter().filter(|s| s.id != session.id && s.folder.as_deref() == Some(folder)).collect()
    }

    /// Puts a message into another session, from `from` (a session's title, or Demido's own note), as text from
    /// outside: shown in the window as such, and given to its model before its next request, fenced.
    pub fn tell_session(&self, to: &str, from: &str, text: &str) -> Result<Message> {
        let text: String = tools::clean(text).chars().take(2_000).collect();
        let said = self.sessions.add(to, "session", &format!("{from}\n{text}"), "done", None)?;
        self.events.tell("session.message", Some(to), json!({ "from": from, "message": said.id }));
        let _ = self.events.append("session.told", Some(to), json!({ "from": from, "characters": text.chars().count() }));
        Ok(said)
    }

    /// After a turn that changed files in its folder, tells every other session with the same folder which
    /// (DECISIONS S5): by code, so that a session does not overwrite what another just did without knowing.
    pub(crate) fn tell_others_of_changes(&self, session: &Session, steps: &[tools::Step]) {
        let mut changed: Vec<String> = Vec::new();
        for step in steps.iter().filter(|s| s.state == "done" && s.change.is_some()) {
            let what = step.args.get("path").and_then(|p| p.as_str()).map(str::to_string).unwrap_or_else(|| "files, by a command".into());
            if !changed.contains(&what) {
                changed.push(what);
            }
        }
        if changed.is_empty() {
            return;
        }
        for other in self.others_in_folder(session) {
            let _ = self.tell_session(
                &other.id,
                &format!("Demido's note, about the session \"{}\"", session.title),
                &format!("It changed {} in the folder just now. Read a file again before you change it.", changed.join(", ")),
            );
        }
    }

    /// The account models use, when there is one and the person lets models use their mail: the first.
    pub(crate) fn mail_account(&self) -> Option<(Arc<crate::email::Mail>, crate::email::Account)> {
        if !self.settings.value::<bool>("email.models").unwrap_or(true) {
            return None;
        }
        let mail = self.mail.clone()?;
        let account = mail.accounts().ok()?.into_iter().next()?;
        Some((mail, account))
    }

    /// Whether models may use Demido's browser: there is one, downloaded, the internet is on, and so is the setting.
    pub(crate) fn browsing(&self) -> bool {
        self.internet() && self.settings.value::<bool>("web.browser").unwrap_or(true) && self.browser.as_ref().is_some_and(|b| b.ready())
    }

    /// Whether models may read from the internet at all: the person's setting.
    pub(crate) fn internet(&self) -> bool {
        self.settings.value::<bool>("web.use").unwrap_or(true)
    }

    /// Whether models may give the person artifacts in a session with or without a folder (DECISIONS Q14).
    /// By default only without one: asked to add a file to the folder, a real model gave an artifact instead.
    pub(crate) fn artifacts(&self, has_folder: bool) -> bool {
        match self.settings.value::<String>("tools.artifacts").as_deref() {
            Ok("always") => true,
            Ok("never") => false,
            _ => !has_folder,
        }
    }

    /// The same, saving folders under `place` before a program runs in them.
    pub fn keeping_in(mut self, place: std::path::PathBuf) -> Self {
        self.kept_in = place;
        self
    }

    /// The same, with this sandbox for the programs a model runs: the one found on this computer, or, in a
    /// test, one said to be so.
    pub fn with_sandbox(self, sandbox: crate::sandbox::Sandbox) -> Self {
        self.set_sandbox(sandbox);
        self
    }

    /// The sandbox programs are run in on this computer, or why there is none.
    pub fn sandbox(&self) -> crate::sandbox::Sandbox {
        self.sandbox.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Another sandbox from now on: the one the person has just set up.
    pub fn set_sandbox(&self, sandbox: crate::sandbox::Sandbox) {
        *self.sandbox.write().unwrap_or_else(|e| e.into_inner()) = sandbox;
    }

    /// Takes `folder` back from the sandbox, unless a session still has it (`sandbox::give_back`).
    fn give_back(&self, folder: Option<&str>) {
        let Some(folder) = folder else { return };
        let still = self.sessions.list().map(|all| all.iter().any(|s| s.folder.as_deref() == Some(folder))).unwrap_or(true);
        if !still {
            self.sandbox().give_back(std::path::Path::new(folder));
        }
    }

    /// The same, telling the time and the day from `clock`: for tests.
    pub fn with_clock(mut self, clock: tools::Clock) -> Self {
        self.clock = clock;
        self
    }

    /// What the model is loaded with room for, and the size at which a conversation is made shorter. A model
    /// elsewhere holds what its provider says, up to `REMOTE_MOST_IN_VIEW`: past that, every turn costs a lot.
    fn limits(&self, model: &str) -> (u64, u64) {
        let remote = crate::providers::split(model).and(self.providers.as_ref()).and_then(|p| p.chosen_one(model).ok().flatten());
        let limit = match remote {
            Some(m) => m.context_length.unwrap_or(REMOTE_MOST_IN_VIEW).min(REMOTE_MOST_IN_VIEW),
            // A model on this computer: what it is loaded with room for, its own or the one for every model.
            None => self.engine.context_of(model),
        };
        // Three quarters, so there is always room for the answer. A size chosen by hand never goes above that.
        let most = limit * 3 / 4;
        let chosen = self.settings.value::<String>("context.compactAt").ok().and_then(|v| v.parse::<u64>().ok());
        (limit, chosen.map_or(most, |c| c.min(most)))
    }

    /// The messages the model is still given word for word: the ones after the last account.
    fn in_view(&self, session_id: &str) -> Result<(Option<Summary>, Vec<Message>)> {
        let account = self.sessions.summaries(session_id)?.pop();
        let after = account.as_ref().map_or(0, |a| a.upto_seq);
        Ok((account, self.sessions.messages(session_id)?.into_iter().filter(|m| m.seq > after).collect()))
    }

    /// About how many tokens the next prompt of a session will be.
    fn tokens_in_view(account: &Option<Summary>, messages: &[Message]) -> u64 {
        // The last answer the runtime timed says exactly how long the conversation was up to and including it,
        // as long as nothing was made shorter since. From there on it is an estimate.
        let counted = messages.iter().rposition(|m| m.timings.is_some());
        match counted {
            Some(at) => {
                let t = messages[at].timings.as_ref().expect("just found");
                t.reused + t.read + t.written + messages[at + 1..].iter().map(|m| roughly(&m.content) + 4).sum::<u64>()
            }
            None => roughly(INSTRUCTIONS) + account.as_ref().map_or(0, |a| roughly(&a.content)) + messages.iter().map(|m| roughly(&m.content) + 4).sum::<u64>(),
        }
    }

    /// How much of what the model can hold this session is using.
    pub fn context_use(&self, session_id: &str) -> Result<ContextUse> {
        let (account, messages) = self.in_view(session_id)?;
        let (limit, compact_at) = self.limits(&self.sessions.get(session_id)?.model);
        // An account made after the last timed answer makes that count too high: estimate from scratch then.
        let stale = account.as_ref().is_some_and(|a| messages.iter().all(|m| m.timings.is_none() || m.created_at < a.created_at));
        let used = if stale {
            Self::tokens_in_view(
                &account,
                &messages
                    .iter()
                    .cloned()
                    .map(|mut m| {
                        m.timings = None;
                        m
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            Self::tokens_in_view(&account, &messages)
        };
        Ok(ContextUse { used, limit, compact_at, auto: self.settings.value::<bool>("context.autoCompact").unwrap_or(true) })
    }

    /// Makes a session shorter for the model: writes an account of its older part and gives the model that
    /// account from now on, in place of those messages. The messages stay. `why` is `asked` or `auto`.
    pub async fn compact(&self, session_id: &str, why: &str) -> Result<Summary> {
        if self.answering(session_id) {
            return Err(Error::Busy("This session is writing an answer. Wait for it, or stop it, and then make it shorter.".into()));
        }
        let session = self.sessions.get(session_id)?;
        self.account_of(&session, why, None).await
    }

    /// Writes the account. `still_to_answer` is the answer about to be written, when this happens inside a turn.
    async fn account_of(&self, session: &Session, why: &str, still_to_answer: Option<&str>) -> Result<Summary> {
        let (before, messages) = self.in_view(&session.id)?;
        let said: Vec<&Message> = messages.iter().filter(|m| Some(m.id.as_str()) != still_to_answer && !(m.role == "assistant" && m.content.trim().is_empty())).collect();
        let (_, compact_at) = self.limits(&session.model);
        let tokens_before = self.context_use(&session.id)?.used;
        // The newest messages stay word for word, and never fewer than the last thing the person said and, if
        // it was answered, its answer. When it happens by itself, as many more as fit in a share of the
        // size. When the person asks, only those two: they asked for it to be as short as it gets.
        let room = if why == "asked" { 0 } else { (compact_at as f64 * KEPT_WORD_FOR_WORD) as u64 };
        let (mut keep, mut kept_tokens) = (0, 0);
        for m in said.iter().rev() {
            let size = roughly(&m.content) + 4;
            if keep >= 2 && kept_tokens + size > room {
                break;
            }
            keep += 1;
            kept_tokens += size;
        }
        let older = &said[..said.len().saturating_sub(keep.max(1))];
        let Some(last_covered) = older.last() else {
            return Err(Error::Invalid("There is nothing to make shorter yet: the conversation is only a few messages long.".into()));
        };
        self.events.tell("session.compacting", Some(&session.id), json!({ "why": why, "messages": older.len() }));
        let mut transcript = String::new();
        if let Some(earlier) = &before {
            transcript.push_str(&format!("Notes on what came before:\n{}\n\n", earlier.content.trim()));
        }
        for m in older {
            transcript.push_str(&format!(
                "{}: {}\n\n",
                if m.role == "user" {
                    "Person"
                } else if m.role == "session" {
                    "Another session"
                } else {
                    "Model"
                },
                m.content.trim()
            ));
        }
        // The material first, fenced. What to do with it, last.
        let asked = format!("<conversation>\n{}\n</conversation>\n\n{ACCOUNT_INSTRUCTIONS}", transcript.trim_end());
        self.ready(session).await?;
        let body = json!({
            "model": session.model,
            "messages": [{ "role": "system", "content": ACCOUNT_ROLE }, { "role": "user", "content": asked }],
            "stream": true,
            "max_tokens": ACCOUNT_AT_MOST,
            "temperature": 0.2,
            "chat_template_kwargs": { "enable_thinking": false },
            "reasoning_effort": "none",
        });
        let written = self.ask(session, body, &AtomicBool::new(false), &mut |_| {}).await?;
        let account = written.content.trim();
        if account.is_empty() {
            return Err(Error::Other("The model wrote nothing when asked for an account of the conversation, so nothing was made shorter.".into()));
        }
        let summary = self.sessions.summarise(&session.id, last_covered.seq, account, why, tokens_before)?;
        self.events.append(
            "session.compacted",
            Some(&session.id),
            json!({ "summary": summary.id, "why": why, "upto_seq": summary.upto_seq, "messages": older.len(), "tokens_before": tokens_before }),
        )?;
        Ok(summary)
    }

    fn turns(&self) -> std::sync::MutexGuard<'_, HashMap<String, Running>> {
        self.turns.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether this session is being answered right now.
    pub fn answering(&self, session_id: &str) -> bool {
        self.turns().contains_key(session_id)
    }

    /// Whether any session is being answered right now: a schedule's run waits for that to end rather than
    /// taking the model from it (DECISIONS J7).
    pub fn anyone_answering(&self) -> bool {
        !self.turns().is_empty()
    }

    /// Whether a model on this computer is loaded now. A model elsewhere is never loaded here.
    pub async fn loaded(&self, model: &str) -> bool {
        crate::providers::split(model).is_none() && self.engine.list().await.is_ok_and(|list| list.models.iter().any(|m| m.id == model && m.state == "loaded"))
    }

    /// Unloads a model that a schedule's run loaded, once the run is over (J7): unless a session is being
    /// answered by then, which may be using it.
    pub async fn unload_after_a_run(&self, model: &str) {
        if !self.anyone_answering() && self.loaded(model).await {
            let _ = self.engine.unload(model, "a schedule's run that loaded it ended").await;
        }
    }

    /// The shop's own offers on a page, for a monitor (J5): read by code, never by a model. The address is the
    /// person's own, so this computer's network is read when they turned that on, as for a page they named.
    pub async fn offers_at(&self, url: &str, stop: &AtomicBool) -> std::result::Result<(Vec<crate::web::page::Offer>, String), String> {
        let may_be_local = self.settings.value::<bool>("web.local").unwrap_or(false);
        self.web.offers(url, may_be_local, stop).await
    }

    /// Starts a session. With no model named, it takes one that is loaded; with none loaded, the first on disk.
    /// A model elsewhere is taken only when it is named, and only once it is chosen.
    pub async fn start(&self, title: Option<&str>, model: Option<&str>) -> Result<Session> {
        if let Some(named) = model.filter(|m| crate::providers::split(m).is_some()) {
            if self.providers()?.chosen_one(named)?.is_none() {
                return Err(Error::Invalid(format!("{named} is not chosen. Choose it on the Providers screen first: nothing is sent to a model you did not choose.")));
            }
            return self.started(title, named);
        }
        let on_disk = self.engine.list().await?;
        let model = match model {
            Some(named) => on_disk
                .models
                .iter()
                .find(|m| m.id == named)
                .ok_or_else(|| Error::NotFound(format!("There is no model called \"{named}\" in {}. `demido models` lists what is there.", on_disk.folder)))?,
            None => on_disk.models.iter().find(|m| m.state == "loaded").or(on_disk.models.first()).ok_or_else(|| {
                Error::Invalid(format!(
                    "There is no model on this computer to talk to yet. Download one on the Models screen (or put a GGUF file in {}), or name a model you chose on the Providers screen.",
                    on_disk.folder
                ))
            })?,
        };
        let id = model.id.clone();
        self.started(title, &id)
    }

    fn started(&self, title: Option<&str>, model: &str) -> Result<Session> {
        let session = self.sessions.create(title, model)?;
        // A new session starts at the level the person chose for new sessions.
        let level = self.settings.value::<String>("permissions.level").ok().filter(|l| Level::from(l).is_some());
        let session = match level {
            Some(level) if level != session.permission => self.sessions.set_permission(&session.id, &level)?,
            _ => session,
        };
        self.events.append("session.created", Some(&session.id), json!({ "title": session.title, "model": session.model }))?;
        Ok(session)
    }

    /// Deletes a session and everything said in it. An answer being written in it is stopped first.
    pub fn delete(&self, session_id: &str) -> Result<Session> {
        self.stop(session_id);
        let gone = self.sessions.delete(session_id)?;
        self.give_back(gone.folder.as_deref());
        // The folders saved for undoing its programs go with it, and what its programs printed.
        let _ = std::fs::remove_dir_all(self.kept_in.join(session_id));
        let _ = std::fs::remove_dir_all(self.outputs.join(session_id));
        let _ = std::fs::remove_dir_all(self.programs_in.join(session_id));
        self.events.append("session.deleted", Some(&gone.id), json!({ "title": gone.title }))?;
        Ok(gone)
    }

    /// Writes down what the person said and starts the answer. Returns at once; the answer follows as news.
    pub fn say(self: &Arc<Self>, session_id: &str, text: &str) -> Result<Turn> {
        self.say_with(session_id, text, false)
    }

    /// The same. With `plan`, the model is first asked for the steps of the request, and Demido then has it
    /// do them one at a time.
    pub fn say_with(self: &Arc<Self>, session_id: &str, text: &str, plan: bool) -> Result<Turn> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("Nothing was written.".into()));
        }
        let session = self.sessions.get(session_id)?;
        let running = Running { stop: Arc::new(AtomicBool::new(false)), named: Arc::default(), answer: None };
        {
            let mut turns = self.turns();
            if turns.contains_key(session_id) {
                return Err(Error::Busy("This session is still writing its last answer. Wait for it, or stop it.".into()));
            }
            turns.insert(session_id.to_string(), running.clone());
        }
        let written = (|| -> Result<Turn> {
            let said = self.sessions.add(session_id, "user", text, "done", None)?;
            if session.messages == 0 && session.title == "New session" {
                self.sessions.change(session_id, Some(&title_from(text)), None, None)?;
            }
            let answer = self.sessions.add(session_id, "assistant", "", "answering", Some(&session.model))?;
            self.events.append("message.added", Some(session_id), json!({ "message": said.id, "answer": answer.id }))?;
            Ok(Turn { said, answer })
        })();
        let turn = match written {
            Ok(turn) => {
                if let Some(r) = self.turns().get_mut(session_id) {
                    r.answer = Some(turn.answer.id.clone());
                }
                turn
            }
            Err(e) => {
                self.turns().remove(session_id);
                return Err(e);
            }
        };
        let (me, answer_id) = (self.clone(), turn.answer.id.clone());
        tokio::spawn(async move { me.write_answer(session, answer_id, running, plan).await });
        Ok(turn)
    }

    /// What the model of a session is told first, word for word: what day it is, and how to answer. A model
    /// does not know the date, and does not know that it does not: asked how long until Christmas, one made
    /// a date up and counted from that. The day is the day the session started, so these words never
    /// change, and the runtime reads them once. The date comes first and the way to answer last: with the
    /// date last, one model began its answers with it.
    pub fn told_first(&self, session: &Session) -> String {
        format!("Today's date: {}.\n\n{INSTRUCTIONS}", (self.clock)(session.created_at).day)
    }

    /// What a model is shown before each answer when it may use tools, word for word. `None` when tools are
    /// off. A session with a folder is shown the tools that reach into it too.
    pub fn tools_told(&self, session: &Session) -> Option<String> {
        self.settings.value::<bool>("tools.use").unwrap_or(true).then(|| turn::told_about_tools(session.folder.is_some(), &self.offer(session, true)))
    }

    fn asking(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiting>> {
        self.asking.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn granted_in(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashSet<String>>> {
        self.granted.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn granted(&self, session_id: &str, tool: &str) -> bool {
        self.granted_in().get(session_id).is_some_and(|tools| tools.contains(tool))
    }

    /// The use of a tool that is waiting for the person's yes in a session, if one is.
    pub fn waiting_for(&self, session_id: &str) -> Option<permissions::Request> {
        self.asking().get(session_id).map(|w| w.request.clone())
    }

    /// The tools that need not ask again in a session, because the person said so.
    pub fn need_not_ask(&self, session_id: &str) -> Vec<String> {
        let mut tools: Vec<String> = self.granted_in().get(session_id).map(|t| t.iter().cloned().collect()).unwrap_or_default();
        tools.sort();
        tools
    }

    /// The person's answer to a use of a tool that is waiting.
    pub fn answer_request(&self, session_id: &str, request_id: &str, said: Said) -> Result<()> {
        let mut asking = self.asking();
        let waiting = asking
            .get_mut(session_id)
            .filter(|w| w.request.id == request_id)
            .ok_or_else(|| Error::NotFound("Nothing is waiting for that answer. It may have been answered already, or the answer was stopped.".into()))?;
        if let Some(answer) = waiting.answer.take() {
            let _ = answer.send(said);
        }
        Ok(())
    }

    /// Gives a session the folder its tools may work in, or takes it away (`None`). The folder must be
    /// there. Not while an answer is being written: the tools in view would change under it.
    pub fn set_folder(&self, session_id: &str, folder: Option<&str>) -> Result<Session> {
        if self.answering(session_id) {
            return Err(Error::Busy("This session is writing an answer. Wait for it, or stop it, and then change its folder.".into()));
        }
        let folder = match folder.map(str::trim).filter(|f| !f.is_empty()) {
            None => None,
            Some(named) => {
                let path = std::path::Path::new(named);
                if !path.is_absolute() {
                    return Err(Error::Invalid(format!("\"{named}\" is not a full path. Write it from the root, like /home/you/project.")));
                }
                let real = path.canonicalize().map_err(|_| Error::Invalid(format!("There is no folder \"{named}\" on this computer.")))?;
                if !real.is_dir() {
                    return Err(Error::Invalid(format!("\"{named}\" is a file, not a folder.")));
                }
                if real.parent().is_none() {
                    return Err(Error::Invalid("The whole computer cannot be a session's folder. Choose a folder inside it.".into()));
                }
                Some(real.to_string_lossy().into_owned())
            }
        };
        let before = self.sessions.get(session_id)?.folder;
        let session = self.sessions.set_folder(session_id, folder.as_deref())?;
        if before != session.folder {
            self.give_back(before.as_deref());
        }
        self.events.append("session.changed", Some(session_id), json!({ "folder": session.folder }))?;
        Ok(session)
    }

    /// Sets how much a session may do without asking. What the person had said need not ask again is
    /// forgotten: it was said under the old level.
    pub fn set_permission(&self, session_id: &str, level: &str) -> Result<Session> {
        if Level::from(level).is_none() {
            return Err(Error::Invalid(format!("A session can be: {}. \"{level}\" is not one of them.", permissions::LEVELS.join(", "))));
        }
        let session = self.sessions.set_permission(session_id, level)?;
        self.granted_in().remove(session_id);
        self.events.append("session.changed", Some(session_id), json!({ "permission": session.permission }))?;
        Ok(session)
    }

    /// Undoes a change a tool made to a file: puts back what the file held, or removes a file the change
    /// created. What is in the file now is lost; that is what undoing means.
    pub fn undo(&self, session_id: &str, change_id: &str) -> Result<Change> {
        if self.answering(session_id) {
            return Err(Error::Busy("This session is writing an answer. Wait for it, or stop it, and then undo.".into()));
        }
        let session = self.sessions.get(session_id)?;
        let (change, before) = self.sessions.change_with_before(change_id)?;
        if change.session_id != session_id {
            return Err(Error::NotFound(format!("There is no change \"{change_id}\" in this session.")));
        }
        if change.undone_at.is_some() {
            return Err(Error::Invalid(format!("The change to {} was undone already.", change.path)));
        }
        let Some(folder) = session.folder else {
            return Err(Error::Invalid("This session has no folder any more, so there is nowhere to put the file back.".into()));
        };
        if change.whole_folder {
            // Everything a program did: the folder goes back to how it was saved before the program ran.
            let Some(place) = self.sessions.checkpoint_of(change_id)? else {
                return Err(Error::Invalid("The folder as it was before that command is no longer kept.".into()));
            };
            crate::checkpoint::restore(std::path::Path::new(&folder), std::path::Path::new(&place)).map_err(Error::Invalid)?;
            let _ = std::fs::remove_dir_all(&place);
        } else {
            tools::files::put_back(std::path::Path::new(&folder), &tools::files::Changed { path: change.path.clone(), before }).map_err(Error::Invalid)?;
        }
        self.sessions.mark_undone(change_id)?;
        self.events.append("change.undone", Some(session_id), json!({ "change": change_id, "path": change.path }))?;
        self.sessions.change_with_before(change_id).map(|(change, _)| change)
    }

    /// Changes a session: its title, its model, whether the model thinks first. A model must be one that is
    /// on this computer; a way of thinking must be one the model has.
    pub async fn change(&self, session_id: &str, title: Option<&str>, model: Option<&str>, thinking: Option<&str>) -> Result<Session> {
        let before = self.sessions.get(session_id)?;
        if let Some(named) = model {
            if crate::providers::split(named).is_some() {
                if self.providers()?.chosen_one(named)?.is_none() {
                    return Err(Error::Invalid(format!("{named} is not chosen. Choose it on the Providers screen first: nothing is sent to a model you did not choose.")));
                }
            } else if !self.engine.list().await?.models.iter().any(|m| m.id == named) {
                return Err(Error::NotFound(format!("There is no model called \"{named}\" on this computer.")));
            }
        }
        if let Some(wanted) = thinking.filter(|t| *t != "off") {
            // What the session's model (the new one, if it is being changed) can be set to. Not known until
            // the model has been loaded once; then "on" is taken on trust and the runtime decides.
            let choices = self.thinking_choices(model.unwrap_or(&before.model)).await?;
            match choices {
                Some(can) if can.is_empty() => return Err(Error::Invalid(format!("{} does not think before it answers, so there is nothing to turn on.", model.unwrap_or(&before.model)))),
                Some(can) if !can.iter().any(|c| c == wanted) => return Err(Error::Invalid(format!("Thinking can be: off, {}. \"{wanted}\" is not one of them.", can.join(", ")))),
                None if wanted != "on" => return Err(Error::Invalid("Until this model has been loaded once, thinking can only be off or on.".into())),
                _ => {}
            }
        }
        let mut changed = self.sessions.change(session_id, title, model, thinking)?;
        // An account belongs to one provider: a model of another goes through that one's default.
        let provider_of = |model: &str| crate::providers::split(model).map(|(d, _)| d.id);
        if changed.account.is_some() && provider_of(&changed.model) != provider_of(&before.model) {
            changed = self.sessions.set_account(session_id, None)?;
        }
        self.events.append("session.changed", Some(session_id), json!({ "title": changed.title, "model": changed.model, "thinking": changed.thinking, "account": changed.account }))?;
        Ok(changed)
    }

    /// Sets the account a session goes through: one of its model's provider's, or (`None`) the default.
    pub fn set_account(&self, session_id: &str, account: Option<&str>) -> Result<Session> {
        let session = self.sessions.get(session_id)?;
        if let Some(account) = account {
            let Some((def, _)) = crate::providers::split(&session.model) else {
                return Err(Error::Invalid("This session's model is on this computer: it has no account.".into()));
            };
            if !self.providers()?.account_rows(Some(def.id))?.iter().any(|a| a.id == account) {
                return Err(Error::NotFound(format!("{} has no account \"{account}\" in Demido.", def.name)));
            }
        }
        let changed = self.sessions.set_account(session_id, account)?;
        self.events.append("session.changed", Some(session_id), json!({ "title": changed.title, "model": changed.model, "thinking": changed.thinking, "account": changed.account }))?;
        Ok(changed)
    }

    /// Ends the thought the model is in, so that it goes on to its answer. False when it was not thinking.
    pub async fn answer_now(&self, session_id: &str) -> Result<bool> {
        let named = self.turns().get(session_id).and_then(|r| r.named.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let Some(named) = named else { return Ok(false) };
        let session = self.sessions.get(session_id)?;
        // A provider's model cannot be told this from outside.
        if crate::providers::split(&session.model).is_some() {
            return Ok(false);
        }
        self.engine.end_thought(&session.model, &named).await
    }

    /// Stops the answer being written in a session. What was written so far is kept. False if none was.
    pub fn stop(&self, session_id: &str) -> bool {
        match self.turns().get(session_id) {
            Some(running) => {
                running.stop.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// What comes before the request of a turn: what the model is told first, then what was said, oldest
    /// first. And the request itself: the last thing the person said. Nothing that was in an earlier prompt
    /// is changed or left out later, so the runtime never reads it twice. The one exception is when the
    /// conversation is made shorter: then the older messages give way to an account of them, once, and the
    /// prompt is stable again from there.
    fn before(&self, session: &Session, answer_id: &str) -> Result<(Vec<Value>, String)> {
        let (account, in_view) = self.in_view(&session.id)?;
        let told = match &account {
            Some(account) => format!("{}\n\nEarlier in this conversation, in short:\n{}", self.told_first(session), tools::clean(&account.content)),
            None => self.told_first(session),
        };
        let mut messages = vec![json!({ "role": "system", "content": told })];
        // The day is said once, at the start. When the person speaks again on a later day, that is said too.
        // From what is stored, so it reads the same in every later prompt.
        let mut day = (self.clock)(session.created_at).date;
        let mut dated = |m: &Message| {
            let at = (self.clock)(m.created_at);
            // What the person wrote may be pasted from anywhere. It cannot carry a turn marker either.
            let said = tools::clean(&m.content);
            if at.date == day {
                return said;
            }
            day = at.date;
            format!("Today's date is now {}.\n\n{said}", at.day)
        };
        // What a past answer's tools were followed by names the request they were used for.
        let mut asked = String::new();
        // The answer about to be written is not part of its own question.
        let upto = in_view.iter().position(|m| m.id == answer_id).unwrap_or(in_view.len());
        let request_at = in_view[..upto].iter().rposition(|m| m.role == "user");
        // What other sessions said (S5) goes with the person's next message, fenced as text from outside: a model's
        // template has no turn of its own for it.
        let mut from_others = String::new();
        let mut before_request = String::new();
        for (i, m) in in_view[..upto].iter().enumerate() {
            if m.role == "session" {
                let (from, text) = m.content.split_once('\n').unwrap_or(("Another session", &m.content));
                from_others.push_str(&format!("<from another session: {from}>\n{}\n</from another session>\n\n", tools::clean(text)));
                continue;
            }
            if Some(i) == request_at {
                before_request = std::mem::take(&mut from_others);
            }
            // An answer that failed said nothing.
            if Some(i) == request_at || (m.role == "assistant" && m.content.trim().is_empty()) {
                continue;
            }
            if m.role == "assistant" {
                // The tools an answer used are part of what led to it: each call and its result, as they were.
                messages.extend(turn::after_request(&m.steps, &turn::End::Answer, &turn::Words::request(&asked)));
                messages.push(json!({ "role": m.role, "content": m.content }));
            } else {
                asked = dated(m);
                let with_others = format!("{}{asked}", std::mem::take(&mut from_others));
                messages.push(json!({ "role": m.role, "content": with_others }));
            }
        }
        Ok((messages, request_at.map(|i| format!("{before_request}{}", dated(&in_view[i]))).unwrap_or_default()))
    }

    /// The prompt for the answer itself: everything before, the request, and the tools used for it.
    fn prompt(&self, session: &Session, answer_id: &str) -> Result<Vec<Value>> {
        let (mut messages, request) = self.before(session, answer_id)?;
        let steps = self.sessions.message(answer_id)?.steps;
        messages.extend(turn::request_messages(&request, &steps, &turn::End::Answer, &turn::Words::request(&request)));
        Ok(messages)
    }

    async fn write_answer(self: Arc<Self>, session: Session, answer_id: String, running: Running, plan: bool) {
        // Gathered as they arrive, told every few tens of milliseconds.
        let mut waiting = (String::new(), String::new());
        let mut last_told = Instant::now();
        let tell = |waiting: &mut (String, String)| {
            if !waiting.0.is_empty() || !waiting.1.is_empty() {
                self.events.tell("message.delta", Some(&session.id), json!({ "message": answer_id, "text": waiting.0, "thought": waiting.1 }));
                waiting.0.clear();
                waiting.1.clear();
            }
        };
        let outcome: Result<Answer> = async {
            // The model is loaded if it is not: its loading is told by the engine's own news. One elsewhere is checked.
            self.ready(&session).await?;
            // A program on this computer runs its own loop: it is given the message, and watched.
            if crate::providers::split(&session.model).is_some_and(|(d, _)| d.program) {
                let mut heard = |piece: Piece| {
                    match piece {
                        Piece::Text(text) => waiting.0.push_str(text),
                        Piece::Thought(text) => waiting.1.push_str(text),
                        Piece::Started(_) => {}
                    }
                    if last_told.elapsed() >= TELL_EVERY {
                        last_told = Instant::now();
                        tell(&mut waiting);
                    }
                };
                return self.program_answer(&session, &answer_id, &running, &mut heard).await;
            }
            // A conversation that has grown past the set size is made shorter first, so that this answer has room.
            let room = self.context_use(&session.id)?;
            if room.auto && room.used > room.compact_at {
                match self.account_of(&session, "auto", Some(&answer_id)).await {
                    Ok(_) | Err(Error::Invalid(_)) => {}
                    // If the account cannot be written, the answer is still tried: the runtime says no if it truly does not fit.
                    Err(e) => tracing::warn!("A session could not be made shorter: {e}"),
                }
            }
            // What the model needs before it can answer: a tool, or the work split into tasks and done.
            if self.settings.value::<bool>("tools.use").unwrap_or(true) {
                self.work(&session, &answer_id, plan, &running.stop).await?;
            }
            if running.stop.load(Ordering::Relaxed) {
                return Ok(Answer { stopped: true, ..Answer::default() });
            }
            let thinks = session.thinking != "off";
            let mut body = json!({
                "model": session.model,
                "messages": self.prompt(&session, &answer_id)?,
                "stream": true,
                "stream_options": { "include_usage": true },
                // Said both ways: the first is how this family of templates is told, the second how the runtime is.
                "chat_template_kwargs": { "enable_thinking": thinks },
            });
            if !thinks {
                body["reasoning_effort"] = json!("none");
            } else {
                // So that a thought that runs long can be ended from outside.
                body["reasoning_control"] = json!(true);
                if session.thinking != "on" {
                    body["reasoning_effort"] = json!(session.thinking);
                }
            }
            // How a model on this computer picks its words: the person's settings for it, or its maker's,
            // for this way of answering (`engine::sampling`). A model elsewhere is left to its provider.
            if crate::providers::split(&session.model).is_none() {
                self.engine.sampling_for(&session.model, thinks).into_request(&mut body);
            }
            // The answer so far, looked at now and then: a model that goes round is stopped (`turn::going_round`).
            let (mut written, mut look_at, mut ends_at) = (String::new(), 400usize, None);
            // After a tool was used, a small model sometimes hands its answer back wrapped the way the tool's
            // result came to it: {"result": "..."}. And with a tool or without, one sometimes writes a request for
            // a tool where its answer should be, with a result it made up after it (Granite 3.3, asked "Ready?" in
            // voice mode). An answer that starts like either is held back until it is whole, and then unwrapped
            // (`turn::unwrapped`), tried again (T17) or shown as it is.
            let used_a_tool = !self.sessions.message(&answer_id)?.steps.is_empty();
            let held = |w: &str| {
                let w = w.trim_start();
                w.starts_with('{') || w.starts_with("<result>") || (!w.is_empty() && "<result>".starts_with(w))
            };
            // How much of `written` has been told: all of it, from the moment it is not held back.
            let mut told = 0;
            // Set before a second try at the answer, so that what was written at the first is let go.
            let again = AtomicBool::new(false);
            let mut heard = |piece: Piece| {
                if again.swap(false, Ordering::Relaxed) {
                    written.clear();
                    told = 0;
                }
                match piece {
                    Piece::Started(name) => *running.named.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string()),
                    Piece::Text(text) => {
                        written.push_str(text);
                        if !held(&written) {
                            waiting.0.push_str(&written[told..]);
                            told = written.len();
                        }
                        if ends_at.is_none() && written.len() >= look_at {
                            look_at = written.len() + 200;
                            ends_at = turn::going_round(&written);
                            if ends_at.is_some() {
                                running.stop.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Piece::Thought(text) => waiting.1.push_str(text),
                }
                if last_told.elapsed() >= TELL_EVERY {
                    last_told = Instant::now();
                    tell(&mut waiting);
                }
            };
            let mut answer = self.ask(&session, body.clone(), &running.stop, &mut heard).await?;
            // A request for a tool where the answer should be: the asking is over, so nothing more is run. The
            // model is told so once and tries again. (Tried on a real model: after a command that printed
            // nothing, it wrote the same request again as its answer. Granite, with no tool needed, asked for
            // one and wrote a made-up result under it: what it is shown again is the request alone.)
            if !answer.stopped
                && let Some(request) = turn::request_of(&answer.content)
            {
                let mut messages = body["messages"].as_array().cloned().unwrap_or_default();
                messages.push(json!({ "role": "assistant", "content": request }));
                messages
                    .push(json!({ "role": "user", "content": "That is a request for a tool, not an answer. No more tools can be used now. Answer the person in plain words, from what you have." }));
                body["messages"] = Value::Array(messages);
                again.store(true, Ordering::Relaxed);
                answer = self.ask(&session, body, &running.stop, &mut heard).await?;
            }
            if held(&answer.content) {
                if let Some(inside) = turn::unwrapped(&answer.content).filter(|_| used_a_tool) {
                    answer.content = inside;
                } else if turn::request_of(&answer.content).is_some() {
                    answer.not_an_answer = true;
                }
                // Held back while it was written; told now, whole.
                waiting.0 = answer.content.clone();
            }
            if let Some(end) = ends_at {
                // Kept up to the first of the repeats. The rest said nothing new.
                answer.content.truncate(end.min(answer.content.len()));
                answer.went_round = true;
            }
            Ok(answer)
        }
        .await;
        tell(&mut waiting);
        self.turns().remove(&session.id);
        // What the answer cost elsewhere, with everything asked for it (the tools it chose, its tasks).
        let cost = self.providers.as_ref().filter(|_| crate::providers::split(&session.model).is_some()).and_then(|p| p.cost_of(&answer_id).ok().flatten());
        let outcome = outcome.map(|mut answer| {
            if let Some((usd, exact)) = cost {
                (answer.timings.cost_usd, answer.timings.cost_exact) = (Some(usd), Some(exact));
            }
            answer
        });
        let ended = match outcome {
            Ok(answer) => {
                // A request for a tool where the answer should be is shown, and said to be no answer.
                let state = if answer.stopped {
                    "stopped"
                } else if answer.not_an_answer {
                    "failed"
                } else {
                    "done"
                };
                let why = if answer.went_round {
                    Some("The model began to repeat itself, so Demido stopped it here.")
                } else if answer.not_an_answer {
                    Some("The model wrote a request for a tool where its answer should be. Nothing more was run. Ask again, or try another model.")
                } else {
                    None
                };
                self.sessions.finish(&answer_id, &answer.content, Some(&answer.thought), state, why, Some(&answer.timings)).map(|_| {
                    (
                        if answer.stopped {
                            "message.stopped"
                        } else if answer.not_an_answer {
                            "message.failed"
                        } else {
                            "message.done"
                        },
                        json!({ "message": answer_id, "written": answer.timings.written, "per_second": answer.timings.per_second, "reused": answer.timings.reused, "read": answer.timings.read, "problem": why }),
                    )
                })
            }
            Err(why) => self.sessions.finish(&answer_id, "", None, "failed", Some(&why.to_string()), None).map(|_| ("message.failed", json!({ "message": answer_id, "problem": why.to_string() }))),
        };
        match ended {
            Ok((kind, data)) => {
                let _ = self.events.append(kind, Some(&session.id), data);
            }
            // The session was deleted while its answer was being written: there is nowhere to write it.
            Err(e) => tracing::debug!("An answer could not be stored: {e}"),
        }
        // The other sessions in this folder are told which files this turn changed (S5).
        if let Ok(done) = self.sessions.message(&answer_id) {
            self.tell_others_of_changes(&session, &done.steps);
        }
    }
}

#[cfg(test)]
mod tests;
