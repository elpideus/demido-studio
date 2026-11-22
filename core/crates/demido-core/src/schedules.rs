//! Schedules and monitors: work Demido does with nobody there (brief 4.27; DECISIONS section J).
//!
//! A schedule is a session of its own (J1): what to do, in the person's words, and when, as a rule kept in
//! code (`when`). It always runs autonomously, with every tool, and has no permission level (J2, the
//! owner's direction); what still holds in a run is what holds at every level (the folder, the sandbox,
//! Undo, the public internet only, the cleaning of text from outside). A monitor watches a shop's page for
//! a price, read by code from the shop's own product data, and says something only when what it watches
//! for becomes true (J5). Every run is kept, with what it did; three failures in a row pause a schedule and
//! say why (J10). The runs themselves are made by `Scheduler` (`runner`).

use std::sync::Arc;

use jiff::tz::TimeZone;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::Db;
use crate::{Error, Result, now_ms};

pub mod runner;
pub mod when;

pub use runner::Scheduler;
pub use when::{Price, Understood, Watch, When};

/// Failures in a row after which a schedule pauses itself (J10).
pub const PAUSES_AFTER: i64 = 3;

/// One schedule, as it is now.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Schedule {
    pub id: String,
    /// The session of its own that its runs are in.
    pub session_id: String,
    /// What to do, in the person's words.
    pub what: String,
    /// The sentence it was made from.
    pub said: String,
    pub when: When,
    /// When, in words: "Every day at 07:00".
    pub when_said: String,
    /// For a monitor: what it watches for.
    pub watch: Option<Watch>,
    pub watch_said: Option<String>,
    /// `on`, `paused` (by the person, or by itself after failures: `paused_why` says), or `done` (once, and run).
    pub state: String,
    pub paused_why: Option<String>,
    /// When it runs next. `null` when it is not due again.
    pub next_at: Option<i64>,
    pub last_run_at: Option<i64>,
    /// Runs in a row that failed.
    pub failures: i64,
    /// A monitor's last reading.
    pub watched: Option<Watched>,
    /// The most a paid provider may cost this schedule in a calendar month, in dollars (J8). `null`: no
    /// limit of its own; the limit for everything still holds.
    pub limit_month_usd: Option<f64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Its last run.
    pub last_run: Option<Run>,
}

/// What a monitor last read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Watched {
    /// The price at its first reading: what "drops" is measured from.
    pub first: Option<f64>,
    pub last: Option<f64>,
    pub currency: Option<String>,
    /// Whether what it watches for was true at the last reading: it speaks up only when this becomes true.
    pub holds: bool,
    /// The shop's own line for it: "Merino sweater: 64.00 EUR, InStock".
    pub line: Option<String>,
}

/// One run of a schedule.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Run {
    pub id: String,
    pub schedule_id: String,
    /// When it was due. A run the person started is due when they started it.
    pub due_at: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// `running`, `done`, `failed`, `told` (a monitor spoke up), `quiet` (a monitor, and nothing new) or
    /// `skipped` (it did not run, and `note` says why).
    pub state: String,
    /// It ran after its time, because the computer was asleep or Demido was not running.
    pub late: bool,
    /// The person started it ("Run now").
    pub by_hand: bool,
    /// The answer it wrote in its session, if it wrote one.
    pub message_id: Option<String>,
    /// What happened, in one line.
    pub note: String,
}

/// What a new schedule is made of. The session is made by the caller (`Scheduler::make`).
#[derive(Clone, Debug, PartialEq, Deserialize, ToSchema)]
pub struct New {
    /// The sentence it was made from.
    pub said: String,
    pub what: String,
    pub when: When,
    #[serde(default)]
    pub watch: Option<Watch>,
    /// The model its runs use. Left out: one that is loaded, or else the first on this computer.
    #[serde(default)]
    pub model: Option<String>,
    /// A folder for its runs' tools, by its full path.
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub limit_month_usd: Option<f64>,
}

/// A change to a schedule. What is left out stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, ToSchema)]
pub struct Change {
    pub what: Option<String>,
    pub when: Option<When>,
    /// `on` or `paused`.
    pub state: Option<String>,
    /// Set to a number, or to `null` to take the limit off. Left out: as it is.
    #[serde(default, deserialize_with = "some_or_null")]
    pub limit_month_usd: Option<Option<f64>>,
}

/// A text on one line, cut to `at_most` characters.
pub(crate) fn one_line(text: &str, at_most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= at_most { flat } else { format!("{}…", flat.chars().take(at_most).collect::<String>()) }
}

