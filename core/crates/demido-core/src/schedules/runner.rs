//! Running schedules when they are due (DECISIONS J1 to J10).
//!
//! The server looks every few seconds (`LOOK_EVERY`) and runs what is due, one at a time. A run of a task
//! is the schedule's words said in its session, answered as any request is, at Autonomous (J2); a run of a
//! monitor reads the shop's own price, by code, and needs no model at all (J5). A run waits for a session's
//! answer to end rather than taking the model from it (J7). A run that was missed while the computer slept
//! or Demido was not running is run once when it can be, and said to be late; missed runs are never run
//! several times over (J9). What a run did is in the schedule's session, and the window is told (J6).

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::when::{Price, Watch, number};
use super::{New, Run, Schedule, Schedules, Watched, one_line};
use crate::chat::Chat;
use crate::events::EventLog;
use crate::sessions::Sessions;
use crate::{Error, Result, now_ms};

/// How often the server looks for what is due.
pub const LOOK_EVERY: Duration = Duration::from_secs(20);
/// A run that starts later than this after its time is said to be late.
pub const LATE_AFTER_MS: i64 = 2 * 60_000;
/// The longest a run of a task may take before it is stopped.
const AT_MOST: Duration = Duration::from_secs(30 * 60);
/// The longest a run waits for another session's answer to end.
const WAITS_AT_MOST: Duration = Duration::from_secs(30 * 60);

/// How a run ended.
struct Outcome {
    state: &'static str,
    message: Option<String>,
    note: String,
    watched: Option<Watched>,
}

impl Outcome {
    fn failed(note: impl Into<String>) -> Self {
        Self { state: "failed", message: None, note: note.into(), watched: None }
    }
    fn skipped(note: impl Into<String>) -> Self {
        Self { state: "skipped", message: None, note: note.into(), watched: None }
    }
}

pub struct Scheduler {
    schedules: Arc<Schedules>,
    sessions: Arc<Sessions>,
    chat: Arc<Chat>,
    events: Arc<EventLog>,
    /// The schedules running now: one is never run twice at once.
    running: Mutex<HashSet<String>>,
    at_most: Duration,
    waits_at_most: Duration,
    look: Duration,
}

impl Scheduler {
    pub fn new(schedules: Arc<Schedules>, sessions: Arc<Sessions>, chat: Arc<Chat>, events: Arc<EventLog>) -> Self {
        Self { schedules, sessions, chat, events, running: Mutex::default(), at_most: AT_MOST, waits_at_most: WAITS_AT_MOST, look: Duration::from_millis(500) }
    }

    /// Tests only: shorter waits.
    #[cfg(test)]
    pub(crate) fn quicker(mut self, at_most: Duration, waits_at_most: Duration) -> Self {
        (self.at_most, self.waits_at_most, self.look) = (at_most, waits_at_most, Duration::from_millis(10));
        self
    }

    pub fn schedules(&self) -> &Arc<Schedules> {
        &self.schedules
    }

