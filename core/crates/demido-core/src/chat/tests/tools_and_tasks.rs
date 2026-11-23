//! Turns that use tools, and work split into tasks, against the stand-in runtime.

use super::*;
use crate::tools::Step;

const NONE: &str = r#"{"tool": "none", "args": {}}"#;
const TOOLS: &str = "Tools you can ask for:\n- time(): The time now, to the minute. You do not know it without it.\n- calculate(expression: text): Exact arithmetic, and the days between two dates, or two times of day. Use it for every sum; do not work sums out yourself.\n- plan(steps: list of text): For work that needs several steps: write the steps as a list first.\n- none(): No tool is needed. Answer from what you know.";
const REPLY: &str = "Reply with one JSON object, nothing else: {\"tool\": \"name\", \"args\": {...}}";

/// A world where models may use tools, and what the stand-in will say: `decisions` to the questions that
/// want a JSON object, in order, and `replies` to the others.
fn with_tools(name: &str, decisions: &[&str], replies: &[&str]) -> World {
    let w = world(name);
    w.settings.set("tools.use", json!(true)).unwrap();
    w.stand.decisions.lock().unwrap().extend(decisions.iter().map(|d| d.to_string()));
    w.stand.replies.lock().unwrap().extend(replies.iter().map(|r| r.to_string()));
    w
}

