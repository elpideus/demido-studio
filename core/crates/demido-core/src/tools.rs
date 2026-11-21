//! Tools: things a model can ask Demido to do for it.
//!
//! How a model is asked was measured before it was built (research note 13, `core/dev/tool-call-trial.py`
//! and `core/dev/tool-turn-trial.py`):
//!
//! - The list is a few short lines, and "no tool" is one of the tools. Without that entry, small models use
//!   a tool for everything.
//! - Each description says what the tool does and what the model cannot do without it. Small models do
//!   not know their own limits: told only "exact arithmetic", they work sums out themselves, and get them wrong.
//! - No example of use. An example is copied: it came back as the arguments of unrelated requests.
//! - The model's reply is one JSON object that names the tool. Only the name is held to the list; the
//!   arguments are checked here afterwards, mended where the meaning is plain, and otherwise handed back
//!   to the model once, with what is wrong in plain words.
//!
//! The date is not a tool: Demido tells every model what day it is, outright (`chat`). The time of day
//! is a tool, because it changes while a conversation goes on. Writing the time on every message was tried
//! and taken out again: one model went on in the style of a chat log, another saw the digits and reached
//! for the calculator.
//!
//! The tools in this file touch nothing outside Demido, so they need no permission. Tools that read
//! files, run commands or use the network come with the permission levels (plan item 5), and plug in here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;

mod calculate;

/// The entry that means "no tool is needed".
pub const NONE: &str = "none";
/// The tool that splits work into steps. It is carried out by `chat`, which owns the task list.
pub const PLAN: &str = "plan";

/// What an argument may be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Text,
    Whole,
    /// A list of short texts.
    List,
}

#[derive(Clone, Copy, Debug)]
pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Tool {
    pub name: &'static str,
    /// What it does, and what the model cannot do without it. One or two short sentences.
    pub does: &'static str,
    pub args: &'static [Arg],
}

const TOOLS: &[Tool] = &[
    Tool { name: "time", does: "The time now, to the minute. You do not know it without it.", args: &[] },
    Tool {
        name: "calculate",
        does: "Exact arithmetic, and the days between two dates, or two times of day. Use it for every sum; do not work sums out yourself.",
        args: &[Arg { name: "expression", kind: Kind::Text }],
    },
    Tool { name: PLAN, does: "For work that needs several steps: write the steps as a list first.", args: &[Arg { name: "steps", kind: Kind::List }] },
];

/// The most steps a plan may have. More than a small model keeps apart.
pub const MOST_STEPS_IN_A_PLAN: usize = 8;

/// Every tool there is.
pub fn all() -> Vec<&'static Tool> {
    TOOLS.iter().collect()
}

/// The tools without the one that plans: for work that is already one step of a plan.
pub fn without_plan() -> Vec<&'static Tool> {
    TOOLS.iter().filter(|t| t.name != PLAN).collect()
}

/// The tools as the model is shown them: a line each, and "no tool" as the last of them.
pub fn listed(tools: &[&Tool]) -> String {
    let mut out = String::from("Tools you can ask for:\n");
    for t in tools {
        let args: Vec<String> = t
            .args
            .iter()
            .map(|a| {
                format!(
                    "{}: {}",
                    a.name,
                    match a.kind {
                        Kind::Text => "text",
                        Kind::Whole => "whole number",
                        Kind::List => "list of text",
                    }
                )
            })
            .collect();
        out.push_str(&format!("- {}({}): {}\n", t.name, args.join(", "), t.does));
    }
    out.push_str("- none(): No tool is needed. Answer from what you know.");
    out
}

/// The shape a reply that names a tool is held to: a JSON object whose `tool` is one from the list.
/// The arguments are left to the model and checked afterwards.
pub fn shape(tools: &[&Tool]) -> Value {
    let mut names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    names.push(NONE);
    json!({ "type": "object", "properties": { "tool": { "enum": names }, "args": { "type": "object" } }, "required": ["tool", "args"] })
}

/// One use of a tool: what was asked for, and what came of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Step {
    pub tool: String,
    #[schema(value_type = Object)]
    pub args: Value,
    /// What the tool gave back, or why it gave nothing.
    pub result: String,
    /// `running`, `done`, `failed` (the tool ran and could not do it) or `refused` (the request could not
    /// be used, so nothing ran).
    pub state: String,
    pub took_ms: u64,
}

impl Step {
    /// The request as the model made it, for the conversation it is part of.
    pub fn call(&self) -> String {
        json!({ "tool": self.tool, "args": self.args }).to_string()
    }
}