    fn running(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a schedule is running now.
    pub fn is_running(&self, id: &str) -> bool {
        self.running().contains(id)
    }

    /// Makes a schedule and the session of its own that its runs are in (J1). Its session runs at
    /// Autonomous, and its level is never asked about (J2). A program such as Claude Code or OpenCode is not
    /// offered: their terms are for a person's own use, not for runs with nobody there (U23).
    pub async fn make(&self, new: &New) -> Result<Schedule> {
        make_with(&self.chat, &self.schedules, &self.events, new).await
    }

    /// Runs whatever is due by `now`, one at a time, the longest waiting first.
    pub async fn look(&self, now: i64) {
        let due = match self.schedules.due(now) {
            Ok(due) => due,
            Err(e) => return tracing::warn!("Could not read the schedules: {e}"),
        };
        for schedule in due {
            if let Err(e) = self.run(&schedule.id, false).await {
                tracing::warn!("A run of the schedule {} could not be made: {e}", schedule.id);
            }
        }
    }

    /// Runs a schedule now. `by_hand`: the person asked for it ("Run now"); its next time does not move.
    pub async fn run(&self, id: &str, by_hand: bool) -> Result<Run> {
        if !self.running().insert(id.to_string()) {
            return Err(Error::Busy("This schedule is running already.".into()));
        }
        let out = self.run_once(id, by_hand).await;
        self.running().remove(id);
        out
    }

    async fn run_once(&self, id: &str, by_hand: bool) -> Result<Run> {
        let schedule = self.schedules.get(id)?;
        let now = now_ms();
        let due = if by_hand { now } else { schedule.next_at.unwrap_or(now) };
        let late = !by_hand && now - due > LATE_AFTER_MS;
        let run = self.schedules.start_run(&schedule.id, due, late, by_hand)?;
        self.events.append("schedule.started", Some(&schedule.id), json!({ "run": run.id, "session": schedule.session_id, "late": late, "by_hand": by_hand }))?;
        // A model the run loads is unloaded when it ends (J7); one that was loaded already stays.
        let model = self.sessions.get(&schedule.session_id).map(|s| s.model).ok();
        let was_loaded = match &model {
            Some(model) => self.chat.loaded(model).await,
            None => true,
        };
        let outcome = match &schedule.watch {
            Some(watch) => self.watch(&schedule, watch).await,
            None => self.task(&schedule).await,
        };
        if let Some(model) = model.filter(|_| !was_loaded) {
            self.chat.unload_after_a_run(&model).await;
        }
        let after = self.schedules.end_run(&run.id, outcome.state, outcome.message.as_deref(), &outcome.note, outcome.watched.as_ref())?;
        let run = self.schedules.run(&run.id)?;
        // The window says what a run gave (J6); a monitor that spoke up and a run that failed most of all.
        self.events.append(
            "schedule.ran",
            Some(&schedule.id),
            json!({
                "run": run.id, "session": schedule.session_id, "what": schedule.what, "watch": schedule.watch_said,
                "state": run.state, "note": run.note, "late": late, "by_hand": by_hand, "message": run.message_id,
                "paused": after.state == "paused", "paused_why": after.paused_why,
            }),
        )?;
        Ok(run)
    }

    /// A task: the schedule's words, said in its session, and answered as any request is.
    async fn task(&self, schedule: &Schedule) -> Outcome {
        let session = match self.sessions.get(&schedule.session_id) {
            Ok(session) => session,
            Err(e) => return Outcome::failed(format!("Its session could not be opened: {e}")),
        };
        if let Err(e) = refuse_a_program(&session.model) {
            return Outcome::skipped(e.to_string());
        }
        // A paid provider: only up to the schedule's own limit for the month (J8). The limit for everything
        // is the provider's own, and holds as for any session.
        if let (Some(limit), Some(_)) = (schedule.limit_month_usd, crate::providers::split(&session.model)) {
            let (from, _) = crate::providers::this_month();
            match self.schedules.spent(&session.id, from) {
                Ok(spent) if spent >= limit => {
                    return Outcome::skipped(format!("Its limit for this month is spent: ${spent:.2} of ${limit:.2}. It runs again next month, or when the limit is raised."));
                }
                Ok(_) => {}
                Err(e) => return Outcome::failed(format!("What it spent this month could not be read: {e}")),
            }
        }
        // Schedules have no permission level: they always run autonomously (J2, the owner's direction).
        if session.permission != "autonomous"
            && let Err(e) = self.chat.set_permission(&session.id, "autonomous")
        {
            return Outcome::failed(format!("Its session could not be set to run on its own: {e}"));
        }
        // A session being answered keeps its model; the run waits its turn (J7).
        let waited_from = std::time::Instant::now();
        while self.chat.anyone_answering() {
            if waited_from.elapsed() > self.waits_at_most {
                return Outcome::failed(format!("A session was being answered for {} minutes, so the run gave up waiting for the model.", self.waits_at_most.as_secs() / 60));
            }
            tokio::time::sleep(self.look).await;
        }
        let turn = match self.chat.say(&session.id, &schedule.what) {
            Ok(turn) => turn,
            Err(e) => return Outcome::failed(format!("It could not be started: {e}")),
        };
        let answer = turn.answer.id;
        let started = std::time::Instant::now();
        loop {
            match self.sessions.message(&answer) {
                Ok(m) if m.state != "answering" => {
                    return match m.state.as_str() {
                        "done" => Outcome { state: "done", message: Some(answer), note: if m.content.trim().is_empty() { "It answered.".into() } else { one_line(&m.content, 200) }, watched: None },
                        _ => Outcome { state: "failed", message: Some(answer), note: m.problem.unwrap_or_else(|| format!("The answer ended {}.", m.state)), watched: None },
                    };
                }
                Ok(_) if started.elapsed() > self.at_most => {
                    self.chat.stop(&session.id);
                    // Stopping ends the answer; what it wrote so far stays in the session.
                    for _ in 0..200 {
                        if !self.chat.answering(&session.id) {
                            break;
                        }
                        tokio::time::sleep(self.look).await;
                    }
                    return Outcome { state: "failed", message: Some(answer), note: format!("It took longer than {} minutes, so it was stopped.", self.at_most.as_secs() / 60), watched: None };
                }
                Ok(_) => tokio::time::sleep(self.look).await,
                Err(e) => return Outcome::failed(format!("Its answer could not be read: {e}")),
            }
        }
    }

    /// A monitor: the shop's own price, read by code from the page's product data and compared by code (J5).
    /// It says something only when what it watches for becomes true; while it stays true, nothing more.
    async fn watch(&self, schedule: &Schedule, watch: &Watch) -> Outcome {
        let stop = AtomicBool::new(false);
        let (offers, address) = match self.chat.offers_at(&watch.url, &stop).await {
            Ok(found) => found,
            Err(e) => return Outcome::failed(format!("The page could not be read: {e}.")),
        };
        // The shop's own data where there is one; where there is none, the page's text read for its price by
        // the schedule's model, and the person told so (J5).
        let (price_now, currency, line, address, by_model) = match offers.into_iter().next() {
            Some(offer) => (offer.price, offer.currency, offer.line, address, false),
            None => match self.chat.price_on_page(&schedule.session_id, &watch.url, &schedule.said, &stop).await {
                Ok((price, at)) => (price, before_currency(schedule), format!("{} (read from the page's text)", number(price)), at, true),
                Err(e) => {
                    return Outcome::failed(format!(
                        "The page gives no product data of its own (the description shops publish for search engines), and its price could not be read from its text either: {e}."
                    ));
                }
            },
        };
        let before = schedule.watched.clone().unwrap_or_default();
        let first = before.first.or(Some(price_now));
        let holds = watch.price.holds(price_now, first);
        let watched = Watched { first, last: Some(price_now), currency: currency.clone(), holds, line: Some(line.clone()) };
        let price = format!("{}{}", number(price_now), currency.as_ref().map(|c| format!(" {c}")).unwrap_or_default());
        let against = match &watch.price {
            Price::Below { below } => format!("under the {} you set", number(*below)),
            Price::Above { above } => format!("over the {} you set", number(*above)),
            Price::Drops => format!("lower than the {} it was when this monitor first looked", first.map(number).unwrap_or_default()),
        };
        if holds && !before.holds {
            let source = if by_model {
                format!(
                    "The page at {address} gives no product data of its own, so the schedule's model read the price from the page's text. A page can tell a model a false price (research note 24): look at the page before you act on this."
                )
            } else {
                format!(
                    "The shop's own product data at {address} says: \"{line}\". Demido read this from the data the page publishes for search engines, not from its text, so a note in a review or a comment on the page cannot change it."
                )
            };
            let text = format!("The price is now {price}: {against}.\n\n{source}");
            let said = match self.sessions.add(&schedule.session_id, "assistant", &text, "done", None) {
                Ok(m) => m,
                Err(e) => return Outcome { state: "failed", message: None, note: format!("It found the price, but could not write it down: {e}"), watched: Some(watched) },
            };
            let _ = self.events.append("message.added", Some(&schedule.session_id), json!({ "message": said.id, "answer": said.id, "by": "schedule" }));
            let how = if by_model { " (read from the page's text by the model)" } else { "" };
            return Outcome { state: "told", message: Some(said.id), note: format!("{price}{how}, {against}: you were told."), watched: Some(watched) };
        }
        let note = match (holds, &watch.price) {
            (true, _) => format!("{price}: still {against}. You were told before."),
            (false, Price::Drops) if before.first.is_none() => format!("{price}: the first reading, which a drop is measured from."),
            (false, Price::Below { below }) => format!("{price}: not under {}.", number(*below)),
            (false, Price::Above { above }) => format!("{price}: not over {}.", number(*above)),
            (false, Price::Drops) => format!("{price}: not lower than the first reading."),
        };
        Outcome { state: "quiet", message: None, note, watched: Some(watched) }
    }
}

/// The currency a monitor saw before, for a price read from a page's text, which says no currency.
fn before_currency(schedule: &Schedule) -> Option<String> {
    schedule.watched.as_ref().and_then(|w| w.currency.clone())
}

/// A program such as Claude Code or OpenCode never runs a schedule (U23).
fn refuse_a_program(model: &str) -> Result<()> {
    match crate::providers::split(model) {
        Some((def, _)) if def.program => Err(Error::Invalid(format!(
            "{} cannot run a schedule: its terms are for a person's own use, not for runs with nobody there. Choose a model on this computer, or one through an API.",
            def.name
        ))),
        _ => Ok(()),
    }
}

/// Makes a schedule and the session of its own that its runs are in (J1): from the Schedules screen, or from a
/// model in a session when the person said yes (J13). Its session runs at Autonomous, and its level is never asked
/// about (J2). A program such as Claude Code or OpenCode is not offered: their terms are for a person's own use,
/// not for runs with nobody there (U23).
pub(crate) async fn make_with(chat: &Chat, schedules: &Schedules, events: &EventLog, new: &New) -> Result<Schedule> {
    new.when.check().map_err(Error::Invalid)?;
    if let Some(model) = &new.model {
        refuse_a_program(model)?;
    }
    // A monitor's session is named for what it watches; a task's, for what it does.
    let title = match &new.watch {
        Some(watch) => watch.said(),
        None => new.what.trim().to_string(),
    };
    let session = chat.start(Some(&one_line(&title, 60)), new.model.as_deref()).await?;
    let made = (|| -> Result<Schedule> {
        refuse_a_program(&session.model)?;
        if let Some(folder) = &new.folder {
            chat.set_folder(&session.id, Some(folder))?;
        }
        chat.set_permission(&session.id, "autonomous")?;
        schedules.create(&session.id, new)
    })();
    match made {
        Ok(made) => {
            events.append("schedule.created", Some(&made.id), json!({ "session": made.session_id, "what": made.what, "when": made.when_said, "watch": made.watch_said }))?;
            Ok(made)
        }
        Err(e) => {
            let _ = chat.delete(&session.id);
            Err(e)
        }
    }
}
