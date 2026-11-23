//! One spoken conversation: the window on one side of a WebSocket, `demido voice-engine` on the other, and a
//! session between them (DECISIONS V1 to V7).
//!
//! From the window: sound (16 kHz, mono, 16-bit) as binary messages; and as text, `{"type":"answer_now"}`,
//! `{"type":"playing","on":true}` (Demido's voice is being played), `{"type":"played","message":id,"sentence":n,"ms":t}`
//! (after `stop_playback`, which names the answer: how much of its sentence `n` was heard) and `{"type":"stop"}`
//! (stop speaking).
//! To the window: Demido's sound as binary messages (the sentence's number, four bytes, then 24 kHz mono 16-bit),
//! and as text what happens: `ready`, `speech`, `waiting`, `heard`, `heard_nothing`, `thinking`, `say`, `said`,
//! `answered`, `stop_playback`, `cut`, `error`.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::{Saying, Voice, engine_args, event_of, heard_of};
use crate::chat::Chat;
use crate::events::EventLog;
use crate::sessions::Sessions;

/// What a conversation is asked for, from the address the window opens.
#[derive(Clone, Debug, Default, serde::Deserialize, utoipa::IntoParams)]
pub struct Opened {
    /// The session to talk in. Left out: a new one.
    pub session: Option<String>,
    /// The voice, by its id. Left out: the first.
    pub voice: Option<String>,
    /// "Stop when I speak" (V3): on unless `false`.
    pub barge_in: Option<bool>,
    /// The model, for a new session. Left out: one that is loaded, or else the first on this computer.
    pub model: Option<String>,
}

/// The answer being spoken.
#[derive(Default)]
struct Answer {
    id: String,
    /// Its text, as it is written and said.
    saying: Saying,
    /// The sentences sent, in order, with their sound's length once it is known (24 kHz samples).
    sentences: Vec<(u32, String, u64)>,
    /// The number of the last sentence the engine has finished saying.
    said: u32,
    /// Some of its sound has gone to the window: what was heard of it is the window's to say.
    voiced: bool,
}

impl Answer {
    /// All of it has been said: the model has finished writing, and the engine saying.
    fn all_said(&self) -> bool {
        self.saying.all_taken() && self.sentences.last().is_none_or(|(no, _, _)| *no <= self.said)
    }
}

async fn send_frame(stdin: &mut ChildStdin, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(kind);
    frame.extend((payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    stdin.write_all(&frame).await?;
    stdin.flush().await
}

async fn read_frame(stdout: &mut ChildStdout) -> std::io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    stdout.read_exact(&mut head).await?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    let mut payload = vec![0u8; len];
    stdout.read_exact(&mut payload).await?;
    Ok((head[0], payload))
}

async fn tell(socket: &mut WebSocket, what: Value) -> bool {
    socket.send(Message::Text(what.to_string().into())).await.is_ok()
}

/// Starts the engine for a voice. `Err`: why it cannot.
pub fn start_engine(voice: &Voice, voice_id: &str) -> Result<Child, String> {
    let program = voice.program().ok_or("Voice runs in Demido's own program (demido voice-engine), and it was not found beside this server.")?;
    let wav = voice.voice_path(voice_id).filter(|p| p.is_file()).ok_or_else(|| format!("The voice {voice_id} is not downloaded."))?;
    let threads = std::thread::available_parallelism().map_or(4, |n| (n.get() as u32 / 4).clamp(2, 4));
    tokio::process::Command::new(program)
        .args(engine_args(voice.dir(), &wav, threads))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("The voice engine could not be started: {e}"))
}