fn some_or_null<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<f64>>, D::Error> {
    Ok(Some(Option::<f64>::deserialize(d)?))
}

pub struct Schedules {
    db: Arc<Db>,
    tz: TimeZone,
}

const COLUMNS: &str = "id, session_id, what, said, rule, watch, state, paused_why, next_at, last_run_at, failures, watched, limit_month_usd, created_at, updated_at";
const RUN_COLUMNS: &str = "id, schedule_id, due_at, started_at, ended_at, state, late, by_hand, message_id, note";

impl Schedules {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db, tz: TimeZone::system() }
    }

    /// The same, in another time zone: for tests, so that "at 7" means the same on every computer.
    pub fn in_zone(mut self, tz: TimeZone) -> Self {
        self.tz = tz;
        self
    }

    pub fn tz(&self) -> &TimeZone {
        &self.tz
    }

    fn row(&self, r: &rusqlite::Row) -> rusqlite::Result<Schedule> {
        let rule: String = r.get(4)?;
        let watch: Option<String> = r.get(5)?;
        let watched: Option<String> = r.get(11)?;
        let when: When = serde_json::from_str(&rule).unwrap_or(When::Every { minutes: 24 * 60 });
        let watch: Option<Watch> = watch.and_then(|w| serde_json::from_str(&w).ok());
        Ok(Schedule {
            id: r.get(0)?,
            session_id: r.get(1)?,
            what: r.get(2)?,
            said: r.get(3)?,
            when_said: when.said(&self.tz),
            when,
            watch_said: watch.as_ref().map(Watch::said),
            watch,
            state: r.get(6)?,
            paused_why: r.get(7)?,
            next_at: r.get(8)?,
            last_run_at: r.get(9)?,
            failures: r.get(10)?,
            watched: watched.and_then(|w| serde_json::from_str(&w).ok()),
            limit_month_usd: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
            last_run: None,
        })
    }

    fn run_row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
        Ok(Run {
            id: r.get(0)?,
            schedule_id: r.get(1)?,
            due_at: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            state: r.get(5)?,
            late: r.get::<_, i64>(6)? != 0,
            by_hand: r.get::<_, i64>(7)? != 0,
            message_id: r.get(8)?,
            note: r.get(9)?,
        })
    }

    /// Keeps a new schedule, for a session already made for it. It is due first at the first moment its rule
    /// gives after now.
    pub fn create(&self, session_id: &str, new: &New) -> Result<Schedule> {
        new.when.check().map_err(Error::Invalid)?;
        let what = new.what.trim();
        if what.is_empty() && new.watch.is_none() {
            return Err(Error::Invalid("Say what the schedule should do.".into()));
        }
        if let Some(watch) = &new.watch {
            crate::web::guard::address(&watch.url).map_err(|e| Error::Invalid(format!("The monitor's address cannot be read: {e}.")))?;
        }
        if new.limit_month_usd.is_some_and(|l| !l.is_finite() || l < 0.0) {
            return Err(Error::Invalid("A limit is a sum of money of zero or more.".into()));
        }
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        let next = new.when.next_after(now, &self.tz);
        let rule = serde_json::to_string(&new.when)?;
        let watch = new.watch.as_ref().map(serde_json::to_string).transpose()?;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO schedules (id, session_id, what, said, rule, watch, state, next_at, failures, limit_month_usd, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'on', ?7, 0, ?8, ?9, ?9)",
                rusqlite::params![id, session_id, what, new.said.trim(), rule, watch, next, new.limit_month_usd, now],
            )
        })?;
        self.get(&id)
    }

    pub fn get(&self, id: &str) -> Result<Schedule> {
        let found = self.db.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM schedules WHERE id = ?1"), [id], |r| self.row(r)).optional())?;
        let mut schedule = found.ok_or_else(|| Error::NotFound(format!("There is no schedule {id}.")))?;
        schedule.last_run = self.runs(id, 1)?.into_iter().next();
        Ok(schedule)
    }

    /// Every schedule, the next due first; those not due again last.
    pub fn list(&self) -> Result<Vec<Schedule>> {
        let mut all = self.db.with(|c| {
            let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM schedules ORDER BY next_at IS NULL, next_at, created_at"))?;
            q.query_map([], |r| self.row(r))?.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for schedule in &mut all {
            schedule.last_run = self.runs(&schedule.id, 1)?.into_iter().next();
        }
        Ok(all)
    }

    /// The schedule of a session, if the session is one's.
    pub fn of_session(&self, session_id: &str) -> Result<Option<Schedule>> {
        let id: Option<String> = self.db.with(|c| c.query_row("SELECT id FROM schedules WHERE session_id = ?1", [session_id], |r| r.get(0)).optional())?;
        id.map(|id| self.get(&id)).transpose()
    }

    /// The schedules that are on and due by `now`, the longest waiting first.
    pub fn due(&self, now: i64) -> Result<Vec<Schedule>> {
        let ids: Vec<String> = self.db.with(|c| {
            let mut q = c.prepare("SELECT id FROM schedules WHERE state = 'on' AND next_at IS NOT NULL AND next_at <= ?1 ORDER BY next_at")?;
            q.query_map([now], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()
        })?;
        ids.iter().map(|id| self.get(id)).collect()
    }

    pub fn change(&self, id: &str, change: &Change) -> Result<Schedule> {
        let before = self.get(id)?;
        let now = now_ms();
        if let Some(when) = &change.when {
            when.check().map_err(Error::Invalid)?;
        }
        let what = change.what.as_deref().map(str::trim).unwrap_or(&before.what).to_string();
        if what.is_empty() && before.watch.is_none() {
            return Err(Error::Invalid("Say what the schedule should do.".into()));
        }
        let when = change.when.clone().unwrap_or(before.when.clone());
        let state = match change.state.as_deref() {
            None => before.state.clone(),
            Some(s @ ("on" | "paused")) => s.to_string(),
            Some(other) => return Err(Error::Invalid(format!("A schedule is \"on\" or \"paused\", not \"{other}\"."))),
        };
        // Turned on again, or given a new time, it is due at the next moment its rule gives from now.
        let fresh = change.when.is_some() || (state == "on" && before.state != "on");
        let next = if fresh { when.next_after(now, &self.tz) } else { before.next_at };
        let state = if state == "on" && next.is_none() { "done".to_string() } else { state };
        let (why, failures) = if state == "on" {
            (None, 0)
        } else if state == "paused" && before.state == "on" {
            (Some("Paused by you.".to_string()), before.failures)
        } else {
            (before.paused_why.clone(), before.failures)
        };
        let limit = change.limit_month_usd.unwrap_or(before.limit_month_usd);
        if limit.is_some_and(|l| !l.is_finite() || l < 0.0) {
            return Err(Error::Invalid("A limit is a sum of money of zero or more.".into()));
        }
        let rule = serde_json::to_string(&when)?;
        self.db.with(|c| {
            c.execute(
                "UPDATE schedules SET what = ?2, rule = ?3, state = ?4, paused_why = ?5, next_at = ?6, failures = ?7, limit_month_usd = ?8, updated_at = ?9 WHERE id = ?1",
                rusqlite::params![id, what, rule, state, why, next, failures, limit, now],
            )
        })?;
        self.get(id)
    }

    /// Deletes a schedule and its runs. Its session stays, with everything its runs wrote.
    pub fn delete(&self, id: &str) -> Result<Schedule> {
        let gone = self.get(id)?;
        self.db.with(|c| c.execute("DELETE FROM schedules WHERE id = ?1", [id]))?;
        Ok(gone)
    }

    /// The runs of a schedule, the last first.
    pub fn runs(&self, id: &str, at_most: usize) -> Result<Vec<Run>> {
        self.db.with(|c| {
            let mut q = c.prepare(&format!("SELECT {RUN_COLUMNS} FROM schedule_runs WHERE schedule_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT ?2"))?;
            q.query_map(rusqlite::params![id, at_most as i64], Self::run_row)?.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// Writes down that a run has started.
    pub fn start_run(&self, schedule_id: &str, due_at: i64, late: bool, by_hand: bool) -> Result<Run> {
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        self.db.with(|c| {
            c.execute(
                "INSERT INTO schedule_runs (id, schedule_id, due_at, started_at, state, late, by_hand, note) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, '')",
                rusqlite::params![id, schedule_id, due_at, now, i64::from(late), i64::from(by_hand)],
            )
        })?;
        self.run(&id)
    }

    pub fn run(&self, id: &str) -> Result<Run> {
        self.db
            .with(|c| c.query_row(&format!("SELECT {RUN_COLUMNS} FROM schedule_runs WHERE id = ?1"), [id], Self::run_row).optional())?
            .ok_or_else(|| Error::NotFound(format!("There is no run {id}.")))
    }

    /// Writes down how a run ended, and what follows for its schedule: when it is due next (unless the person
    /// started it), how many runs in a row failed, and a pause after three (J10). `watched`: a monitor's reading.
    pub fn end_run(&self, run_id: &str, state: &str, message_id: Option<&str>, note: &str, watched: Option<&Watched>) -> Result<Schedule> {
        let run = self.run(run_id)?;
        let before = self.get(&run.schedule_id)?;
        let now = now_ms();
        self.db.with(|c| {
            c.execute("UPDATE schedule_runs SET ended_at = ?2, state = ?3, message_id = ?4, note = ?5 WHERE id = ?1", rusqlite::params![run_id, now, state, message_id, one_line(note, 300)])
        })?;
        let failed = state == "failed";
        let failures = if failed {
            before.failures + 1
        } else if state == "skipped" {
            before.failures
        } else {
            0
        };
        // A missed run is run once, late, and the next is counted from now: missed runs are never run several
        // times over to catch up (J9).
        let next = if run.by_hand { before.next_at } else { before.when.next_after(now.max(run.due_at), &self.tz) };
        let (state, why) = if before.state != "on" {
            (before.state.clone(), before.paused_why.clone())
        } else if failures >= PAUSES_AFTER {
            ("paused".to_string(), Some(format!("Paused after {failures} runs in a row failed. The last: {}", one_line(note, 200))))
        } else if next.is_none() {
            ("done".to_string(), None)
        } else {
            ("on".to_string(), None)
        };
        let watched = watched.map(serde_json::to_string).transpose()?;
        self.db.with(|c| {
            c.execute(
                "UPDATE schedules SET state = ?2, paused_why = ?3, next_at = ?4, last_run_at = ?5, failures = ?6, watched = COALESCE(?7, watched), updated_at = ?5 WHERE id = ?1",
                rusqlite::params![before.id, state, why, next, now, failures, watched],
            )
        })?;
        self.get(&before.id)
    }

    /// What a session's answers from paid providers cost since `from_ms`, in dollars (J8).
    pub fn spent(&self, session_id: &str, from_ms: i64) -> Result<f64> {
        self.db.with(|c| c.query_row("SELECT COALESCE(SUM(cost_usd), 0.0) FROM usage WHERE session_id = ?1 AND at >= ?2", rusqlite::params![session_id, from_ms], |r| r.get(0)))
    }

    /// A run left `running` when Demido stopped never finished: it is said to have failed, so that it does
    /// not count as running for ever. Its schedule is due again by its rule.
    pub fn close_unfinished(&self) -> Result<usize> {
        let ids: Vec<String> = self.db.with(|c| {
            let mut q = c.prepare("SELECT id FROM schedule_runs WHERE state = 'running'")?;
            q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()
        })?;
        for id in &ids {
            self.end_run(id, "failed", None, "Demido stopped while this run was under way.", None)?;
        }
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Schedules, Arc<crate::sessions::Sessions>) {
        let db = Arc::new(Db::memory().unwrap());
        (Schedules::new(db.clone()).in_zone(TimeZone::get("Europe/Rome").unwrap()), Arc::new(crate::sessions::Sessions::new(db)))
    }
    fn new(when: When) -> New {
        New { said: "Give me my report every morning at 7".into(), what: "Give me my report".into(), when, watch: None, model: None, folder: None, limit_month_usd: None }
    }

    #[test]
    fn a_schedule_is_kept_with_its_session_and_is_due_by_its_rule() {
        let (s, sessions) = store();
        let session = sessions.create(Some("Report"), "small").unwrap();
        let made = s.create(&session.id, &new(When::Daily { at: "07:00".into() })).unwrap();
        assert_eq!((made.state.as_str(), made.when_said.as_str(), made.session_id.as_str()), ("on", "Every day at 07:00", session.id.as_str()));
        let next = made.next_at.unwrap();
        assert!(next > now_ms() && next - now_ms() <= 25 * 3_600_000);
        assert_eq!(s.of_session(&session.id).unwrap().map(|x| x.id), Some(made.id.clone()));
        assert!(s.due(now_ms()).unwrap().is_empty(), "not due yet");
        assert_eq!(s.due(next).unwrap().len(), 1);
        assert!(s.create(&session.id, &new(When::Every { minutes: 5 })).is_err(), "never more often than every 15 minutes");
        let mut nothing = new(When::Daily { at: "07:00".into() });
        nothing.what = " ".into();
        assert!(s.create(&session.id, &nothing).is_err());
        // Its session goes, and it goes with it.
        sessions.delete(&session.id).unwrap();
        assert!(s.get(&made.id).is_err());
    }

    #[test]
    fn three_failed_runs_in_a_row_pause_a_schedule_and_say_why() {
        let (s, sessions) = store();
        let session = sessions.create(None, "small").unwrap();
        let made = s.create(&session.id, &new(When::Every { minutes: 60 })).unwrap();
        for n in 1..=3 {
            let run = s.start_run(&made.id, made.next_at.unwrap(), false, false).unwrap();
            let after = s.end_run(&run.id, "failed", None, "the model could not be loaded", None).unwrap();
            assert_eq!(after.failures, n);
            assert_eq!(after.state, if n < 3 { "on" } else { "paused" });
        }
        let paused = s.get(&made.id).unwrap();
        assert!(paused.paused_why.as_deref().unwrap().starts_with("Paused after 3 runs in a row failed. The last: the model could not be loaded"));
        assert!(s.due(i64::MAX).unwrap().is_empty(), "a paused schedule is not due");
        // Turned on again: due from now, and the count starts over.
        let on = s.change(&made.id, &Change { state: Some("on".into()), ..Change::default() }).unwrap();
        assert_eq!((on.state.as_str(), on.failures, on.paused_why), ("on", 0, None));
        assert!(on.next_at.unwrap() > now_ms());
        // A run that went well also starts the count over.
        let run = s.start_run(&made.id, now_ms(), false, false).unwrap();
        s.end_run(&run.id, "failed", None, "x", None).unwrap();
        let run = s.start_run(&made.id, now_ms(), false, false).unwrap();
        assert_eq!(s.end_run(&run.id, "done", None, "It is done.", None).unwrap().failures, 0);
        assert_eq!(s.runs(&made.id, 10).unwrap().len(), 5);
        assert_eq!(s.get(&made.id).unwrap().last_run.unwrap().state, "done");
    }

    #[test]
    fn a_missed_run_runs_once_and_the_next_is_counted_from_now() {
        let (s, sessions) = store();
        let session = sessions.create(None, "small").unwrap();
        let made = s.create(&session.id, &new(When::Every { minutes: 60 })).unwrap();
        // Due ten hours ago (the computer slept): one run, and the next an hour from now, not ten runs.
        let long_ago = now_ms() - 10 * 3_600_000;
        let run = s.start_run(&made.id, long_ago, true, false).unwrap();
        let after = s.end_run(&run.id, "done", None, "Done, late.", None).unwrap();
        let next = after.next_at.unwrap();
        assert!(next > now_ms() + 59 * 60_000 && next <= now_ms() + 60 * 60_000, "{}", next - now_ms());
        assert!(s.runs(&made.id, 10).unwrap()[0].late);
        // A run the person started does not move the next one.
        let run = s.start_run(&made.id, now_ms(), false, true).unwrap();
        assert_eq!(s.end_run(&run.id, "done", None, "By hand.", None).unwrap().next_at, Some(next));
        // Once, and run: done.
        let once = s.create(&session.id, &new(When::Once { at_ms: now_ms() + 60_000 })).unwrap();
        let run = s.start_run(&once.id, once.next_at.unwrap(), false, false).unwrap();
        let after = s.end_run(&run.id, "done", None, "Done.", None).unwrap();
        assert_eq!((after.state.as_str(), after.next_at), ("done", None));
        // A run left running when Demido stopped is closed as failed.
        let run = s.start_run(&made.id, now_ms(), false, false).unwrap();
        assert_eq!(s.close_unfinished().unwrap(), 1);
        assert_eq!(s.run(&run.id).unwrap().state, "failed");
    }

    #[test]
    fn a_change_keeps_what_it_does_not_name() {
        let (s, sessions) = store();
        let session = sessions.create(None, "small").unwrap();
        let made = s.create(&session.id, &new(When::Daily { at: "07:00".into() })).unwrap();
        let changed = s.change(&made.id, &Change { when: Some(When::Daily { at: "08:15".into() }), limit_month_usd: Some(Some(2.5)), ..Change::default() }).unwrap();
        assert_eq!((changed.what.as_str(), changed.when_said.as_str(), changed.limit_month_usd), ("Give me my report", "Every day at 08:15", Some(2.5)));
        let paused = s.change(&made.id, &Change { state: Some("paused".into()), ..Change::default() }).unwrap();
        assert_eq!((paused.state.as_str(), paused.paused_why.as_deref(), paused.limit_month_usd), ("paused", Some("Paused by you."), Some(2.5)));
        let off = s.change(&made.id, &serde_json::from_str::<Change>(r#"{"limit_month_usd": null}"#).unwrap()).unwrap();
        assert_eq!(off.limit_month_usd, None);
        assert!(s.change(&made.id, &Change { state: Some("maybe".into()), ..Change::default() }).is_err());
        s.delete(&made.id).unwrap();
        assert!(s.get(&made.id).is_err());
        assert!(sessions.get(&session.id).is_ok(), "the session stays, with what its runs wrote");
    }
}
