//! Messages between sessions (DECISIONS S5): one session tells another in the same folder, Demido tells the others
//! which files a turn changed, and what came from another session reaches a model fenced, as text from outside.

use super::*;

const NONE: &str = r#"{"tool": "none", "args": {}}"#;

#[tokio::test]
async fn sessions_in_one_folder_are_told_of_each_others_changes_and_can_message_each_other() {
    let w = world("between");
    w.settings.set("tools.use", json!(true)).unwrap();
    let folder = w.home.join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.md"), "Herons\n").unwrap();
    let a = w.chat.start(Some("Writer"), Some("small")).await.unwrap();
    let b = w.chat.start(Some("Checker"), Some("small")).await.unwrap();
    let alone = w.chat.start(Some("Elsewhere"), Some("small")).await.unwrap();
    for s in [&a, &b] {
        w.chat.set_folder(&s.id, Some(folder.to_str().unwrap())).unwrap();
    }
    let a = w.sessions.get(&a.id).unwrap();
    assert!(w.chat.tools_told(&a).unwrap().contains("- message_session(to: text, text: text)"), "offered while another session shares the folder");
    assert!(!w.chat.tools_told(&w.sessions.get(&alone.id).unwrap()).unwrap().contains("message_session"));
    assert!(w.chat.others_in_folder(&w.sessions.get(&alone.id).unwrap()).is_empty(), "a session with no folder has nobody to tell");

    // A changes a file: B is told which, by Demido, not by a model.
    w.stand.decisions.lock().unwrap().extend([r#"{"tool": "add_to_file", "args": {"path": "notes.md", "text": "Grey herons nest in reeds."}}"#.to_string(), NONE.to_string()]);
    w.stand.replies.lock().unwrap().push_back("Added.".into());
    let _ = turn(&w, &a.id, "Add a line about nests to notes.md.").await;
    let told = w.sessions.messages(&b.id).unwrap();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].role, "session");
    assert!(told[0].content.starts_with("Demido's note, about the session \"Writer\"\nIt changed notes.md in the folder just now."), "{}", told[0].content);
    assert!(w.sessions.messages(&alone.id).unwrap().is_empty());

    // A messages B by its name.
    w.stand.decisions.lock().unwrap().extend([r#"{"tool": "message_session", "args": {"to": "checker", "text": "I am rewriting notes.md; leave it for a minute."}}"#.to_string(), NONE.to_string()]);
    w.stand.replies.lock().unwrap().push_back("Told it.".into());
    let answer = turn(&w, &a.id, "Tell the other session to wait.").await;
    assert_eq!((answer.steps[0].tool.as_str(), answer.steps[0].state.as_str()), ("message_session", "done"));
    let told = w.sessions.messages(&b.id).unwrap();
    assert_eq!(told[1].content, "Writer\nI am rewriting notes.md; leave it for a minute.");

    // B's model is given both, fenced as text from another session, with its person's request after them.
    w.stand.decisions.lock().unwrap().push_back(NONE.into());
    w.stand.replies.lock().unwrap().push_back("I will wait.".into());
    let _ = turn(&w, &b.id, "Go on with the checks.").await;
    let asked = w.stand.asked.lock().unwrap().last().unwrap()["messages"].as_array().unwrap().iter().map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n");
    assert!(asked.contains("<from another session: Writer>\nI am rewriting notes.md; leave it for a minute.\n</from another session>"), "{asked}");
    assert!(asked.find("<from another session: Writer>").unwrap() < asked.find("Go on with the checks.").unwrap());
    w.end().await;
}
