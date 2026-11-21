//! What a turn does before its answer is written: tools, and work split into tasks.
//!
//! The shape was measured before it was built (research note 13, `core/dev/tool-turn-trial.py`), on four
//! small models:
//!
//! - **Deciding and answering are two questions.** First the model is asked only "which tool, or none", and
//!   replies with a few tokens of JSON. Then it writes its answer as ordinary text. Asked to do both in one
//!   JSON reply, small models lose answers; and with the decision apart, the request for the answer is
//!   exactly the request of a plain chat, so having tools cannot make answers worse.
//! - **The decision is not kept in the conversation.** Kept, the smallest model starts answering in JSON.
//!   What is kept is what the conversation needs: the call that was made, and its result.
//! - **The tools are listed with the request, not in the first message.** The first message then never
//!   changes, whatever tools are in view, and the runtime never reads the conversation twice.
//! - **A result is material, and what to do with it comes after it** (DECISIONS L13).
//! - **The question does not take a tool for granted.** "Which tool does the request need?" made one family
//!   of models search the web to say hello. "Does it need a tool? Most do not." No wording was best for
//!   every model; this one errs towards not acting, which is the cheaper mistake.
//! - **Models do not plan by themselves.** Offered `plan` as a tool, the small ones never chose it. Asked
//!   directly for the steps, they list them. So a plan is asked for directly, when the person wants one,
//!   and Demido keeps the list and walks it: the model is only ever asked to do the task in hand.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde_json::{Value, json};

use super::Chat;
use crate::Result;
use crate::sessions::{Session, Task};
use crate::tools::{self, Asked, Step, Tool};

const REPLY: &str = "Reply with one JSON object, nothing else: {\"tool\": \"name\", \"args\": {...}}";
/// The steps are asked for as instructions. Asked only to "list the steps", models wrote each step with its
/// sum already worked out, wrongly, and the later steps then trusted the title over the tool.
const ASK_STEPS: &str = "List the steps needed to do the request, as short instructions. Do not do them yet. Reply with one JSON object: {\"steps\": [\"...\"]}";

/// No more uses of tools than this for one request, or for one task. A model that wants more is going round.
const MOST_TOOLS_IN_A_ROW: usize = 6;
/// What a task gave is written in no more than this many tokens.
const A_TASK_GIVES_AT_MOST: u32 = 350;
/// In the list a model is shown, what an earlier task gave is cut to this many characters.
const SHOWN_OF_A_RESULT: usize = 400;

/// The few words that differ between working on the person's request and working on one task of it.
pub(super) struct Words {
    /// Asked after the request: which tool it needs.
    ask: String,
    /// Asked after a tool's result: which tool is needed next.
    ask_next: String,
    /// Said after the last result, where it stays in the conversation.
    use_result: String,
}

impl Words {
    pub(super) fn request() -> Self {
        Self {
            ask: format!("Does the request need a tool? Most do not. {REPLY}"),
            ask_next: format!("Above is the tool's result. Is another tool needed? Mostly not. {REPLY}"),
            use_result: "Above is the tool's result. Answer the request with it.".into(),
        }
    }
    fn task(label: &str) -> Self {
        Self {
            ask: format!("Does task {label} need a tool? Most do not. {REPLY}"),
            ask_next: format!("Above is the tool's result. Does task {label} need another tool? Mostly not. {REPLY}"),
            use_result: format!("Above is the tool's result. Write the result of task {label}, in one or two sentences."),
        }
    }
}

/// The list of tools and the question a model is asked before each answer, for a person to read.
pub(super) fn told_about_tools() -> String {
    format!("{}\n\n(the request)\n\n{}", tools::listed(&tools::all()), Words::request().ask)
}

/// How the last message of a piece of work ends.
pub(super) enum End<'a> {
    /// With the tools in view and the question of which one is needed. Never kept.
    Decide(&'a str),
    /// As it stays in the conversation.
    Answer,
}

fn user(text: impl Into<String>) -> Value {
    json!({ "role": "user", "content": text.into() })
}
fn assistant(text: impl Into<String>) -> Value {
    json!({ "role": "assistant", "content": text.into() })
}

/// What a step gave, as the model is told it.
fn told(step: &Step) -> String {
    match step.state.as_str() {
        "refused" => format!("The tool was not run: {}.", step.result),
        "failed" => format!("The tool could not do it: {}.", step.result),
        _ => step.result.clone(),
    }
}

