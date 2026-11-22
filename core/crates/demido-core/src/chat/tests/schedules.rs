//! Schedules and monitors, run against the stand-in model and the stand-in site (DECISIONS section J).

use super::*;
use crate::schedules::{Change, New, Price, Scheduler, Schedules, Watch, When};
use crate::web::tests as site;

fn scheduler(w: &World) -> Scheduler {
    let schedules = Arc::new(Schedules::new(w.db.clone()).in_zone(jiff::tz::TimeZone::get("Europe/Rome").unwrap()));
    Scheduler::new(schedules, w.sessions.clone(), w.chat.clone(), w.events.clone()).quicker(Duration::from_secs(10), Duration::from_secs(2))
}

fn task(what: &str, when: When) -> New {
    New { said: format!("{what} every morning at 7"), what: what.into(), when, watch: None, model: Some("small".into()), folder: None, limit_month_usd: None }
}

fn ran(w: &World) -> Vec<Value> {
    w.events.after(0, 500).unwrap().into_iter().filter(|e| e.kind == "schedule.ran").map(|e| e.data).collect()
}

#[tokio::test]
async fn a_schedule_is_a_session_of_its_own_and_a_run_is_its_words_said_there_autonomously() {
    let w = world("schedule-task");
    w.stand.replies.lock().unwrap().extend(["Here is your report: two new messages.".to_string(), "Today: nothing new.".to_string()]);
    let s = scheduler(&w);
    let made = s.make(&task("Give me my email report", When::Daily { at: "07:00".into() })).await.unwrap();
    let session = w.sessions.get(&made.session_id).unwrap();
    // A session of its own, with what to do as its name, at Autonomous: schedules have no level (J2).
    assert_eq!((session.title.as_str(), session.permission.as_str(), session.model.as_str()), ("Give me my email report", "autonomous", "small"));
    assert_eq!(made.when_said, "Every day at 07:00");
    let next = made.next_at.unwrap();

    // "Run now": its words, said in its session, answered as any request is. Its next time does not move.
    let run = s.run(&made.id, true).await.unwrap();
    assert_eq!((run.state.as_str(), run.by_hand, run.late), ("done", true, false), "{}", run.note);
    assert_eq!(run.note, "Here is your report: two new messages.");
    let said: Vec<(String, String)> = w.sessions.messages(&made.session_id).unwrap().into_iter().map(|m| (m.role, m.content)).collect();
    assert_eq!(said, [("user".to_string(), "Give me my email report".to_string()), ("assistant".to_string(), "Here is your report: two new messages.".to_string())]);
    assert_eq!(run.message_id.as_deref(), Some(w.sessions.messages(&made.session_id).unwrap()[1].id.as_str()));
    assert_eq!(s.schedules().get(&made.id).unwrap().next_at, Some(next));
    // The window is told what it gave (J6).
    let told = ran(&w);
    assert_eq!((told[0]["state"].as_str(), told[0]["note"].as_str(), told[0]["session"].as_str()), (Some("done"), Some("Here is your report: two new messages."), Some(made.session_id.as_str())));

    // Nothing is due before its time; at its time it runs, and is due again the next day.
    s.look(next - 1).await;
    assert_eq!(s.schedules().runs(&made.id, 10).unwrap().len(), 1);
    s.look(next).await;
    let after = s.schedules().get(&made.id).unwrap();
    assert_eq!(s.schedules().runs(&made.id, 10).unwrap().len(), 2);
    assert_eq!(after.last_run.as_ref().map(|r| (r.state.as_str(), r.by_hand)), Some(("done", false)));
    assert!(after.next_at.unwrap() > next, "due again by its rule");

    // A program never runs a schedule (U23).
    let mut by_program = task("Fix the tests", When::Daily { at: "07:00".into() });
    by_program.model = Some("claude-code/claude-sonnet-4-5".into());
    let refused = s.make(&by_program).await.unwrap_err().to_string();
    assert!(refused.contains("Claude Code cannot run a schedule"), "{refused}");
    // Nor more often than every 15 minutes (J9), and no session is left behind by a refusal.
    let before = w.sessions.list().unwrap().len();
    assert!(s.make(&task("Check", When::Every { minutes: 10 })).await.is_err());
    assert_eq!(w.sessions.list().unwrap().len(), before);
    w.end().await;
}

#[tokio::test]
async fn a_level_set_by_hand_in_a_schedules_session_does_not_hold_at_its_next_run() {
    let w = world("schedule-level");
    w.stand.replies.lock().unwrap().push_back("Done.".into());
    let s = scheduler(&w);
    let made = s.make(&task("Tidy the notes", When::Every { minutes: 60 })).await.unwrap();
    w.chat.set_permission(&made.session_id, "cautious").unwrap();
    assert_eq!(s.run(&made.id, true).await.unwrap().state, "done");
    assert_eq!(w.sessions.get(&made.session_id).unwrap().permission, "autonomous");
    // Paused, it is not due; deleted, its session stays with what its runs wrote.
    let paused = s.schedules().change(&made.id, &Change { state: Some("paused".into()), ..Change::default() }).unwrap();
    s.look(i64::MAX / 2).await;
    assert_eq!(s.schedules().runs(&paused.id, 10).unwrap().len(), 1);
    s.schedules().delete(&made.id).unwrap();
    assert_eq!(w.sessions.messages(&made.session_id).unwrap().len(), 2);
    w.end().await;
}