/// What a model's reply asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    /// No tool: it will answer from what it knows.
    Nothing,
    Tool {
        tool: String,
        args: Map<String, Value>,
    },
}

/// The first JSON object in a reply, with words or code fences around it let be.
fn first_object(text: &str) -> Option<Map<String, Value>> {
    let bytes = text.as_bytes();
    let mut start = text.find('{');
    while let Some(from) = start {
        let (mut depth, mut in_text, mut escaped) = (0i32, false, false);
        for (i, b) in bytes.iter().enumerate().skip(from) {
            match (in_text, *b) {
                (true, _) if escaped => escaped = false,
                (true, b'\\') => escaped = true,
                (true, b'"') => in_text = false,
                (false, b'"') => in_text = true,
                (false, b'{') => depth += 1,
                (false, b'}') => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(Value::Object(found)) = serde_json::from_str(&text[from..=i]) {
                            return Some(found);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
        start = text[from + 1..].find('{').map(|at| at + from + 1);
    }
    None
}

/// Reads a model's reply as a request for a tool. `Err` says what is wrong with it, in words the model
/// can act on.
///
/// Mended without asking, because the meaning is plain: `time()` for `time`; a number written as text;
/// one argument under another name when the tool takes exactly one; arguments the tool does not have.
pub fn read(reply: &str, tools: &[&Tool]) -> std::result::Result<Asked, String> {
    let Some(said) = first_object(reply) else {
        return Err("the reply is not a JSON object".into());
    };
    let named = match said.get("tool") {
        None | Some(Value::Null) => return Ok(Asked::Nothing),
        Some(Value::String(name)) => name.trim().to_lowercase(),
        Some(other) => return Err(format!("\"tool\" must be a tool's name, not {other}")),
    };
    // "time()" and "calculate(expression)" are the names as the list shows them.
    let named = named.split('(').next().unwrap_or_default().trim().to_string();
    if named.is_empty() || named == NONE {
        return Ok(Asked::Nothing);
    }
    let Some(tool) = tools.iter().find(|t| t.name == named) else {
        return Err(format!("there is no tool called \"{named}\". The tools are: {}, {NONE}", tools.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")));
    };
    let given = match said.get("args") {
        // {"calculate": {"expression": "..."}}: the arguments one level too deep, under the tool's own name.
        Some(Value::Object(args)) if args.len() == 1 && args.get(tool.name).is_some_and(Value::is_object) => args[tool.name].as_object().expect("checked").clone(),
        Some(Value::Object(args)) => args.clone(),
        None | Some(Value::Null) => Map::new(),
        // One bare value for a tool that takes one argument.
        Some(bare) if tool.args.len() == 1 => Map::from_iter([(tool.args[0].name.to_string(), bare.clone())]),
        Some(_) => return Err("\"args\" must be an object".into()),
    };
    let mut args = Map::new();
    for arg in tool.args {
        let value = match given.get(arg.name) {
            Some(v) => v,
            None if tool.args.len() == 1 && given.len() == 1 => given.values().next().expect("one"),
            None => return Err(format!("{} needs \"{}\"", tool.name, arg.name)),
        };
        args.insert(arg.name.to_string(), fitted(tool, arg, value)?);
    }
    Ok(Asked::Tool { tool: tool.name.to_string(), args })
}

/// An argument's value as the tool takes it, or what is wrong with it.
fn fitted(tool: &Tool, arg: &Arg, value: &Value) -> std::result::Result<Value, String> {
    match (arg.kind, value) {
        (Kind::Text, Value::String(s)) if !s.trim().is_empty() => Ok(json!(s.trim())),
        (Kind::Text, Value::Number(n)) => Ok(json!(n.to_string())),
        (Kind::Text, _) => Err(format!("\"{}\" of {} must be text, and not empty", arg.name, tool.name)),
        (Kind::Whole, Value::Number(n)) if n.as_i64().is_some() => Ok(value.clone()),
        (Kind::Whole, Value::Number(n)) if n.as_f64().is_some_and(|f| f.fract() == 0.0) => Ok(json!(n.as_f64().expect("checked") as i64)),
        (Kind::Whole, Value::String(s)) if s.trim().parse::<i64>().is_ok() => Ok(json!(s.trim().parse::<i64>().expect("checked"))),
        (Kind::Whole, _) => Err(format!("\"{}\" of {} must be a whole number, not {value}", arg.name, tool.name)),
        (Kind::List, Value::Array(items)) => {
            let texts: Vec<String> = items.iter().filter_map(|i| i.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect();
            if texts.is_empty() { Err(format!("\"{}\" of {} must be a list of texts, and not empty", arg.name, tool.name)) } else { Ok(json!(texts)) }
        }
        // A list written as lines of one text.
        (Kind::List, Value::String(s)) if s.lines().filter(|l| !l.trim().is_empty()).count() > 1 => Ok(json!(
            s.lines()
                .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')').trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        )),
        (Kind::List, _) => Err(format!("\"{}\" of {} must be a list of texts", arg.name, tool.name)),
    }
}

/// The steps of a plan, from the arguments of a checked request for `plan`: trimmed, and no more than a
/// small model keeps apart.
pub fn steps_of(args: &Map<String, Value>) -> Vec<String> {
    args.get("steps")
        .and_then(Value::as_array)
        .map(|steps| steps.iter().filter_map(Value::as_str).map(|s| s.chars().take(200).collect::<String>()).take(MOST_STEPS_IN_A_PLAN).collect())
        .unwrap_or_default()
}

/// A moment as a person would say it, and its date for sums.
#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    /// `Saturday, 21 November 2026`
    pub day: String,
    /// `14:05`
    pub time: String,
    /// The year, the month and the day.
    pub date: (i64, i64, i64),
}

/// What day and time a moment is here. The moment is in milliseconds since 1970. A test gives its own.
pub type Clock = fn(i64) -> Moment;

/// A moment in this computer's own time zone.
pub fn here(at_ms: i64) -> Moment {
    let moment = jiff::Timestamp::from_millisecond(at_ms).unwrap_or_else(|_| jiff::Timestamp::now()).to_zoned_datetime(jiff::tz::TimeZone::system());
    Moment { day: moment.strftime("%A, %-d %B %Y").to_string(), time: moment.strftime("%H:%M").to_string(), date: (moment.year() as i64, moment.month() as i64, moment.day() as i64) }
}

/// Does what a checked request asks. `Err` is why the tool could not do it, in plain words.
/// `plan` is not done here: `chat` carries it out.
pub fn run(tool: &str, args: &Map<String, Value>, clock: Clock) -> std::result::Result<String, String> {
    match tool {
        "time" => {
            let now = clock(crate::now_ms());
            Ok(format!("{}, {}", now.time, now.day))
        }
        "calculate" => {
            let now = clock(crate::now_ms());
            let minutes = now.time.split_once(':').and_then(|(h, m)| Some(h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?));
            calculate::work_out(args.get("expression").and_then(Value::as_str).unwrap_or_default(), Some(now.date), minutes)
        }
        other => Err(format!("{other} cannot be run here")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(reply: &str) -> std::result::Result<Asked, String> {
        read(reply, &all())
    }
    fn tool(name: &str, args: Value) -> Asked {
        Asked::Tool { tool: name.into(), args: args.as_object().unwrap().clone() }
    }

    #[test]
    fn the_list_is_short_and_no_tool_is_one_of_the_tools() {
        let list = listed(&all());
        assert!(list.starts_with("Tools you can ask for:\n- time(): The time now, to the minute."));
        assert!(list.contains("- calculate(expression: text): Exact arithmetic, and the days between two dates, or two times of day."));
        assert!(list.ends_with("- none(): No tool is needed. Answer from what you know."));
        assert!(!listed(&without_plan()).contains("plan("));
        assert_eq!(shape(&without_plan())["properties"]["tool"]["enum"], json!(["time", "calculate", "none"]));
    }

    #[test]
    fn a_reply_is_read_as_a_request_for_a_tool_or_for_none() {
        assert_eq!(asked(r#"{"tool": "none", "args": {}}"#), Ok(Asked::Nothing));
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expression": " 437 * 4 + 35 "}}"#), Ok(tool("calculate", json!({ "expression": "437 * 4 + 35" }))));
        // Words and fences around it are let be.
        assert_eq!(asked("Sure!\n```json\n{\"tool\": \"calculate\", \"args\": {\"expression\": \"1+1\"}}\n```\nThat is the tool."), Ok(tool("calculate", json!({ "expression": "1+1" }))));
        // A brace inside a text does not end the object.
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expression": "2 * (3 + 4) } "}}"#), Ok(tool("calculate", json!({ "expression": "2 * (3 + 4) }" }))));
    }

    #[test]
    fn what_is_plain_is_mended_without_asking() {
        // The name as the list shows it, in capitals, with spaces.
        assert_eq!(asked(r#"{"tool": "time()", "args": {}}"#), Ok(tool("time", json!({}))));
        assert_eq!(asked(r#"{"tool": "time", "args": {"zone": "Rome"}}"#), Ok(tool("time", json!({}))));
        assert_eq!(asked(r#"{"tool": " None() "}"#), Ok(Asked::Nothing));
        assert_eq!(asked(r#"{"tool": "Calculate(expression)", "args": {"expression": "1+1"}}"#), Ok(tool("calculate", json!({ "expression": "1+1" }))));
        // One argument under another name, a bare value, a number where text is taken, an argument too many.
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expr": "1+1"}}"#), Ok(tool("calculate", json!({ "expression": "1+1" }))));
        assert_eq!(asked(r#"{"tool": "calculate", "args": "1+1"}"#), Ok(tool("calculate", json!({ "expression": "1+1" }))));
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expression": 42}}"#), Ok(tool("calculate", json!({ "expression": "42" }))));
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expression": "1+1", "unit": "euros"}}"#), Ok(tool("calculate", json!({ "expression": "1+1" }))));
        // The arguments one level too deep, under the tool's name: what one family of models writes.
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"calculate": {"expression": "437 * 4 + 35"}}}"#), Ok(tool("calculate", json!({ "expression": "437 * 4 + 35" }))));
        // A plan written as lines.
        assert_eq!(
            asked(r#"{"tool": "plan", "args": {"steps": "1. Read the file\n2. Add the hours\n- Tell the total"}}"#),
            Ok(tool("plan", json!({ "steps": ["Read the file", "Add the hours", "Tell the total"] })))
        );
    }

    #[test]
    fn what_cannot_be_used_is_said_in_words_the_model_can_act_on() {
        assert_eq!(asked("I think the calculator would help."), Err("the reply is not a JSON object".into()));
        assert_eq!(asked(r#"{"tool": "weather", "args": {"city": "Rome"}}"#), Err("there is no tool called \"weather\". The tools are: time, calculate, plan, none".into()));
        assert_eq!(asked(r#"{"tool": "calculate", "args": {}}"#), Err("calculate needs \"expression\"".into()));
        assert_eq!(asked(r#"{"tool": "calculate", "args": {"expression": ""}}"#), Err("\"expression\" of calculate must be text, and not empty".into()));
        assert_eq!(asked(r#"{"tool": "plan", "args": {"steps": []}}"#), Err("\"steps\" of plan must be a list of texts, and not empty".into()));
        // A tool that is not offered here is not there.
        assert!(read(r#"{"tool": "plan", "args": {"steps": ["a", "b"]}}"#, &without_plan()).unwrap_err().contains("no tool called \"plan\""));
    }

    #[test]
    fn a_plan_is_no_longer_than_a_small_model_keeps_apart() {
        let many: Vec<String> = (1..=12).map(|i| format!("step {i}")).collect();
        let Ok(Asked::Tool { args, .. }) = asked(&json!({ "tool": "plan", "args": { "steps": many } }).to_string()) else { panic!() };
        assert_eq!(steps_of(&args).len(), MOST_STEPS_IN_A_PLAN);
    }

    #[test]
    fn the_tools_that_touch_nothing_are_run_here() {
        let clock: Clock = |_| Moment { day: "Saturday, 21 November 2026".into(), time: "14:05".into(), date: (2026, 11, 21) };
        assert_eq!(run("time", &Map::new(), clock), Ok("14:05, Saturday, 21 November 2026".into()));
        assert_eq!(run("calculate", json!({ "expression": "437 * 4 + 35" }).as_object().unwrap(), clock), Ok("1783".into()));
        assert_eq!(run("calculate", json!({ "expression": "days until 25 December" }).as_object().unwrap(), clock), Ok("34 days".into()));
        assert_eq!(run("calculate", json!({ "expression": "17:30 - time()" }).as_object().unwrap(), clock), Ok("3 hours 25 minutes".into()));
        assert!(run("calculate", json!({ "expression": "1 / 0" }).as_object().unwrap(), clock).is_err());
        assert!(run("plan", &Map::new(), clock).is_err(), "a plan is carried out by the chat, not here");
        // The real clock says the day and the time of a moment, here.
        let then = here(86_400_000 * 365);
        assert!(then.day.contains("1971") || then.day.contains("1970"), "{}", then.day);
        assert!((1970..=1971).contains(&then.date.0) && then.time.len() == 5 && then.time.contains(':'));
    }
}