fn asked(w: &World) -> Vec<Value> {
    w.stand.asked.lock().unwrap().clone()
}
fn contents(request: &Value) -> Vec<(String, String)> {
    request["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap().to_string(), m["content"].as_str().unwrap().to_string())).collect()
}
fn said(role: &str, content: impl Into<String>) -> (String, String) {
    (role.to_string(), content.into())
}

#[tokio::test]
async fn a_tool_is_asked_for_run_and_its_result_used() {
    let w = with_tools("tool", &[r#"{"tool": "calculate", "args": {"expression": "437 * 4 + 35"}}"#, NONE, NONE], &["It costs 1783 euros.", "You are welcome."]);
    let mut ear = w.events.listen();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "The flight is 437 euros each. How much for 4 of us plus one 35 euro bag?").await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "It costs 1783 euros."));
    assert_eq!(answer.steps.len(), 1);
    let step = &answer.steps[0];
    assert_eq!((step.tool.as_str(), &step.args, step.result.as_str(), step.state.as_str()), ("calculate", &json!({ "expression": "437 * 4 + 35" }), "1783", "done"));

    let requests = asked(&w);
    assert_eq!(requests.len(), 3, "which tool; which tool next; the answer");
    let request = "The flight is 437 euros each. How much for 4 of us plus one 35 euro bag?";
    // 1. The decision: the tools and the question are with the request, and only there. The reply is held to the tools' names.
    assert_eq!(contents(&requests[0]), [said("system", TOLD), said("user", format!("{TOOLS}\n\n{request}\n\nDoes the request need a tool? Most do not. {REPLY}"))]);
    assert_eq!(requests[0]["response_format"]["schema"]["properties"]["tool"]["enum"], json!(["time", "calculate", "plan", "none"]));
    assert_eq!((&requests[0]["temperature"], &requests[0]["chat_template_kwargs"]["enable_thinking"]), (&json!(0), &json!(false)));
    // 2. After the result: the call and its result are part of the conversation, the question is asked again.
    let call = r#"{"args":{"expression":"437 * 4 + 35"},"tool":"calculate"}"#;
    assert_eq!(
        contents(&requests[1]),
        [
            said("system", TOLD),
            said("user", request),
            said("assistant", call),
            said("user", format!("{TOOLS}\n\n<result>\n1783\n</result>\n\nAbove is the tool's result. Is another tool needed? Mostly not. {REPLY}"))
        ]
    );
    // 3. The answer: no tools in view, the result as material, what to do with it after it. Not held to any shape.
    let for_answer = contents(&requests[2]);
    assert_eq!(
        for_answer,
        [
            said("system", TOLD),
            said("user", request),
            said("assistant", call),
            said("user", format!("<result>\n1783\n</result>\n\nAbove is the tool's result. The person asked: \"{request}\" Answer that."))
        ]
    );
    assert!(requests[2].get("response_format").is_none());

    // The next turn starts with exactly what the last answer was asked with, and its answer: nothing is read twice.
    let next = turn(&w, &session.id, "Thanks.").await;
    assert!(next.steps.is_empty());
    let later = contents(asked(&w).last().unwrap());
    assert_eq!(&later[..4], &for_answer[..]);
    assert_eq!(&later[4..], [said("assistant", "It costs 1783 euros."), said("user", "Thanks.")]);

    // The use of the tool is in the log; how it went was told as it happened.
    let used = w.events.after(0, 200).unwrap().into_iter().find(|e| e.kind == "tool.used").expect("logged");
    assert_eq!((&used.data["tool"], &used.data["state"]), (&json!("calculate"), &json!("done")));
    let mut seen = Vec::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.steps" {
            seen.push(e.data["steps"][0]["state"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(seen, ["running", "done"]);
    w.end().await;
}

#[tokio::test]
async fn when_no_tool_is_needed_the_answer_is_asked_for_as_in_a_plain_chat() {
    let w = with_tools("none", &[NONE], &[]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "Say hello in French.").await;
    assert!(answer.steps.is_empty());
    let requests = asked(&w);
    assert_eq!(requests.len(), 2);
    assert_eq!(contents(&requests[1]), [said("system", TOLD), said("user", "Say hello in French.")]);
    assert!(requests[1].get("response_format").is_none() && requests[1].get("temperature").is_none());
    w.end().await;
}

#[tokio::test]
async fn the_clock_is_read_for_the_model() {
    let w = with_tools("clock", &[r#"{"tool": "time()", "args": {}}"#, NONE], &["It is 14:05."]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "What time is it?").await;
    assert_eq!(
        answer.steps,
        [Step {
            tool: "time".into(),
            args: json!({}),
            result: "14:05, Saturday, 21 November 2026".into(),
            state: "done".into(),
            took_ms: answer.steps[0].took_ms,
            change: None,
            output: None,
            artifact: None,
            model_chose: None
        }]
    );
    w.end().await;
}

#[tokio::test]
async fn a_request_that_cannot_be_used_is_handed_back_once() {
    // Mended at the second try.
    let w = with_tools("repair", &[r#"{"tool": "weather", "args": {"city": "Rome"}}"#, r#"{"tool": "calculate", "args": {"expression": "2 + 2"}}"#, NONE], &[]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "What is two and two?").await;
    assert_eq!((answer.steps.len(), answer.steps[0].result.as_str()), (1, "4"));
    let second = contents(&asked(&w)[1]);
    assert_eq!(second[second.len() - 2], said("assistant", r#"{"tool": "weather", "args": {"city": "Rome"}}"#));
    assert_eq!(second[second.len() - 1], said("user", "That cannot be used: there is no tool called \"weather\". The tools are: time, calculate, plan, none. Reply again with one JSON object."));
    w.end().await;

    // Still wrong at the second try: nothing is run, and the model answers knowing why.
    let w = with_tools("refused", &[r#"{"tool": "calculate", "args": {}}"#, r#"{"tool": "calculate", "args": {}}"#], &["About four."]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "What is two and two?").await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "About four."));
    assert_eq!((answer.steps[0].state.as_str(), answer.steps[0].result.as_str()), ("refused", "calculate needs \"expression\""));
    let requests = asked(&w);
    assert_eq!(requests.len(), 3, "the question, the repair, the answer: no third try");
    assert!(contents(&requests[2]).last().unwrap().1.starts_with("<result>\nThe tool was not run: calculate needs \"expression\".\n</result>"));
    assert!(w.events.after(0, 200).unwrap().iter().any(|e| e.kind == "tool.refused"));
    w.end().await;
}

#[tokio::test]
async fn a_tool_that_fails_says_why_and_asking_for_the_same_again_ends_the_asking() {
    let again = r#"{"tool": "calculate", "args": {"expression": "1 / 0"}}"#;
    let w = with_tools("fails", &[again, again, again], &["That cannot be worked out."]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "Divide one by zero.").await;
    assert_eq!(answer.steps.len(), 1, "the same request is not run twice");
    assert_eq!((answer.steps[0].state.as_str(), answer.steps[0].result.as_str()), ("failed", "that divides by zero"));
    let requests = asked(&w);
    assert_eq!(requests.len(), 3, "asked, asked again (the same came back), then the answer");
    assert!(contents(&requests[2]).last().unwrap().1.starts_with("<result>\nThe tool could not do it: that divides by zero.\n</result>"));
    w.end().await;
}

#[tokio::test]
async fn work_is_split_into_tasks_when_the_person_asks_and_demido_walks_them() {
    let w = with_tools(
        "tasks",
        &[
            r#"{"steps": ["Find the hours", "Add them up", "Tell the total"]}"#,
            NONE,                                                              // task 1 needs no tool
            r#"{"tool": "calculate", "args": {"expression": "3 + 2 + 4.5"}}"#, // task 2 does
            NONE,
            NONE, // task 3
            NONE, // the next turn
        ],
        &["The hours are 3, 2 and 4.5.", "9.5 hours.", "The total is 9.5 hours.", "You worked 9.5 hours.", "Yes."],
    );
    let mut ear = w.events.listen();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let request = "I worked 3 h, 2 h and 4.5 h. Find the hours, add them up and tell me the total.";
    w.chat.say_with(&session.id, request, true).unwrap();
    let answer = w.answered(&session.id).await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "You worked 9.5 hours."));

    let tasks = w.sessions.tasks_of(&answer.id).unwrap();
    assert_eq!(
        tasks.iter().map(|t| (t.title.as_str(), t.state.as_str(), t.result.as_deref().unwrap_or(""))).collect::<Vec<_>>(),
        [("Find the hours", "done", "The hours are 3, 2 and 4.5."), ("Add them up", "done", "9.5 hours."), ("Tell the total", "done", "The total is 9.5 hours.")]
    );
    assert_eq!(tasks[1].steps.len(), 1, "the tool a task used is kept with the task");
    assert_eq!(tasks[1].steps[0].result, "9.5");
    // What the plan gave is one step of the answer: each task and its result.
    let outcome = "1. Find the hours: The hours are 3, 2 and 4.5.\n2. Add them up: 9.5 hours.\n3. Tell the total: The total is 9.5 hours.";
    assert_eq!(answer.steps.len(), 1);
    assert_eq!((answer.steps[0].tool.as_str(), answer.steps[0].state.as_str(), answer.steps[0].result.as_str()), ("plan", "done", outcome));

    let requests = asked(&w);
    // The steps were asked for directly.
    assert_eq!(
        contents(&requests[0])[1].1,
        format!("{request}\n\nList the steps needed to do the request, as short instructions. Do not do them yet. Reply with one JSON object: {{\"steps\": [\"...\"]}}")
    );
    // Task 2 was asked for with the list in view: what is done and what it gave, the task in hand, what is left.
    let in_hand = format!("{request}\n\n<tasks>\n1. [done] Find the hours: The hours are 3, 2 and 4.5.\n2. [now] Add them up\n3. [ ] Tell the total\n</tasks>\n\nDo task 2 only: Add them up");
    let deciding = requests.iter().find(|r| contents(r).last().unwrap().1.contains("Does task 2 need a tool?")).expect("asked");
    assert_eq!(contents(deciding).last().unwrap().1, format!("{TOOLS}\n\n{in_hand}\n\nDoes task 2 need a tool? Most do not. {REPLY}"));
    let writing = requests.iter().find(|r| contents(r).last().unwrap().1.ends_with("Write the result of task 2, in one or two sentences.")).expect("asked");
    assert_eq!(contents(writing)[1], said("user", in_hand));
    assert_eq!(contents(writing).last().unwrap().1, "<result>\n9.5\n</result>\n\nAbove is the tool's result. Write the result of task 2, in one or two sentences.");
    // The answer was written from what the tasks gave.
    let plan = r#"{"args":{"steps":["Find the hours","Add them up","Tell the total"]},"tool":"plan"}"#;
    let for_answer = contents(requests.last().unwrap());
    assert_eq!(
        for_answer,
        [
            said("system", TOLD),
            said("user", request),
            said("assistant", plan),
            said("user", format!("<result>\n{outcome}\n</result>\n\nAbove is the tool's result. The person asked: \"{request}\" Answer that."))
        ]
    );

    // The list was told as it changed.
    let mut running = Vec::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.tasks" {
            running.push(e.data["tasks"].as_array().unwrap().iter().filter(|t| t["state"] == "done").count());
        }
    }
    assert_eq!((running.first(), running.last()), (Some(&0), Some(&3)), "{running:?}");

    // Later, the conversation holds the request, the plan and what it gave, and the answer: not what each task did.
    turn(&w, &session.id, "Is that more than a working day?").await;
    let later = contents(asked(&w).last().unwrap());
    assert_eq!(&later[..4], &for_answer[..]);
    assert!(!later.iter().any(|(_, text)| text.contains("3 + 2 + 4.5")));
    w.end().await;
}

#[tokio::test]
async fn a_model_can_ask_for_a_plan_itself_and_split_a_task_once() {
    let w = with_tools(
        "subtasks",
        &[
            r#"{"tool": "plan", "args": {"steps": ["Draft the note", "Check it"]}}"#,
            r#"{"tool": "plan", "args": {"steps": ["Write the greeting", "Write the body"]}}"#, // task 1 is split
            NONE,                                                                               // 1.1
            NONE,                                                                               // 1.2
            r#"{"tool": "plan", "args": {"steps": ["Just check it"]}}"#,                        // task 2 asks too, with one step: that is no plan
        ],
        &["Dear Ada,", "The shop opens on Monday.", "A note that greets Ada and says the shop opens on Monday.", "It reads well.", "Here is the note."],
    );
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "Write Ada a note that the shop opens on Monday, and check it.").await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "Here is the note."));
    let tasks = w.sessions.tasks_of(&answer.id).unwrap();
    assert_eq!(
        tasks.iter().map(|t| (t.parent_id.is_some(), t.title.as_str(), t.state.as_str(), t.result.as_deref().unwrap_or(""))).collect::<Vec<_>>(),
        [
            (false, "Draft the note", "done", "A note that greets Ada and says the shop opens on Monday."),
            (true, "Write the greeting", "done", "Dear Ada,"),
            (true, "Write the body", "done", "The shop opens on Monday."),
            (false, "Check it", "done", "It reads well."),
        ]
    );
    let requests = asked(&w);
    // A part of a task is not offered a plan of its own.
    let part = requests.iter().find(|r| contents(r).last().unwrap().1.contains("Does task 1.2 need a tool?")).expect("asked");
    assert_eq!(part["response_format"]["schema"]["properties"]["tool"]["enum"], json!(["time", "calculate", "none"]));
    assert!(contents(part).last().unwrap().1.contains("1. [now] Draft the note\n   1.1. [done] Write the greeting: Dear Ada,\n   1.2. [now] Write the body\n2. [ ] Check it"));
    // What the whole task gave was written from what its parts gave.
    assert!(requests.iter().any(|r| contents(r).last().unwrap().1.ends_with("All parts of task 1 are done. Write the result of task 1, in one or two sentences.")));
    w.end().await;
}

#[tokio::test]
async fn one_step_is_no_plan_and_always_is_a_setting() {
    // Asked for a plan, the model lists one step: the turn goes on as any other.
    let w = with_tools("one-step", &[r#"{"steps": ["Say hello"]}"#, NONE], &["Hello."]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say_with(&session.id, "Say hello.", true).unwrap();
    let answer = w.answered(&session.id).await;
    assert!(answer.steps.is_empty() && w.sessions.tasks_of(&answer.id).unwrap().is_empty());
    assert_eq!(asked(&w).len(), 3, "the steps, the tool, the answer");
    // The person asked for a plan, so the reply was held to two steps or more; a model that gives one all the same is taken at its word.
    assert_eq!(asked(&w)[0]["response_format"]["schema"]["properties"]["steps"]["minItems"], 2);

    // With the setting on Always, the steps are asked for without the person asking.
    w.settings.set("tools.plan", json!("always")).unwrap();
    w.stand.decisions.lock().unwrap().extend([r#"{"steps": ["One", "Two"]}"#.to_string()]);
    let answer = turn(&w, &session.id, "Do one, then two.").await;
    assert_eq!(w.sessions.tasks_of(&answer.id).unwrap().len(), 2);
    w.end().await;
}

#[tokio::test]
async fn stopping_ends_the_walk_and_says_where_it_stood() {
    let w = with_tools("stop-walk", &[r#"{"steps": ["First", "Second", "Third"]}"#], &["one two three four five six seven eight nine ten eleven twelve"]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.engine.load("small").await.unwrap();
    w.stand.word_every_ms.store(40, Ordering::Relaxed);
    let turn = w.chat.say_with(&session.id, "Do three things.", true).unwrap();
    // Wait for the first task to be in hand, then stop.
    for _ in 0..500 {
        if w.sessions.tasks_of(&turn.answer.id).unwrap().iter().any(|t| t.state == "running") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(w.chat.stop(&session.id));
    let answer = w.answered(&session.id).await;
    assert_eq!(answer.state, "stopped");
    let states: Vec<String> = w.sessions.tasks_of(&answer.id).unwrap().into_iter().map(|t| t.state).collect();
    assert_eq!(states, ["stopped", "pending", "pending"]);
    assert_eq!(answer.steps[0].result, "1. First: (not done)\n2. Second: (not done)\n3. Third: (not done)");
    w.end().await;
}

#[test]
fn a_text_that_goes_round_is_told_from_one_that_only_repeats_a_little() {
    use super::super::turn::going_round;
    let line = "25 December 2026 - 21 November 2026 = ";
    let stuck = format!("Let me work it out. {}", line.repeat(12));
    let end = going_round(&stuck).expect("it goes round");
    assert_eq!(stuck[..end].trim_end(), format!("Let me work it out. {line}").trim_end(), "kept up to the first of the repeats");
    // A long piece, repeated four times.
    let verse = "The sum is the difference between the two dates, which is the same as the difference between the two dates. ";
    assert!(going_round(&format!("Well. {}", verse.repeat(4))).is_some());
    assert!(going_round(&format!("Well. {}", verse.repeat(3))).is_none(), "three times is not yet round");
    // Decoration, a table's edge and ordinary writing do not count.
    assert!(going_round(&"-".repeat(300)).is_none());
    assert!(going_round(&"|---|---|---".repeat(40)).is_none());
    assert!(going_round(&"=-".repeat(200)).is_none());
    let list: String = (1..=60).map(|i| format!("{i}. Item number {i} of the list.\n")).collect();
    assert!(going_round(&list).is_none());
    assert!(going_round("Short.").is_none());
}

#[tokio::test]
async fn a_model_that_goes_round_is_stopped_and_the_answer_says_so() {
    let w = world("round");
    let line = "25 December 2026 minus 21 November 2026 equals ";
    *w.stand.reply.lock().unwrap() = format!("Let me see. {}", line.repeat(200));
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "How many days until 25 December?").await;
    assert_eq!(answer.state, "stopped");
    assert_eq!(answer.problem.as_deref(), Some("The model began to repeat itself, so Demido stopped it here."));
    assert_eq!(answer.content.trim_end(), format!("Let me see. {line}").trim_end());
    assert!((answer.timings.unwrap().written as usize) < 200 * 9, "it was not left to write all of it");
    w.end().await;
}

#[tokio::test]
async fn a_later_day_is_said_when_the_person_speaks_again() {
    // A clock on which a moment before the year 2000 is a Saturday, and anything later the Monday after.
    let clock: tools::Clock = |at| {
        if at < 946_684_800_000 {
            tools::Moment { day: "Saturday, 21 November 2026".into(), time: "14:05".into(), date: (2026, 11, 21) }
        } else {
            tools::Moment { day: "Monday, 23 November 2026".into(), time: "09:00".into(), date: (2026, 11, 23) }
        }
    };
    let w = world("days");
    let chat = Arc::new(Chat::new(w.sessions.clone(), w.engine.clone(), w.events.clone(), w.settings.clone()).with_clock(clock));
    let session = chat.start(None, Some("small")).await.unwrap();
    // The session, and the first thing said in it, are moved back to that Saturday.
    chat.say(&session.id, "Remind me what to buy.").unwrap();
    for _ in 0..500 {
        if !chat.answering(&session.id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    w.sessions.backdate(&session.id, 1_000).unwrap();
    chat.say(&session.id, "And today?").unwrap();
    for _ in 0..500 {
        if !chat.answering(&session.id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let asked = contents(asked(&w).last().unwrap());
    assert_eq!(asked[0], said("system", TOLD), "the first words never change: they say the day the session started");
    assert_eq!(asked[1], said("user", "Remind me what to buy."));
    assert_eq!(asked[3], said("user", "Today's date is now Monday, 23 November 2026.\n\nAnd today?"));
    w.end().await;
}

#[tokio::test]
async fn a_step_that_already_holds_its_result_is_handed_back_and_then_cut() {
    let request = "I worked 3.5 hours and 2.25 hours at 38 euros an hour. Add the hours, then work out the pay.";
    // The model does the sums while listing the steps, and gets one wrong. Asked again, it does it again.
    let done = r#"{"steps": ["Add the hours: 3.5 + 2.25 = 5.75 hours", "Calculate the pay: 5.75 hours x 38 = 219.5 euros"]}"#;
    let w = with_tools("done-already", &[done, done], &[]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say_with(&session.id, request, true).unwrap();
    let answer = w.answered(&session.id).await;
    let titles: Vec<String> = w.sessions.tasks_of(&answer.id).unwrap().into_iter().map(|t| t.title).collect();
    assert_eq!(titles, ["Add the hours", "Calculate the pay"], "what is left of each step is the instruction");
    let second = contents(&asked(&w)[1]);
    assert_eq!(second.last().unwrap().1, "The steps must be instructions only, with no results and no new numbers in them. Reply again with one JSON object.");
    w.end().await;

    // Steps that only use the request's own numbers are left as they are, and nothing is asked twice.
    let fine = r#"{"steps": ["Add 3.5 and 2.25", "Multiply the total by 38"]}"#;
    let w = with_tools("instructions", &[fine], &[]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say_with(&session.id, request, true).unwrap();
    let answer = w.answered(&session.id).await;
    let titles: Vec<String> = w.sessions.tasks_of(&answer.id).unwrap().into_iter().map(|t| t.title).collect();
    assert_eq!(titles, ["Add 3.5 and 2.25", "Multiply the total by 38"]);
    assert!(!asked(&w).iter().any(|r| contents(r).last().unwrap().1.starts_with("The steps must be")));
    w.end().await;
}

#[tokio::test]
async fn an_answer_wrapped_like_a_tools_result_is_unwrapped() {
    use super::super::turn::unwrapped;
    assert_eq!(unwrapped(r#"{"result":"Added the line."}"#).as_deref(), Some("Added the line."));
    assert_eq!(unwrapped(" {\"answer\": \"It is 4.\"}\n").as_deref(), Some("It is 4."));
    assert_eq!(unwrapped(r#"{"tool": "none", "args": {}, "answer": "Bonjour."}"#).as_deref(), Some("Bonjour."));
    assert_eq!(unwrapped("<result>\nThe sum cannot be worked out.\n</result>").as_deref(), Some("The sum cannot be worked out."));
    // What is not plainly a wrapped answer is left as it is.
    for kept in [r#"{"name": "Ada", "age": 36}"#, r#"{"result": 4}"#, r#"{"result": "a", "note": "b"}"#, r#"{"tool": "calculate", "args": {"expression": "1+1"}}"#, "Plain words.", r#"{"result": "#] {
        assert_eq!(unwrapped(kept), None, "{kept}");
    }

    // In a turn: after a tool, the wrapped answer is held back while it is written, and told whole, unwrapped.
    let w = with_tools("wrapped", &[r#"{"tool": "calculate", "args": {"expression": "2 + 2"}}"#, NONE, NONE], &[r#"{"result": "It is 4."}"#, r#"{"result": "kept as written"}"#]);
    let mut ear = w.events.listen();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let answer = turn(&w, &session.id, "What is two and two?").await;
    assert_eq!(answer.content, "It is 4.");
    let mut told = String::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.delta" {
            told.push_str(e.data["text"].as_str().unwrap());
        }
    }
    assert_eq!(told, "It is 4.", "the person never saw the wrapping");
    // A request for a tool where the answer should be: the model is told that the asking is over, and tries once more.
    let request = r#"{"tool": "calculate", "args": {"expression": "6 * 2"}}"#;
    *w.stand.decisions.lock().unwrap() = [r#"{"tool": "calculate", "args": {"expression": "3 + 3"}}"#.to_string(), NONE.to_string()].into();
    *w.stand.replies.lock().unwrap() = [request.to_string(), "It is 6, and doubled it is 12.".to_string()].into();
    let mended = turn(&w, &session.id, "And three and three, doubled?").await;
    assert_eq!((mended.state.as_str(), mended.content.as_str()), ("done", "It is 6, and doubled it is 12."));
    let second_try = contents(asked(&w).last().unwrap());
    assert_eq!(second_try[second_try.len() - 2], said("assistant", request));
    assert!(second_try.last().unwrap().1.starts_with("That is a request for a tool, not an answer. No more tools can be used now."));
    // If it does it again, what it wrote is kept, and said to be no answer.
    *w.stand.decisions.lock().unwrap() = [r#"{"tool": "calculate", "args": {"expression": "4 + 4"}}"#.to_string(), NONE.to_string()].into();
    *w.stand.replies.lock().unwrap() = [request.to_string(), request.to_string()].into();
    let stuck = turn(&w, &session.id, "And four and four?").await;
    assert_eq!((stuck.state.as_str(), stuck.content.as_str()), ("failed", request));
    assert!(stuck.problem.unwrap().starts_with("The model wrote a request for a tool where its answer should be."));
    // Without a tool in the turn, an answer that is a JSON object is what was asked for, and stays.
    w.stand.replies.lock().unwrap().push_back(r#"{"result": "kept as written"}"#.into());
    assert_eq!(turn(&w, &session.id, "Give me that as JSON.").await.content, r#"{"result": "kept as written"}"#);
    // But a request for a tool is no answer, with a tool in the turn or without, and so is a result made up
    // under it (Granite, asked "Ready?" in voice mode): the request alone is shown back, and nobody hears it.
    let (date, made_up) = (r#"{"args":{"expression":"now()"},"tool":"calculate"}"#, "\n\n<result>\nThe current date and time is 2023-11-23 10:20:15.\n</result>");
    *w.stand.decisions.lock().unwrap() = [NONE.to_string()].into();
    *w.stand.replies.lock().unwrap() = [format!("{date}{made_up}"), "Yes, I am ready.".to_string()].into();
    let mut ear = w.events.listen();
    let ready = turn(&w, &session.id, "Ready?").await;
    assert_eq!((ready.state.as_str(), ready.content.as_str()), ("done", "Yes, I am ready."));
    let second_try = contents(asked(&w).last().unwrap());
    assert_eq!(second_try[second_try.len() - 2], said("assistant", date));
    let mut told = String::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.delta" && e.data["message"] == ready.id.as_str() {
            told.push_str(e.data["text"].as_str().unwrap());
        }
    }
    assert_eq!(told, "Yes, I am ready.");
    w.end().await;
}

#[test]
fn a_request_is_found_at_the_start_of_an_answer_with_what_came_after_it_left_out() {
    use super::super::turn::request_of;
    let request = r#"{"tool": "calculate", "args": {"expression": "1+1"}}"#;
    assert_eq!(request_of(request), Some(request));
    assert_eq!(request_of(&format!("  {request}\n\n<result>\n2\n</result>")), Some(request));
    assert_eq!(request_of(&format!("{request}<result>2</result>")), Some(request));
    for not in [r#"{"result": "a"}"#, "Plain words.", r#"{"tool": "calculate", "args"#, r#"Sure: {"tool": "calculate"}"#, ""] {
        assert_eq!(request_of(not), None, "{not}");
    }
}

#[tokio::test]
async fn a_model_gives_an_artifact_and_a_new_version_of_it_and_is_told_not_to_write_it_out_again() {
    let page = "<!doctype html>\n<h1>Sol</h1>\n<p>Open 7 to 13.</p>";
    let changed = "<!doctype html>\n<h1>Sol</h1>\n<p>Open 8 to 14.</p>";
    let give = r#"{"tool": "give_file", "args": {"file_name": "sol.html"}}"#;
    // The decision names the file; its text is asked for after it, as ordinary text (here once in a fence,
    // which is taken off).
    let fenced = format!("```html\n{changed}\n```");
    let w = with_tools("artifact", &[give, NONE, give, NONE], &[page, "A page for Sol, with its hours.", &fenced, "The hours are now 8 to 14."]);
    w.settings.set("tools.artifacts", json!("nofolder")).unwrap();
    let session = w.chat.start(None, Some("small")).await.unwrap();

    let answer = turn(&w, &session.id, "Make a one-page site for my bakery, Sol, open 7 to 13.").await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "A page for Sol, with its hours."));
    let step = &answer.steps[0];
    assert_eq!((step.tool.as_str(), step.state.as_str()), ("give_file", "done"));
    let given = step.artifact.clone().expect("the step says which artifact");
    assert_eq!((given.name.as_str(), given.n), ("sol.html", 1));
    assert!(step.result.contains("Do not write its text out again"), "{}", step.result);
    let wrote = asked(&w)[1].clone();
    assert!(wrote.get("response_format").is_none(), "the text is written as ordinary text, not inside JSON");
    assert!(contents(&wrote).last().unwrap().1.ends_with("Write the whole of sol.html now: only its text, as it goes in the file, with nothing before or after it."));
    // It was offered, as a tool that needs nobody's yes.
    assert!(asked(&w)[0]["response_format"]["schema"]["properties"]["tool"]["enum"].as_array().unwrap().contains(&json!("give_file")));

    let again = turn(&w, &session.id, "Change the hours to 8 to 14.").await;
    // Asked again, the model is shown the files it gave, by their names.
    let decided = asked(&w).iter().filter(|r| r.get("response_format").is_some()).nth(2).cloned().unwrap();
    assert!(contents(&decided).last().unwrap().1.contains("Files you gave the person in this session: sol.html. The same name again gives a new version."));
    let second = again.steps[0].artifact.clone().unwrap();
    assert_eq!((second.id.as_str(), second.n), (given.id.as_str(), 2), "the same name: a new version of the same artifact");
    assert_eq!(w.sessions.artifact_version(&given.id, Some(1)).unwrap().text, page, "the first version is kept");
    assert_eq!(w.sessions.artifact_version(&given.id, None).unwrap().text, changed);
    assert_eq!(w.sessions.artifact_version(&given.id, None).unwrap().message_id.as_deref(), Some(again.id.as_str()), "made by that answer");
    w.end().await;
}

#[tokio::test]
async fn artifacts_are_not_offered_when_the_person_turned_them_off() {
    let w = with_tools("no-artifacts", &[NONE], &["Hello."]);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    turn(&w, &session.id, "Hello").await;
    assert_eq!(asked(&w)[0]["response_format"]["schema"]["properties"]["tool"]["enum"], json!(["time", "calculate", "plan", "none"]));
    w.end().await;
}

#[tokio::test]
async fn by_default_artifacts_are_offered_only_in_a_session_without_a_folder() {
    let w = with_tools("artifact-default", &[NONE, NONE, NONE], &["One.", "Two.", "Three."]);
    w.settings.set("tools.artifacts", json!("nofolder")).unwrap();
    let offered = |w: &World, i: usize| asked(w).iter().filter(|r| r.get("response_format").is_some()).nth(i).unwrap()["response_format"]["schema"]["properties"]["tool"]["enum"].clone();
    let without = w.chat.start(None, Some("small")).await.unwrap();
    turn(&w, &without.id, "Hello").await;
    assert!(offered(&w, 0).as_array().unwrap().contains(&json!("give_file")));
    let folder = crate::testing::TempDir::new("demido-artifact-folder");
    let with = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.set_folder(&with.id, Some(folder.to_str().unwrap())).unwrap();
    turn(&w, &with.id, "Hello").await;
    assert!(!offered(&w, 1).as_array().unwrap().contains(&json!("give_file")), "with a folder, models write files there");
    w.settings.set("tools.artifacts", json!("always")).unwrap();
    turn(&w, &with.id, "Hello").await;
    assert!(offered(&w, 2).as_array().unwrap().contains(&json!("give_file")), "unless the person says always");
    w.end().await;
}
