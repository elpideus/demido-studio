//! A turn with a model through a program on this computer: Claude Code or OpenCode (`programs`).
//!
//! The program runs its own loop with its own tools. Demido gives it the person's message (with whatever
//! of the conversation it has not seen yet), shows what it writes as it writes it, shows each tool it uses
//! as a step, asks the person what the program asks to be allowed, and counts what it said the turn cost.
//! Before the turn, the session's folder is saved, as before a program Demido runs itself: whatever the
//! program changed in it can be undone in one go (DECISIONS U19).
//!
//! What Demido's own loop does for small models (the question of which tool, its tools, its tasks, making
//! a conversation shorter) is not done here: the program does all of that its own way.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use super::{Chat, INSTRUCTIONS, Running};
use crate::engine::{Answer, Piece};
use crate::permissions::{Level, Said};
use crate::programs::{self, Approval, Choice, Event};
use crate::providers::{self, For};
use crate::sessions::{Message, ProgramSession, Session};
use crate::tools::{Reach, Step};
use crate::{Error, Result};

/// What a tool's result is cut to, in what is kept and shown. The program has the whole of it.
const RESULT_KEPT: usize = 20_000;

/// A turn a program is running, as Demido's approval tool finds it by the turn's token.
#[derive(Clone)]
pub(super) struct Live {
    session: Session,
    answer: String,
    steps: Arc<Mutex<Vec<Step>>>,
    stop: Arc<AtomicBool>,
}

/// How far a tool reaches, from its name in Claude Code or its kind in OpenCode.
fn reach_of(tool: &str) -> Reach {
    match tool {
        "Bash" | "BashOutput" | "KillShell" | "execute" => Reach::Runs,
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "read" | "search" | "think" => Reach::Reads,
        "WebFetch" | "WebSearch" | "fetch" => Reach::Network,
        _ => Reach::Changes,
    }
}

/// What an ACP tool kind does, as a verb: "read", "change", "run".
fn verb_of(kind: &str) -> &'static str {
    match kind {
        "read" | "search" => "read",
        "edit" => "change",
        "delete" => "delete",
        "move" => "move",
        "execute" => "run",
        "fetch" => "fetch",
        _ => "use",
    }
}

/// What a tool of Claude Code's would do, in one plain line.
fn claude_code_what(tool: &str, input: &Value) -> String {
    let text = |name: &str| input[name].as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    let short = |s: String| if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s };
    match tool {
        "Bash" => format!("Claude Code wants to run: {}", short(text("command"))),
        "Edit" | "MultiEdit" => format!("Claude Code wants to change {}", text("file_path")),
        "Write" => format!("Claude Code wants to write {}", text("file_path")),
        "NotebookEdit" => format!("Claude Code wants to change {}", text("notebook_path")),
        "WebFetch" => format!("Claude Code wants to read {}", text("url")),
        "WebSearch" => format!("Claude Code wants to search the internet for \"{}\"", short(text("query"))),
        "Read" => format!("Claude Code wants to read {}", text("file_path")),
        other => format!("Claude Code wants to use {other}"),
    }
}

/// Claude Code's permission mode for a session's level. Balanced lets it change files without asking,
/// because the folder is saved before the turn; Smart is Claude Code's own "auto", where a second look
/// decides what needs a person, which is what Smart means.
fn claude_code_mode(level: Level) -> &'static str {
    match level {
        Level::Cautious => "manual",
        Level::Balanced => "acceptEdits",
        Level::Smart => "auto",
        Level::Autonomous => "bypassPermissions",
    }
}

/// A message for the program, from what it has not seen of the conversation and the request.
fn prompt_from(unseen: &[Message], request: &str) -> String {
    let earlier: Vec<String> =
        unseen.iter().filter(|m| !(m.role == "assistant" && m.content.trim().is_empty())).map(|m| format!("{}: {}", if m.role == "user" { "Person" } else { "Model" }, m.content.trim())).collect();
    if earlier.is_empty() {
        return request.to_string();
    }
    format!("Earlier in this conversation, which you did not see (another model, or another account, answered it):\n\n{}\n\nThe person now says:\n\n{request}", earlier.join("\n\n"))
}