#[tokio::test]
async fn a_monitor_reads_the_shops_own_price_by_code_and_speaks_up_once() {
    let mut w = world("schedule-monitor");
    let net = site::world().await;
    let web = crate::web::Web::with(Some(net.kept_in.to_path_buf()), net.web.sources.clone(), vec![net.base.trim_start_matches("http://").to_string()]);
    w.chat = Arc::new(Chat::new(w.sessions.clone(), w.engine.clone(), w.events.clone(), w.settings.clone()).with_clock(CLOCK).with_web(Arc::new(web)));
    let s = scheduler(&w);
    let monitor = |below: f64, path: &str| New {
        said: format!("Tell me when the sweater at {} goes under {below}", net.at(path)),
        what: "Tell me when the sweater goes under the price".into(),
        when: When::Every { minutes: 360 },
        watch: Some(Watch { url: net.at(path), price: Price::Below { below } }),
        model: Some("small".into()),
        folder: None,
        limit_month_usd: None,
    };
    // The shop's data says 79.00 EUR; a "review" on the page says 49. The price is the shop's, read by code.
    let under_80 = s.make(&monitor(80.0, "/shop")).await.unwrap();
    let run = s.run(&under_80.id, true).await.unwrap();
    assert_eq!((run.state.as_str(), run.note.as_str()), ("told", "79 EUR, under the 80 you set: you were told."));
    let message = w.sessions.message(run.message_id.as_deref().unwrap()).unwrap();
    assert!(message.content.starts_with("The price is now 79 EUR: under the 80 you set.") && message.content.contains("\"Merino crew neck, navy: 79.00 EUR, InStock\""), "{}", message.content);
    let watched = s.schedules().get(&under_80.id).unwrap().watched.unwrap();
    assert_eq!((watched.first, watched.last, watched.holds, watched.currency.as_deref()), (Some(79.0), Some(79.0), true, Some("EUR")));
    // Still true: nothing more is said.
    let again = s.run(&under_80.id, true).await.unwrap();
    assert_eq!((again.state.as_str(), again.message_id), ("quiet", None));
    assert_eq!(again.note, "79 EUR: still under the 80 you set. You were told before.");
    assert_eq!(w.sessions.messages(&under_80.session_id).unwrap().len(), 1, "told once");

    // Not true: quiet, and never 49 from the note on the page.
    let under_70 = s.make(&monitor(70.0, "/shop")).await.unwrap();
    let quiet = s.run(&under_70.id, true).await.unwrap();
    assert_eq!((quiet.state.as_str(), quiet.note.as_str()), ("quiet", "79 EUR: not under 70."));
    assert!(w.sessions.messages(&under_70.session_id).unwrap().is_empty());

    // A page with no product data of its own: the price is not guessed. Three of those in a row pause it.
    let no_data = s.make(&monitor(80.0, "/article")).await.unwrap();
    for _ in 0..3 {
        let failed = s.run(&no_data.id, false).await.unwrap();
        assert_eq!(failed.state, "failed");
        assert!(failed.note.starts_with("The page gives no product data of its own"), "{}", failed.note);
    }
    let paused = s.schedules().get(&no_data.id).unwrap();
    assert_eq!(paused.state, "paused");
    assert!(paused.paused_why.unwrap().starts_with("Paused after 3 runs in a row failed."));
    assert!(ran(&w).last().unwrap()["paused"].as_bool().unwrap());
    // No model was asked anything: a monitor reads and compares in code.
    assert!(w.stand.asked.lock().unwrap().is_empty());
    drop(net);
    w.end().await;
}

#[tokio::test]
async fn a_run_waits_for_a_session_being_answered_and_gives_up_after_a_while() {
    let w = world("schedule-waits");
    let s = scheduler(&w);
    let made = s.make(&task("Summarise", When::Every { minutes: 60 })).await.unwrap();
    // Another session is being answered, and its answer goes on (a word every five seconds).
    w.stand.word_every_ms.store(5_000, Ordering::Relaxed);
    let other = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say(&other.id, "Write a long story.").unwrap();
    let run = s.run(&made.id, true).await.unwrap();
    assert_eq!(run.state, "failed");
    assert!(run.note.starts_with("A session was being answered"), "{}", run.note);
    assert!(w.sessions.messages(&made.session_id).unwrap().is_empty(), "the model was not taken from it");
    w.chat.stop(&other.id);
    w.stand.word_every_ms.store(0, Ordering::Relaxed);
    w.answered(&other.id).await;
    w.end().await;
}
