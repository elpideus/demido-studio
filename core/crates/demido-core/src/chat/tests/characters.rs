//! Characters through a turn (DECISIONS E): a session set to one is told its instructions after Demido's own,
//! as the person's, and answers at its temperature; a character deleted leaves its sessions, set to none.

use super::*;
use crate::characters::{Characters, NewCharacter};

#[tokio::test]
async fn a_session_set_to_a_character_is_told_its_instructions_and_answers_at_its_temperature() {
    let mut w = world("characters");
    let characters = Arc::new(Characters::new(w.db.clone()));
    w.chat = Arc::new(Chat::new(w.sessions.clone(), w.engine.clone(), w.events.clone(), w.settings.clone()).with_clock(CLOCK).with_characters(characters.clone()));
    let mira = characters
        .create(&NewCharacter {
            name: "Mira".into(),
            about: "Reads long documents".into(),
            temperature: Some(0.3),
            instructions: "Answer in one sentence, then give your sources.".into(),
            ..NewCharacter::default()
        })
        .unwrap();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let plain = w.chat.told_first(&session);
    let session = w.sessions.set_character(&session.id, Some(&mira.id)).unwrap();
    let told = w.chat.told_first(&session);
    assert!(told.starts_with(&plain), "Demido's own words come first");
    assert!(
        told.ends_with("In this session you are \"Mira\": Reads long documents. The person wrote these instructions for it; follow them as theirs:\nAnswer in one sentence, then give your sources."),
        "{told}"
    );

    w.stand.replies.lock().unwrap().push_back("Herons nest in reeds (Birds of Europe, p. 12).".into());
    let answer = turn(&w, &session.id, "Where do herons nest?").await;
    assert_eq!(answer.content, "Herons nest in reeds (Birds of Europe, p. 12).");
    let asked = w.stand.asked.lock().unwrap().last().unwrap().clone();
    assert_eq!(asked["temperature"].as_f64(), Some(0.3), "the character's temperature: {asked}");
    assert!(asked["messages"][0]["content"].as_str().unwrap().contains("follow them as theirs"), "told first, in the system's words");

    // Forgotten: the session stays, set to none, and is told Demido's own words alone.
    assert!(characters.delete(&mira.id).unwrap());
    let after = w.sessions.get(&session.id).unwrap();
    assert_eq!(after.character_id, None);
    assert_eq!(w.chat.told_first(&after), plain);
    w.end().await;
}