impl Chat {
    /// Where Demido's server answers, for a program's approval tool. Set once, by the server.
    pub fn set_address(&self, address: String) {
        let _ = self.address.set(address);
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, Live>> {
        self.programs_live.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What Demido's approval tool answers Claude Code, for the turn the token names: the person's yes or
    /// no, or the session's level. False, with the reason, when the token names no turn that runs.
    pub async fn program_approval(&self, token: &str, tool: &str, input: Value) -> (bool, String) {
        let Some(live) = self.live().get(token).cloned() else {
            return (false, "Demido has no turn for this request any more: it ended or was stopped.".into());
        };
        let what = claude_code_what(tool, &input);
        let yes = self.approve(&live, tool, reach_of(tool), what, input, "Claude Code asks before this, by its own rules for this session's level.").await;
        if yes { (true, String::new()) } else { (false, "The person said no.".into()) }
    }

    /// Whether a program may do what it asks: by the session's level, by what the person said may go on
    /// in this session, and otherwise by asking the person.
    async fn approve(&self, live: &Live, tool: &str, reach: Reach, what: String, input: Value, why: &str) -> bool {
        let level = Level::from(&live.session.permission).unwrap_or(Level::Balanced);
        if level == Level::Autonomous || self.granted(&live.session.id, tool) {
            return true;
        }
        let args = match &input {
            Value::Object(map) => map.clone(),
            other => serde_json::Map::from_iter([("input".to_string(), other.clone())]),
        };
        // The step it is about waits with it, and shows what is asked for: the person's own hooks may have
        // changed the call since the model wrote it (one rewrote `cat` into `rtk read`).
        let at = {
            let mut steps = live.steps.lock().unwrap_or_else(|e| e.into_inner());
            let at = steps.iter().rposition(|s| s.tool == tool && s.state == "running");
            if let Some(step) = at.and_then(|i| steps.get_mut(i))
                && input.is_object()
            {
                step.args = serde_json::from_str(&self.hide(&input.to_string())).unwrap_or(Value::Null);
            }
            at
        };
        self.mark(live, at, "waiting", None);
        let said = self.ask_person(&live.session, &live.answer, tool, &args, reach, why.to_string(), false, &live.stop, Some(what)).await.unwrap_or(Said::No);
        let yes = said != Said::No;
        self.mark(live, at, if yes { "running" } else { "refused" }, (!yes).then_some("the person said no"));
        yes
    }

    fn mark(&self, live: &Live, at: Option<usize>, state: &str, result: Option<&str>) {
        let steps = {
            let mut steps = live.steps.lock().unwrap_or_else(|e| e.into_inner());
            let Some(step) = at.and_then(|i| steps.get_mut(i)) else { return };
            step.state = state.into();
            if let Some(result) = result {
                step.result = result.into();
            }
            steps.clone()
        };
        let _ = self.sessions.set_steps(&live.answer, &steps);
        self.events.tell("message.steps", Some(&live.session.id), json!({ "message": live.answer, "steps": steps }));
    }

    /// Runs a turn through the session's program. `heard` is told the answer as it is written.
    pub(super) async fn program_answer(&self, session: &Session, answer_id: &str, running: &Running, heard: &mut (dyn FnMut(Piece) + Send)) -> Result<Answer> {
        let (def, name) = providers::split(&session.model).filter(|(d, _)| d.program).ok_or_else(|| Error::Invalid(format!("{} is not run by a program.", session.model)))?;
        let providers = self.providers()?.clone();
        let program = providers.program(def.id)?;
        // Claude Code runs in the profile of the session's account (or the default); OpenCode has one.
        let account = if def.id == "claude-code" { Some(providers.account_for(def, session.account.as_deref())?.id) } else { None };
        // Never one that says it does not keep to that profile: it would reach the person's own login.
        if def.id == "claude-code" {
            let found = providers.look_at(def.id, account.as_deref()).await;
            if found.contained == Some(false) {
                return Err(Error::Invalid(found.problem.unwrap_or_default()));
            }
        }
        // Where it works: the session's folder, or a folder of its own, so that it never works in Demido's.
        let cwd = match session.folder.as_deref() {
            Some(folder) => std::path::PathBuf::from(folder),
            None => {
                let own = self.programs_in.join(&session.id);
                std::fs::create_dir_all(&own)?;
                own
            }
        };
        // The folder is saved first: all the program does in it can then be undone.
        let mut saved = None;
        if let Some(folder) = session.folder.clone() {
            let id = uuid::Uuid::now_v7().to_string();
            let place = self.kept_in.join(&session.id).join(&id);
            let into = place.clone();
            if tokio::task::spawn_blocking(move || crate::checkpoint::save(std::path::Path::new(&folder), &into)).await.is_ok_and(|done| done.is_ok()) {
                saved = Some((id, place));
            }
        }
        // What the program has not seen of the conversation, and the request. Its session is kept in one
        // account's profile: with another account, it starts again and is given the whole conversation.
        let known = self.sessions.program_session(&session.id, def.id)?.filter(|k| k.account == account.clone().unwrap_or_default());
        let seen = known.as_ref().map_or(0, |k| k.seen_seq);
        let messages = self.sessions.messages(&session.id)?;
        let answer_seq = messages.iter().find(|m| m.id == answer_id).map_or(i64::MAX, |m| i64::from(m.seq));
        let request_at = messages.iter().rposition(|m| m.role == "user" && i64::from(m.seq) < answer_seq);
        let Some(request_at) = request_at else {
            return Err(Error::Invalid("There is nothing to answer.".into()));
        };
        let unseen: Vec<Message> = messages[..request_at].iter().filter(|m| i64::from(m.seq) > seen).cloned().collect();
        let prompt = self.hide(&prompt_from(&unseen, &messages[request_at].content));

        let steps: Arc<Mutex<Vec<Step>>> = Arc::default();
        let live = Live { session: session.clone(), answer: answer_id.to_string(), steps: steps.clone(), stop: running.stop.clone() };
        let started_at = Instant::now();
        let (mut content, mut thought) = (String::new(), String::new());
        let mut began: HashMap<String, (usize, Instant)> = HashMap::new();
        let shown = |steps: &[Step]| {
            let _ = self.sessions.set_steps(answer_id, steps);
            self.events.tell("message.steps", Some(&session.id), json!({ "message": answer_id, "steps": steps }));
        };
        let mut on = |event: Event| match event {
            Event::Text(text) => {
                content.push_str(&text);
                heard(Piece::Text(&text));
            }
            Event::Thought(text) => {
                thought.push_str(&text);
                heard(Piece::Thought(&text));
            }
            Event::Tool { id, name, input } => {
                let list = {
                    let mut list = steps.lock().unwrap_or_else(|e| e.into_inner());
                    let args = serde_json::from_str(&self.hide(&input.to_string())).unwrap_or(Value::Null);
                    list.push(Step { tool: name, args, state: "running".into(), ..Step::default() });
                    began.insert(id, (list.len() - 1, Instant::now()));
                    list.clone()
                };
                shown(&list);
            }
            Event::ToolDone { id, result, failed } => {
                let list = {
                    let mut list = steps.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some((at, when)) = began.get(&id)
                        && let Some(step) = list.get_mut(*at)
                    {
                        let mut kept = self.hide(&result);
                        if kept.len() > RESULT_KEPT {
                            let cut = (0..=RESULT_KEPT).rev().find(|c| kept.is_char_boundary(*c)).unwrap_or(0);
                            kept.truncate(cut);
                            kept.push_str("\n… (the rest is with the program)");
                        }
                        if step.state != "refused" {
                            step.state = if failed { "failed" } else { "done" }.into();
                            step.result = kept;
                        }
                        step.took_ms = when.elapsed().as_millis() as u64;
                    }
                    list.clone()
                };
                shown(&list);
            }
            Event::ToolInput { id, input } => {
                let list = {
                    let mut list = steps.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some((at, _)) = began.get(&id)
                        && let Some(step) = list.get_mut(*at)
                    {
                        step.args = serde_json::from_str(&self.hide(&input.to_string())).unwrap_or(Value::Null);
                    }
                    list.clone()
                };
                shown(&list);
            }
            Event::Denied { id, why } => {
                let list = {
                    let mut list = steps.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some((at, _)) = began.get(&id)
                        && let Some(step) = list.get_mut(*at)
                    {
                        (step.state, step.result) = ("refused".into(), self.hide(&why));
                    }
                    list.clone()
                };
                shown(&list);
            }
            Event::Limit { status, resets_at, used } => providers.note_plan(def.id, account.as_deref(), &status, resets_at, used),
            Event::Started { model, billing } => {
                self.events.tell("program.started", Some(&session.id), json!({ "provider": def.id, "model": model, "billing": billing }));
            }
            Event::Retrying(why) => tracing::info!("{} is trying again: {why}", def.name),
        };

        let level = Level::from(&session.permission).unwrap_or(Level::Balanced);
        let (turn, external) = match def.id {
            "claude-code" => {
                let (external, resume) = match &known {
                    Some(k) => (k.external_id.clone(), true),
                    None => (uuid::Uuid::new_v4().to_string(), false),
                };
                let token = uuid::Uuid::new_v4().simple().to_string();
                let approve_at = self.address.get().map(|a| format!("{a}/programs/approve/{token}"));
                self.live().insert(token.clone(), live.clone());
                let effort = (session.thinking != "off").then_some(session.thinking.as_str());
                let options = programs::ClaudeCodeOptions { model: name, effort, mode: claude_code_mode(level), session: &external, resume, approve_at: approve_at.as_deref(), append: INSTRUCTIONS };
                // Demido's own copy runs with the account's own profile, and no self-update.
                let env: Vec<(&str, String)> = programs::claude_code_env().into_iter().chain(providers.program_env(def.id, account.as_deref())).collect();
                let started = providers.spawner().spawn(&program, &programs::claude_code_args(&options), &cwd, &env);
                let turn = match started {
                    Ok(started) => programs::claude_code(started, &prompt, &running.stop, &mut on).await,
                    Err(e) => Err(e),
                };
                self.live().remove(&token);
                (turn, external)
            }
            _ => {
                // What reaches Demido is what the rules for the level (`programs::opencode_env`) or the person's
                // own rules say to ask: the person is asked, unless the level asks nothing.
                let approve: &programs::Approver<'_> = &|a: Approval| {
                    let live = live.clone();
                    Box::pin(async move {
                        let what = format!("OpenCode wants to {} {}", verb_of(&a.kind), a.title);
                        let why = "OpenCode asks before this, by the rules Demido gave it for this session's level and your own.";
                        let yes = self.approve(&live, &a.name, reach_of(&a.kind), what, a.input, why).await;
                        if yes { Choice::Once } else { Choice::Reject }
                    })
                };
                let resume = known.as_ref().map(|k| k.external_id.as_str());
                let env: Vec<(&str, String)> = programs::opencode_env(level).into_iter().chain(providers.program_env(def.id, None)).collect();
                let started = providers.spawner().spawn(&program, &["acp".to_string()], &cwd, &env);
                let turn = match started {
                    Ok(started) => programs::opencode(started, &cwd, name, resume, Some(INSTRUCTIONS), &prompt, &running.stop, &mut on, approve).await,
                    Err(e) => Err(e),
                };
                let external = turn.as_ref().ok().and_then(|t| t.session.clone()).or_else(|| resume.map(str::to_string)).unwrap_or_default();
                (turn, external)
            }
        };
        drop(on);
        let took_ms = started_at.elapsed().as_millis() as u64;
        // Whatever the program did in the folder is one change, to be undone in one go.
        if let Some((id, place)) = saved {
            self.keep_what_changed(session, answer_id, &id, &place, &steps).await?;
        }
        let turn = turn?;
        // What it said it cost. OpenCode says what its whole session has cost so far.
        let before = known.as_ref().map_or(0.0, |k| k.cost_so_far);
        let (cost, total) = match def.id {
            "opencode" => {
                let total = turn.cost_usd.unwrap_or(before);
                ((total - before).max(0.0), total)
            }
            _ => (turn.cost_usd.unwrap_or(0.0), before + turn.cost_usd.unwrap_or(0.0)),
        };
        providers.record(def.id, name, For { session: Some(&session.id), message: Some(answer_id), account: account.as_deref() }, turn.tokens, cost)?;
        let external = turn.session.clone().unwrap_or(external);
        if !external.is_empty() {
            let kept = ProgramSession { external_id: external, seen_seq: answer_seq, cost_so_far: total, account: account.clone().unwrap_or_default() };
            self.sessions.set_program_session(&session.id, def.id, &kept)?;
        }
        if let Some(why) = turn.error.clone() {
            return Err(Error::Other(why));
        }
        // What it wrote is the answer. When it streamed nothing, its final words are.
        if content.trim().is_empty()
            && let Some(result) = &turn.result
        {
            content = result.clone();
            heard(Piece::Text(result));
        }
        // No speed: the turn's time holds the program's own work and the person's answers, not only writing.
        let t = turn.tokens;
        let timings = crate::sessions::Timings { reused: t.cached, read: t.input + t.cache_written, written: t.output, per_second: 0.0, took_ms, ..Default::default() };
        Ok(Answer { content, thought, timings, stopped: turn.stopped || running.stop.load(Ordering::Relaxed), ..Answer::default() })
    }

    /// Compares the folder with how it was saved before the turn: what changed becomes one change of the
    /// answer, with its diff, shown as a last step. Nothing changed: the saved copy goes.
    async fn keep_what_changed(&self, session: &Session, answer_id: &str, id: &str, place: &std::path::Path, steps: &Arc<Mutex<Vec<Step>>>) -> Result<()> {
        let Some(folder) = session.folder.clone() else { return Ok(()) };
        let saved_at = place.to_path_buf();
        let found = tokio::task::spawn_blocking(move || crate::checkpoint::compared(std::path::Path::new(&folder), &saved_at)).await;
        let changed = match found {
            Ok(Ok(changed)) => changed,
            _ => return Ok(()),
        };
        if changed.is_empty() {
            let _ = std::fs::remove_dir_all(place);
            return Ok(());
        }
        let shown = crate::tools::diff::of_files(&changed);
        let change = self.sessions.add_folder_change(&session.id, answer_id, id, &place.to_string_lossy())?;
        self.sessions.set_diff(id, &shown.text, &shown.stat())?;
        for old in self.sessions.old_checkpoints(&session.id, super::SAVED_FOLDERS_KEPT)? {
            let _ = std::fs::remove_dir_all(old);
        }
        let list = {
            let mut list = steps.lock().unwrap_or_else(|e| e.into_inner());
            list.push(Step { tool: crate::tools::ALL_IT_CHANGED.into(), args: json!({}), result: shown.stat(), state: "done".into(), change: Some(change.id), ..Step::default() });
            list.clone()
        };
        self.sessions.set_steps(answer_id, &list)?;
        self.events.tell("message.steps", Some(&session.id), json!({ "message": answer_id, "steps": list }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(role: &str, content: &str) -> Message {
        Message {
            id: "m".into(),
            session_id: "s".into(),
            seq: 1,
            role: role.into(),
            content: content.into(),
            thought: None,
            state: "done".into(),
            problem: None,
            model: None,
            timings: None,
            steps: vec![],
            created_at: 0,
        }
    }

    #[test]
    fn a_program_is_given_what_it_has_not_seen_and_then_the_request() {
        assert_eq!(prompt_from(&[], "Add milk."), "Add milk.");
        let earlier = [said("user", "What is 2 + 2?"), said("assistant", "4."), said("assistant", "")];
        assert_eq!(
            prompt_from(&earlier, "And times 3?"),
            "Earlier in this conversation, which you did not see (another model, or another account, answered it):\n\nPerson: What is 2 + 2?\n\nModel: 4.\n\nThe person now says:\n\nAnd times 3?"
        );
    }

    #[test]
    fn each_level_is_a_claude_code_mode_and_each_tool_a_reach() {
        assert_eq!([Level::Cautious, Level::Balanced, Level::Smart, Level::Autonomous].map(claude_code_mode), ["manual", "acceptEdits", "auto", "bypassPermissions"]);
        assert_eq!(
            [reach_of("Bash"), reach_of("Edit"), reach_of("Read"), reach_of("WebFetch"), reach_of("execute"), reach_of("edit")],
            [Reach::Runs, Reach::Changes, Reach::Reads, Reach::Network, Reach::Runs, Reach::Changes]
        );
        assert_eq!(claude_code_what("WebSearch", &json!({ "query": "heron counts" })), "Claude Code wants to search the internet for \"heron counts\"");
    }
}