/// Runs one conversation until the window closes it.
pub async fn run(mut socket: WebSocket, voice: Arc<Voice>, chat: Arc<Chat>, sessions: Arc<Sessions>, events: Arc<EventLog>, opened: Opened) {
    let status = voice.status();
    if !status.ready {
        let why = status.problem.unwrap_or_else(|| format!("Voice mode's files are not all downloaded yet ({} MB to go).", status.missing_bytes / 1_000_000));
        tell(&mut socket, json!({ "type": "error", "message": why })).await;
        return;
    }
    let voice_id = opened.voice.clone().filter(|v| super::VOICES.iter().any(|x| x.id == v)).unwrap_or_else(|| super::VOICES[0].id.to_string());
    let session = match opened.session.as_deref() {
        Some(id) => match sessions.get(id) {
            Ok(s) => s,
            Err(e) => {
                tell(&mut socket, json!({ "type": "error", "message": e.to_string() })).await;
                return;
            }
        },
        None => match chat.start(Some("Spoken conversation"), opened.model.as_deref()).await {
            Ok(s) => s,
            Err(e) => {
                tell(&mut socket, json!({ "type": "error", "message": e.to_string() })).await;
                return;
            }
        },
    };
    let mut engine = match start_engine(&voice, &voice_id) {
        Ok(child) => child,
        Err(why) => {
            tell(&mut socket, json!({ "type": "error", "message": why })).await;
            return;
        }
    };
    let (Some(mut stdin), Some(mut stdout)) = (engine.stdin.take(), engine.stdout.take()) else {
        tell(&mut socket, json!({ "type": "error", "message": "The voice engine could not be talked to." })).await;
        return;
    };
    let barge_in = opened.barge_in.unwrap_or(true);
    let _ = events.append("voice.started", Some(&session.id), json!({ "voice": voice_id }));
    tell(&mut socket, json!({ "type": "ready", "session": session.id, "voice": voice_id, "model": session.model, "sample_rate": 24000 })).await;

    // The engine's frames, read on their own task, so that a long sentence never holds the window's sound up.
    let (frames_tx, mut frames) = tokio::sync::mpsc::channel::<(u8, Vec<u8>)>(256);
    let reader = tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut stdout).await {
            if frames_tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    let mut news = events.listen();
    let mut answer: Option<Answer> = None;
    let mut sentence_no: u32 = 0;
    let mut playing = false;
    // A cut answer waits for the window to say how much of it was heard.
    let mut cut_pending: Option<Answer> = None;

    loop {
        tokio::select! {
            from_window = socket.recv() => {
                let Some(Ok(message)) = from_window else { break };
                match message {
                    Message::Binary(sound) => {
                        if send_frame(&mut stdin, b'a', &sound).await.is_err() { break }
                    }
                    Message::Text(text) => {
                        let said: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                        match said["type"].as_str() {
                            Some("answer_now") => { let _ = send_frame(&mut stdin, b'n', &[]).await; }
                            Some("playing") => {
                                playing = said["on"].as_bool().unwrap_or(false);
                                let _ = send_frame(&mut stdin, b'p', &[u8::from(playing)]).await;
                            }
                            Some("stop") => {
                                if let Some(a) = answer.take() {
                                    interrupt(&mut stdin, &chat, &sessions, &mut socket, &session.id, a, &mut cut_pending).await;
                                }
                            }
                            Some("played") => {
                                let about = said["message"].as_str();
                                if let Some(a) = cut_pending.take_if(|a| about.is_none_or(|m| m == a.id)) {
                                    let at = said["sentence"].as_u64().unwrap_or(0) as u32;
                                    let ms = said["ms"].as_u64().unwrap_or(0);
                                    settle(&chat, &sessions, &mut socket, &session.id, a, Some((at, ms))).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            frame = frames.recv() => {
                let Some((kind, payload)) = frame else {
                    tell(&mut socket, json!({ "type": "error", "message": "The voice engine stopped." })).await;
                    break;
                };
                if kind == b'o' {
                    if payload.len() >= 4 && let Some(a) = answer.as_mut() {
                        let n = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                        if a.sentences.iter().any(|(no, _, _)| *no == n) { a.voiced = true; }
                    }
                    if socket.send(Message::Binary(payload.into())).await.is_err() { break }
                    continue;
                }
                let e = event_of(&payload);
                match e["event"].as_str().unwrap_or_default() {
                    "heard" => {
                        let text = e["text"].as_str().unwrap_or_default().to_string();
                        tell(&mut socket, json!({ "type": "heard", "text": text, "by": e["by"] })).await;
                        // What is said over an answer still being written ends that answer first.
                        if let Some(a) = answer.take() {
                            interrupt(&mut stdin, &chat, &sessions, &mut socket, &session.id, a, &mut cut_pending).await;
                        }
                        for _ in 0..100 {
                            if !chat.answering(&session.id) { break }
                            tokio::time::sleep(Duration::from_millis(30)).await;
                        }
                        match chat.say(&session.id, &text) {
                            Ok(turn) => {
                                answer = Some(Answer { id: turn.answer.id.clone(), ..Answer::default() });
                                tell(&mut socket, json!({ "type": "thinking", "message": turn.answer.id })).await;
                            }
                            Err(err) => { tell(&mut socket, json!({ "type": "error", "message": err.to_string() })).await; }
                        }
                    }
                    "speech" => {
                        let over = e["over_demido"].as_bool().unwrap_or(false) || playing;
                        tell(&mut socket, json!({ "type": "speech" })).await;
                        // Speaking over Demido stops it at once (V3): unless the person turned that off.
                        if barge_in
                            && over
                            && let Some(a) = answer.take()
                        {
                            interrupt(&mut stdin, &chat, &sessions, &mut socket, &session.id, a, &mut cut_pending).await;
                        }
                    }
                    "waiting" => {
                        let _ = events.append("voice.turn", Some(&session.id), json!({ "probability": e["probability"], "seconds": e["seconds"], "decided": if e["maybe"] == true { "maybe" } else { "waits" } }));
                        tell(&mut socket, json!({ "type": "waiting", "probability": e["probability"], "maybe": e["maybe"] })).await;
                    }
                    "finished" => {
                        let decided = if e["after_silence_s"].is_null() { "finished" } else { "finished after more silence" };
                        let _ = events.append("voice.turn", Some(&session.id), json!({ "probability": e["probability"], "seconds": e["seconds"], "after_silence_s": e["after_silence_s"], "decided": decided }));
                        tell(&mut socket, json!({ "type": "finished" })).await;
                    }
                    "heard_nothing" => { tell(&mut socket, json!({ "type": "heard_nothing" })).await; }
                    "speaking" => { tell(&mut socket, json!({ "type": "speaking", "sentence": e["sentence"], "first_ms": e["first_ms"] })).await; }
                    "said" => {
                        let n = e["sentence"].as_u64().unwrap_or(0) as u32;
                        let samples = e["samples"].as_u64().unwrap_or(0);
                        if let Some(a) = answer.as_mut()
                            && let Some(s) = a.sentences.iter_mut().find(|(no, _, _)| *no == n)
                        {
                            s.2 = samples;
                            a.said = a.said.max(n);
                        }
                        tell(&mut socket, json!({ "type": "said", "sentence": n })).await;
                        if answer.as_ref().is_some_and(Answer::all_said) {
                            answer = None;
                            tell(&mut socket, json!({ "type": "answered" })).await;
                        }
                    }
                    "error" => { tell(&mut socket, json!({ "type": "error", "message": e["message"] })).await; }
                    _ => {}
                }
            }
            heard = news.recv() => {
                let Ok(event) = heard else { continue };
                let Some(a) = answer.as_mut() else { continue };
                if event.data["message"].as_str() != Some(a.id.as_str()) { continue }
                match event.kind.as_str() {
                    "message.delta" => a.saying.write(event.data["text"].as_str().unwrap_or_default()),
                    "message.done" | "message.stopped" | "message.failed" => a.saying.end(|| sessions.message(&a.id).ok().map(|m| m.content)),
                    _ => continue,
                }
                // Every finished sentence is said as soon as it is written; at the end, the rest.
                while let Some(spoken) = a.saying.next_sentence() {
                    sentence_no += 1;
                    a.sentences.push((sentence_no, spoken.clone(), 0));
                    let mut payload = sentence_no.to_le_bytes().to_vec();
                    payload.extend(spoken.as_bytes());
                    let _ = send_frame(&mut stdin, b's', &payload).await;
                    tell(&mut socket, json!({ "type": "say", "message": a.id, "sentence": sentence_no, "text": spoken })).await;
                }
                // Nothing to say, or all of it said before the model had finished.
                if a.all_said() {
                    answer = None;
                    tell(&mut socket, json!({ "type": "answered" })).await;
                }
            }
        }
    }
    let _ = send_frame(&mut stdin, b'q', &[]).await;
    drop(stdin);
    if tokio::time::timeout(Duration::from_secs(5), engine.wait()).await.is_err() {
        let _ = engine.kill().await;
    }
    reader.abort();
    let _ = events.append("voice.ended", Some(&session.id), json!({}));
}

/// Stops an answer: the engine's sentences, and the answer being written (V3). When some of its sound went to
/// the window, the window is asked how far it got (`stop_playback`, answered by `played`); when none did, nothing
/// of it was heard, and it is cut at once. A cut still waiting for its answer is settled first, as it stands.
#[allow(clippy::too_many_arguments)]
async fn interrupt(stdin: &mut ChildStdin, chat: &Chat, sessions: &Sessions, socket: &mut WebSocket, session: &str, answer: Answer, cut_pending: &mut Option<Answer>) {
    let _ = send_frame(stdin, b'x', &[]).await;
    chat.stop(session);
    if let Some(earlier) = cut_pending.take() {
        settle(chat, sessions, socket, session, earlier, None).await;
    }
    if answer.voiced {
        tell(socket, json!({ "type": "stop_playback", "message": answer.id })).await;
        *cut_pending = Some(answer);
    } else {
        settle(chat, sessions, socket, session, answer, None).await;
    }
}

/// Takes out of an answer what the person did not hear: `played`, the sentence that was playing and how far
/// into it (in milliseconds); `None`, nothing was heard. Waits for the stopped answer to be stored first, so
/// that its storing does not undo the cut.
async fn settle(chat: &Chat, sessions: &Sessions, socket: &mut WebSocket, session: &str, a: Answer, played: Option<(u32, u64)>) {
    let heard = match played {
        Some((at, ms)) => {
            let spoken: Vec<(String, u64)> = a.sentences.iter().map(|(_, t, n)| (t.clone(), *n)).collect();
            let index = a.sentences.iter().position(|(n, _, _)| *n == at).unwrap_or(spoken.len());
            heard_of(&spoken, index, ms * 24)
        }
        None => String::new(),
    };
    for _ in 0..250 {
        if !chat.answering(session) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let _ = sessions.cut_spoken(&a.id, &heard);
    tell(socket, json!({ "type": "cut", "message": a.id, "heard": heard })).await;
}