/// The tools used for a request, as messages after it: each call, then its result fenced off as material.
pub(super) fn after_request(steps: &[Step], end: &End, words: &Words) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        out.push(assistant(step.call()));
        let result = format!("<result>\n{}\n</result>", told(step));
        out.push(user(match (i + 1 < steps.len(), end) {
            (true, _) => result,
            (false, End::Decide(tools)) => format!("{tools}\n\n{result}\n\n{}", words.ask_next),
            (false, End::Answer) => format!("{result}\n\n{}", words.use_result),
        }));
    }
    out
}

/// A request and the tools used for it so far, as messages.
pub(super) fn request_messages(request: &str, steps: &[Step], end: &End, words: &Words) -> Vec<Value> {
    // A step that is still running has given nothing yet.
    let steps: Vec<Step> = steps.iter().filter(|s| s.state != "running").cloned().collect();
    if steps.is_empty() {
        return vec![user(match end {
            End::Decide(tools) => format!("{tools}\n\n{request}\n\n{}", words.ask),
            End::Answer => request.to_string(),
        })];
    }
    let mut out = vec![user(request)];
    out.extend(after_request(&steps, end, words));
    out
}

/// Where a text that has started repeating itself should end: after the first of the repeats. `None` when
/// it is not repeating.
///
/// A small model that is stuck writes the same line again and again until its room is full: tried on a
/// real one, a sum that failed became 7,900 tokens of one line. A text counts as going round when its end
/// is the same piece four times over (six for a short piece) and the piece is more than decoration.
pub(super) fn going_round(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    for piece in 12..=400 {
        let times = if piece < 40 { 6 } else { 4 };
        if n < piece * times {
            break;
        }
        let from = n - piece * times;
        if !(from..n - piece).all(|i| chars[i].1 == chars[i + piece].1) {
            continue;
        }
        // A rule of dashes or the edge of a table repeats, and is not a model going round.
        let mut kinds: Vec<char> = chars[from..from + piece].iter().map(|c| c.1).collect();
        kinds.sort_unstable();
        kinds.dedup();
        if kinds.len() < 6 {
            continue;
        }
        // Back to where the repeating began, and keep the first of the repeats.
        let mut began = from;
        while began > 0 && chars[began - 1].1 == chars[began - 1 + piece].1 {
            began -= 1;
        }
        return Some(chars[began + piece].0);
    }
    None
}

/// The numbers written in a text: `3.5`, `2,340`, `38`.
fn numbers_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut number = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || ((c == '.' || c == ',') && !number.is_empty()) {
            number.push(c);
        } else if !number.is_empty() {
            found.push(std::mem::take(&mut number).trim_end_matches(['.', ',']).replace(',', ""));
        }
    }
    found
}

/// Whether a step of a plan holds a number that the request does not: the sign that the model did the
/// step while it was listing it. Tried on real models, such steps came back with their sums worked out,
/// wrongly, and what was written later trusted the step's title over the calculator.
fn done_already(step: &str, request: &str) -> bool {
    let given = numbers_in(request);
    // A lone digit is a step's own number, or "step 1", more often than a result.
    numbers_in(step).iter().any(|n| n.len() > 1 && !given.contains(n))
}

/// A step as an instruction only: cut before the part that holds a result.
fn as_instruction(step: &str, request: &str) -> String {
    if !done_already(step, request) {
        return step.trim().to_string();
    }
    // "Calculate the pay: 9.75 h × 38 = 371.5 euros" is "Calculate the pay".
    for (at, c) in step.char_indices() {
        if (c == ':' || c == '=') && !step[..at].trim().is_empty() && !done_already(&step[..at], request) {
            return step[..at].trim().to_string();
        }
    }
    step.trim().to_string()
}

fn one_line(text: &str, at_most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= at_most { flat } else { format!("{}…", flat.chars().take(at_most).collect::<String>()) }
}

/// A task's number as the model and the person see it: `2`, or `2.1` for a sub-task.
fn label(tasks: &[Task], task: &Task) -> String {
    match &task.parent_id {
        None => task.position.to_string(),
        Some(parent) => format!("{}.{}", tasks.iter().find(|t| &t.id == parent).map_or(0, |t| t.position), task.position),
    }
}

/// The list of tasks as the model is shown it: what is done and what it gave, the one in hand, what is left.
fn listing(tasks: &[Task], now: Option<&Task>, result_at_most: usize) -> String {
    let mut lines = Vec::new();
    for task in tasks {
        let mark = match task.state.as_str() {
            _ if now.is_some_and(|n| n.id == task.id) => "now",
            "done" => "done",
            "failed" => "failed",
            // A task whose sub-tasks are being done.
            "running" => "now",
            _ => " ",
        };
        let gave = task.result.as_deref().filter(|r| !r.trim().is_empty()).map(|r| format!(": {}", one_line(r, result_at_most))).unwrap_or_default();
        lines.push(format!("{}{}. [{mark}] {}{gave}", if task.parent_id.is_some() { "   " } else { "" }, label(tasks, task), task.title));
    }
    lines.join("\n")
}

