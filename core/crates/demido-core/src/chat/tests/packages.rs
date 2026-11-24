//! A tool the person installed, through a turn (DECISIONS C9): offered with the others, chosen, run in Wasmtime on
//! the session's folder, and asked for at Cautious as Demido's own reading tools are.

use super::*;
use crate::packages::tests::{Publisher, reading};
use crate::permissions::Said;

const NONE: &str = r#"{"tool": "none", "args": {}}"#;

#[tokio::test(flavor = "multi_thread")]
async fn an_installed_tool_is_offered_chosen_and_run_on_the_folder() {
    let mut w = world("installed-tool");
    w.settings.set("tools.use", json!(true)).unwrap();
    let packages = Arc::new(crate::packages::Packages::new(&w.home, w.events.clone()));
    let (archive, sig) = Publisher::new().package("0.1.0", reading(), None);
    packages.install(&archive, &sig, true).unwrap();
    w.chat = Arc::new(Chat::new(w.sessions.clone(), w.engine.clone(), w.events.clone(), w.settings.clone()).with_clock(CLOCK).with_packages(packages));
    let folder = w.home.join("work");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.md"), "one two three\n").unwrap();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    // Without a folder a tool that reads the folder is not offered.
    assert!(!w.chat.tools_told(&session).unwrap().contains("word_count"));
    let session = w.chat.set_folder(&session.id, Some(folder.to_str().unwrap())).unwrap();
    assert!(w.chat.tools_told(&session).unwrap().contains("- word_count(path: text): The words, lines and characters of a file in the folder."));

    w.stand.decisions.lock().unwrap().extend([r#"{"tool": "word_count", "args": {"path": "notes.md"}}"#.to_string(), NONE.to_string()]);
    w.stand.replies.lock().unwrap().push_back("Three words.".into());
    let answer = turn(&w, &session.id, "How many words are in notes.md?").await;
    assert_eq!((answer.steps[0].tool.as_str(), answer.steps[0].state.as_str()), ("word_count", "done"), "{:?}", answer.steps);
    assert_eq!(answer.steps[0].result, "notes.md: 3 words, 1 lines, 14 characters.");

    // At Cautious it asks first, as any use that reaches the folder does.
    w.chat.set_permission(&session.id, "cautious").unwrap();
    w.stand.decisions.lock().unwrap().extend([r#"{"tool": "word_count", "args": {"path": "notes.md"}}"#.to_string(), NONE.to_string()]);
    w.stand.replies.lock().unwrap().push_back("Not counted.".into());
    w.chat.say(&session.id, "Count it again.").unwrap();
    let mut request = None;
    for _ in 0..500 {
        request = w.chat.waiting_for(&session.id);
        if request.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let request = request.expect("it asks at Cautious");
    assert_eq!((request.tool.as_str(), request.reach), ("word_count", tools::Reach::Reads));
    w.chat.answer_request(&session.id, &request.id, Said::No).unwrap();
    assert_eq!(w.answered(&session.id).await.steps[0].state, "refused");
    w.end().await;
}