/// What a plan gave when all of it is done: each task of the first level and its result. This is what the
/// answer is written from, and what stays in the conversation in place of everything the tasks did.
pub(super) fn outcome_of(tasks: &[Task]) -> String {
    tasks
        .iter()
        .filter(|t| t.parent_id.is_none())
        .map(|t| {
            let gave = match (t.state.as_str(), t.result.as_deref().map(str::trim).filter(|r| !r.is_empty())) {
                ("done", Some(result)) => result.to_string(),
                ("failed", Some(why)) => format!("(failed) {why}"),
                _ => "(not done)".into(),
            };
            format!("{}. {}: {gave}", t.position, t.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The request with the task list under it, and what to do now.
fn task_text(request: &str, tasks: &[Task], now: &Task, instruction: &str) -> String {
    format!("{request}\n\n<tasks>\n{}\n</tasks>\n\n{instruction}", listing(tasks, Some(now), SHOWN_OF_A_RESULT))
}

impl Chat {
    /// Asks the model something the person does not see being written: which tool, which steps, what a task
    /// gave. Thinking is off for these: measured, it costs seconds here and changes nothing.
    async fn quietly(&self, session: &Session, messages: Vec<Value>, shape: Option<Value>, at_most: u32, stop: &AtomicBool) -> Result<String> {
        let mut body = json!({
            "model": session.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
            "max_tokens": at_most,
            "chat_template_kwargs": { "enable_thinking": false },
            "reasoning_effort": "none",
        });
        if let Some(shape) = shape {
            // A decision is the same every time it is asked.
            body["temperature"] = json!(0);
            body["response_format"] = json!({ "type": "json_schema", "schema": shape });
        }
        Ok(self.engine.answer(&session.model, body, stop, &mut |_| {}).await?.content)
    }

    /// Asks which tool is needed, uses it, and asks again, until the model needs none. `Some` when it asked
    /// for a plan: the steps it listed. `changed` is called whenever `steps` has something new to show.
    #[allow(clippy::too_many_arguments)]
    async fn use_tools(
        &self,
        session: &Session,
        before: &[Value],
        request: &str,
        words: &Words,
        offered: &[&Tool],
        steps: &mut Vec<Step>,
        stop: &AtomicBool,
        changed: &(dyn Fn(&[Step]) + Send + Sync),
    ) -> Result<Option<Vec<String>>> {
        let listed = tools::listed(offered);
        for _ in 0..MOST_TOOLS_IN_A_ROW {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let mut messages = before.to_vec();
            messages.extend(request_messages(request, steps, &End::Decide(&listed), words));
            let reply = self.quietly(session, messages.clone(), Some(tools::shape(offered)), 200, stop).await?;
            let (said, asked) = match tools::read(&reply, offered) {
                Ok(asked) => (reply, Ok(asked)),
                Err(problem) if stop.load(Ordering::Relaxed) => (reply, Err(problem)),
                Err(problem) => {
                    // One more try, told what is wrong in plain words. A check cannot do more than that.
                    messages.push(assistant(if reply.trim().is_empty() { "(nothing)" } else { reply.trim() }));
                    messages.push(user(format!("That cannot be used: {problem}. Reply again with one JSON object.")));
                    let again = self.quietly(session, messages, Some(tools::shape(offered)), 200, stop).await?;
                    let asked = tools::read(&again, offered);
                    (again, asked)
                }
            };
            match asked {
                Ok(Asked::Nothing) => break,
                Ok(Asked::Tool { tool, args }) if tool == tools::PLAN => {
                    let titles: Vec<String> = tools::steps_of(&args).iter().map(|step| as_instruction(step, request)).collect();
                    // A plan of one step is no plan.
                    return Ok(if titles.len() > 1 { Some(titles) } else { None });
                }
                Ok(Asked::Tool { tool, args }) => {
                    let args = Value::Object(args);
                    // Asking for the same thing again means the model is going round. It has what the tool gives.
                    if steps.iter().any(|s| s.tool == tool && s.args == args) {
                        break;
                    }
                    steps.push(Step { tool: tool.clone(), args: args.clone(), result: String::new(), state: "running".into(), took_ms: 0 });
                    changed(steps);
                    let started = Instant::now();
                    let outcome = tools::run(&tool, args.as_object().expect("made as one"), self.clock);
                    let step = steps.last_mut().expect("just pushed");
                    step.took_ms = started.elapsed().as_millis() as u64;
                    (step.state, step.result) = match outcome {
                        Ok(result) => ("done".into(), result),
                        Err(why) => ("failed".into(), why),
                    };
                    let (state, took_ms) = (step.state.clone(), step.took_ms);
                    changed(steps);
                    self.events.append("tool.used", Some(&session.id), json!({ "tool": tool, "args": args, "state": state, "took_ms": took_ms }))?;
                }
                Err(problem) => {
                    // Still not usable after one repair. Nothing is run; the model goes on without, and is told why.
                    let tried: Value = serde_json::from_str(said.trim()).unwrap_or_else(|_| json!({ "said": said.trim() }));
                    let tool = tried.get("tool").and_then(Value::as_str).unwrap_or("?").to_string();
                    let args = tried.get("args").cloned().unwrap_or_else(|| tried.clone());
                    self.events.append("tool.refused", Some(&session.id), json!({ "tool": tool, "args": args, "problem": problem }))?;
                    steps.push(Step { tool, args, result: problem, state: "refused".into(), took_ms: 0 });
                    changed(steps);
                    break;
                }
            }
        }
        Ok(None)
    }

    /// Asks the model directly for the steps of a request. `Some` when it listed more than one. `at_least`
    /// is 2 when the person asked for a plan: then there is to be one.
    async fn split(&self, session: &Session, before: &[Value], request: &str, at_least: usize, stop: &AtomicBool) -> Result<Option<Vec<String>>> {
        let mut messages = before.to_vec();
        // Without the tools in view: with them, steps come back written as calls ("calculate(3.5 + 2.25)").
        messages.push(user(format!("{request}\n\n{ASK_STEPS}")));
        let shape = json!({ "type": "object", "properties": { "steps": { "type": "array", "items": { "type": "string" }, "minItems": at_least, "maxItems": tools::MOST_STEPS_IN_A_PLAN } }, "required": ["steps"] });
        let listed = |reply: &str| -> Vec<String> {
            let steps = serde_json::from_str::<Value>(reply.trim()).ok().and_then(|v| v.as_object().map(tools::steps_of)).unwrap_or_default();
            steps.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        };
        let reply = self.quietly(session, messages.clone(), Some(shape.clone()), 300, stop).await?;
        let mut steps = listed(&reply);
        // Steps are instructions. One that already holds its result is handed back once; then it is cut.
        if steps.iter().any(|step| done_already(step, request)) && !stop.load(Ordering::Relaxed) {
            messages.push(assistant(reply.trim()));
            messages.push(user("The steps must be instructions only, with no results and no new numbers in them. Reply again with one JSON object."));
            let again = listed(&self.quietly(session, messages, Some(shape), 300, stop).await?);
            if again.len() >= steps.len().min(2) {
                steps = again;
            }
        }
        let steps: Vec<String> = steps.iter().map(|step| as_instruction(step, request)).collect();
        Ok(if steps.len() > 1 { Some(steps) } else { None })
    }

    fn tell_tasks(&self, session: &Session, answer_id: &str) {
        if let Ok(tasks) = self.sessions.tasks_of(answer_id) {
            self.events.tell("message.tasks", Some(&session.id), json!({ "message": answer_id, "tasks": tasks }));
        }
    }

    /// Does one task that has no parts: asks which tool it needs, uses it, and has the model write what the
    /// task gave. `Err` with the steps of a plan when the model asked to split it (only where `offered` has `plan`).
    #[allow(clippy::too_many_arguments)]
    async fn do_one(&self, session: &Session, answer_id: &str, before: &[Value], request: &str, task: &Task, offered: &[&Tool], stop: &AtomicBool) -> Result<std::result::Result<(), Vec<String>>> {
        self.sessions.set_task(&task.id, "running", None, None)?;
        self.tell_tasks(session, answer_id);
        let tasks = self.sessions.tasks_of(answer_id)?;
        let name = label(&tasks, task);
        let (words, text) = (Words::task(&name), task_text(request, &tasks, task, &format!("Do task {name} only: {}", task.title)));
        let mut steps = Vec::new();
        let shown = |steps: &[Step]| {
            let _ = self.sessions.set_task(&task.id, "running", None, Some(steps));
            self.tell_tasks(session, answer_id);
        };
        if let Some(parts) = self.use_tools(session, before, &text, &words, offered, &mut steps, stop, &shown).await? {
            return Ok(Err(parts));
        }
        let mut messages = before.to_vec();
        messages.extend(request_messages(&text, &steps, &End::Answer, &words));
        let gave = self.quietly(session, messages, None, A_TASK_GIVES_AT_MOST, stop).await?;
        self.settle(session, answer_id, task, &gave, Some(&steps), stop)?;
        Ok(Ok(()))
    }

    /// Writes down how a task ended.
    fn settle(&self, session: &Session, answer_id: &str, task: &Task, gave: &str, steps: Option<&[Step]>, stop: &AtomicBool) -> Result<()> {
        let (state, result) = match (stop.load(Ordering::Relaxed), gave.trim()) {
            (true, _) => ("stopped", None),
            (false, "") => ("failed", Some("The model wrote nothing for this task.")),
            (false, gave) => ("done", Some(gave)),
        };
        self.sessions.set_task(&task.id, state, result, steps)?;
        self.tell_tasks(session, answer_id);
        Ok(())
    }

    /// Walks a plan: each task in turn, and inside a task that the model split, each of its parts. The model
    /// is only ever asked to do the one in hand; Demido keeps the list.
    async fn carry_out(&self, session: &Session, answer_id: &str, before: &[Value], request: &str, titles: &[String], stop: &AtomicBool) -> Result<()> {
        let tasks = self.sessions.add_tasks(&session.id, answer_id, None, titles)?;
        self.tell_tasks(session, answer_id);
        for task in &tasks {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Err(parts) = self.do_one(session, answer_id, before, request, task, &tools::all(), stop).await? else {
                continue;
            };
            // The model split this task. Its parts are done one by one; a part is not split again.
            for part in self.sessions.add_tasks(&session.id, answer_id, Some(&task.id), &parts)? {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                // `plan` is not offered, so this cannot ask to split.
                let _ = self.do_one(session, answer_id, before, request, &part, &tools::without_plan(), stop).await?;
            }
            // What the whole task gave, from what its parts gave.
            let all = self.sessions.tasks_of(answer_id)?;
            let name = label(&all, task);
            let mut messages = before.to_vec();
            messages.push(user(task_text(request, &all, task, &format!("All parts of task {name} are done. Write the result of task {name}, in one or two sentences."))));
            let gave = self.quietly(session, messages, None, A_TASK_GIVES_AT_MOST, stop).await?;
            self.settle(session, answer_id, task, &gave, None, stop)?;
        }
        Ok(())
    }

    /// Everything a turn does before its answer is written. What it did is kept with the answer (its steps,
    /// its tasks), and the answer is then asked for with that in view.
    pub(super) async fn work(&self, session: &Session, answer_id: &str, plan_first: bool, stop: &AtomicBool) -> Result<()> {
        let (before, request) = self.before(session, answer_id)?;
        let mut steps: Vec<Step> = Vec::new();
        let shown = |steps: &[Step]| {
            let _ = self.sessions.set_steps(answer_id, steps);
            self.events.tell("message.steps", Some(&session.id), json!({ "message": answer_id, "steps": steps }));
        };
        // A plan is asked for directly when the person wants one. Left to itself, a small model does not plan.
        let always = self.settings.value::<String>("tools.plan").is_ok_and(|v| v == "always");
        let mut planned = if plan_first || always { self.split(session, &before, &request, if plan_first { 2 } else { 1 }, stop).await? } else { None };
        if planned.is_none() {
            planned = self.use_tools(session, &before, &request, &Words::request(), &tools::all(), &mut steps, stop, &shown).await?;
        }
        let Some(titles) = planned else { return Ok(()) };
        let started = Instant::now();
        steps.push(Step { tool: tools::PLAN.into(), args: json!({ "steps": titles }), result: String::new(), state: "running".into(), took_ms: 0 });
        shown(&steps);
        let walked = self.carry_out(session, answer_id, &before, &request, &titles, stop).await;
        // However the walk ended, what was done is written down. A task left running is not running any more.
        let stopped = stop.load(Ordering::Relaxed);
        self.sessions.end_tasks(answer_id, if walked.is_err() { "failed" } else { "stopped" })?;
        self.tell_tasks(session, answer_id);
        let plan = steps.last_mut().expect("just pushed");
        plan.result = outcome_of(&self.sessions.tasks_of(answer_id)?);
        plan.state = if walked.is_err() { "failed" } else { "done" }.into();
        plan.took_ms = started.elapsed().as_millis() as u64;
        shown(&steps);
        self.events.append("tool.used", Some(&session.id), json!({ "tool": tools::PLAN, "args": { "steps": titles }, "state": if stopped { "stopped" } else { &steps.last().expect("there").state }, "took_ms": steps.last().expect("there").took_ms }))?;
        walked
    }
}
