//! Voice mode (brief 4.25; DECISIONS V1 to V7; research notes 25 and 28).
//!
//! The speech parts run in a process of their own, `demido voice-engine` (the `demido` program links them; the
//! core links none). This module keeps what that process needs, downloaded on demand from its makers and
//! checked, and runs a conversation: the window sends the microphone's sound over a WebSocket; the engine
//! says when a sentence is finished and what was said; that is said in a session, as typed words would be;
//! the answer is spoken a sentence at a time, as it is written; and when the person speaks over it, it stops,
//! and the part they did not hear is taken out of the conversation (V3).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::{Error, Result};

pub mod talk;

/// One file the engine needs, from where its makers publish it, with the sum they publish for it.
#[derive(Clone, Debug)]
pub struct File {
    /// Where it goes, in the voice folder.
    pub path: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Everything voice mode needs (read 22 November 2026, from the makers' pages: research note 28).
pub const FILES: &[File] = &[
    // Silero VAD, MIT: sherpa-onnx's release of it.
    File {
        path: "silero_vad.onnx",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        size: 643_854,
        sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
    },
    // Smart Turn v3.2, BSD-2-Clause, from pipecat-ai.
    File {
        path: "smart-turn-v3.2-cpu.onnx",
        url: "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/main/smart-turn-v3.2-cpu.onnx",
        size: 8_679_182,
        sha256: "2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f",
    },
    // NVIDIA Parakeet TDT 0.6B v3, CC-BY-4.0, as sherpa-onnx's maintainer exports it.
    File {
        path: "parakeet/encoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/encoder.int8.onnx",
        size: 652_282_294,
        sha256: "1afbe69c1ffdabee94755f42e4daf1d93549131a3da11356bda84fcef0f2c479",
    },
    File {
        path: "parakeet/decoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/decoder.int8.onnx",
        size: 11_845_274,
        sha256: "0222a86147299d4dee90edf95b6d13827edb42ad8b7c9cf0b2582f030937c0b5",
    },
    File {
        path: "parakeet/joiner.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/joiner.int8.onnx",
        size: 6_355_277,
        sha256: "cbd02f3696e9944970111fb439d9c3fd365a80374565f13f2b9c7007d253fc98",
    },
    File {
        path: "parakeet/tokens.txt",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/tokens.txt",
        size: 93_939,
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    },
    // Kyutai's Pocket TTS, CC-BY-4.0 with Kyutai's conditions of use, in sherpa-onnx's package (its vocabulary
    // is only there): unpacked into pocket/ (`POCKET_FILES`), the archive not kept.
    File {
        path: POCKET_ARCHIVE,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2",
        size: 98_337_865,
        sha256: "f4da7c7bd465f7b91e89191487fe90504396a51d6f0bcf94d0787633e2657293",
    },
];

/// Whisper turbo, MIT: asked only when Parakeet hears nothing in speech (V7). About 1 GB, so not downloaded
/// unless the person asks for it.
pub const WHISPER: &[File] = &[
    File {
        path: "whisper/turbo-encoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-encoder.int8.onnx",
        size: 674_716_297,
        sha256: "b02dcdf54f348741e93fe732b67d933c8dcb6735655f710640143081db38878b",
    },
    File {
        path: "whisper/turbo-decoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-decoder.int8.onnx",
        size: 361_080_764,
        sha256: "20accd02388482eb3a46bd615631adfdc85e1eb2c7db9ea3f02a40ffe6b81547",
    },
    File {
        path: "whisper/turbo-tokens.txt",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-tokens.txt",
        size: 816_730,
        sha256: "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126",
    },
];

const POCKET_ARCHIVE: &str = "pocket.tar.bz2";
/// What Pocket TTS needs out of its package.
pub const POCKET_FILES: &[&str] = &["lm_flow.int8.onnx", "lm_main.int8.onnx", "encoder.onnx", "decoder.int8.onnx", "text_conditioner.onnx", "vocab.json", "token_scores.json"];

/// The voices Demido speaks with: recordings donated to Kyutai for this use, CC0 (`kyutai/tts-voices`,
/// voice-donations). Only voices whose speakers agreed; never one made from a recording of someone else.
pub struct VoiceFile {
    pub id: &'static str,
    pub file: File,
}
const VOICE_SIZE: u64 = 640_044;
pub const VOICES: &[VoiceFile] = &[
    VoiceFile {
        id: "2181",
        file: File {
            path: "voices/2181.wav",
            url: concat!("https://huggingface.co/kyutai/tts-voices/resolve/main/voice-donations/", "2181_enhanced.wav"),
            size: VOICE_SIZE,
            sha256: "d97bf4947a527b4210f7c32dc69975416189bd74dd6706d2b04b515b2295c121",
        },
    },
    VoiceFile {
        id: "1410",
        file: File {
            path: "voices/1410.wav",
            url: concat!("https://huggingface.co/kyutai/tts-voices/resolve/main/voice-donations/", "1410_enhanced.wav"),
            size: VOICE_SIZE,
            sha256: "e566765a7053fbdcb113bd9ba104bfe186c4321a2e243c598380cdc7af01213d",
        },
    },
    VoiceFile {
        id: "1dd0",
        file: File {
            path: "voices/1dd0.wav",
            url: concat!("https://huggingface.co/kyutai/tts-voices/resolve/main/voice-donations/", "1dd0_enhanced.wav"),
            size: VOICE_SIZE,
            sha256: "cbbedbb763e89feb5d231ca7e34fee61f0bc44277bfd866c9485deb1a39e42f6",
        },
    },
    VoiceFile {
        id: "0a67",
        file: File {
            path: "voices/0a67.wav",
            url: concat!("https://huggingface.co/kyutai/tts-voices/resolve/main/voice-donations/", "0a67_enhanced.wav"),
            size: VOICE_SIZE,
            sha256: "f3d11f89eb6ee77ca0ab1ceed734755d0fff3d2791050cb4fe20428e0f6a76b3",
        },
    },
];

/// How far a download has got.
#[derive(Clone, Debug, Default, PartialEq, Serialize, ToSchema)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// The file being downloaded now.
    pub file: String,
}

/// What voice mode has, and what it lacks.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Status {
    /// Everything it needs is there: it can be used.
    pub ready: bool,
    /// What is still to download, in bytes.
    pub missing_bytes: u64,
    /// Whisper turbo is there, for when Parakeet hears nothing.
    pub whisper: bool,
    pub whisper_bytes: u64,
    /// The voices, those downloaded marked.
    pub voices: Vec<VoiceStatus>,
    /// A download under way.
    pub downloading: Option<Progress>,
    /// Why voice cannot be used here, when it cannot (the program to run it is not found).
    pub problem: Option<String>,
    /// Where its files are kept.
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct VoiceStatus {
    pub id: String,
    pub present: bool,
}

pub struct Voice {
    dir: PathBuf,
    /// `demido`, which runs `demido voice-engine`. `None`: voice cannot run (a test, or a program of another name).
    program: Option<PathBuf>,
    events: Arc<EventLog>,
    downloading: Mutex<Option<Progress>>,
}

impl Voice {
    pub fn new(dir: PathBuf, program: Option<PathBuf>, events: Arc<EventLog>) -> Self {
        let program = program.filter(|p| p.file_stem().is_some_and(|s| s == "demido"));
        Self { dir, program, events, downloading: Mutex::default() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn program(&self) -> Option<&Path> {
        self.program.as_deref()
    }

    fn present(&self, f: &File) -> bool {
        if f.path == POCKET_ARCHIVE {
            return POCKET_FILES.iter().all(|n| self.dir.join("pocket").join(n).is_file());
        }
        std::fs::metadata(self.dir.join(f.path)).is_ok_and(|m| m.len() == f.size)
    }

    pub fn voice_path(&self, id: &str) -> Option<PathBuf> {
        VOICES.iter().find(|v| v.id == id).map(|v| self.dir.join(v.file.path))
    }

    fn needed(&self, whisper: bool) -> Vec<File> {
        let mut all: Vec<File> = FILES.to_vec();
        all.extend(VOICES.iter().map(|v| v.file.clone()));
        if whisper {
            all.extend(WHISPER.iter().cloned());
        }
        all
    }

    pub fn status(&self) -> Status {
        let missing: u64 = self.needed(false).iter().filter(|f| !self.present(f)).map(|f| f.size).sum();
        let whisper = WHISPER.iter().all(|f| self.present(f));
        let first_voice = VOICES.iter().any(|v| self.present(&v.file));
        Status {
            ready: missing == 0 && first_voice && self.program.is_some(),
            missing_bytes: missing,
            whisper,
            whisper_bytes: WHISPER.iter().filter(|f| !self.present(f)).map(|f| f.size).sum(),
            voices: VOICES.iter().map(|v| VoiceStatus { id: v.id.into(), present: self.present(&v.file) }).collect(),
            downloading: self.downloading.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            problem: self.program.is_none().then(|| "Voice runs in Demido's own program (demido voice-engine), and it was not found beside this server.".into()),
            folder: self.dir.display().to_string(),
        }
    }

    /// Downloads what voice mode needs and does not have, Whisper too when asked. One download at a time.
    pub async fn download(&self, whisper: bool) -> Result<()> {
        let files: Vec<File> = self.needed(whisper).into_iter().filter(|f| !self.present(f)).collect();
        self.download_these(&files).await
    }

    pub(crate) async fn download_these(&self, files: &[File]) -> Result<()> {
        {
            let mut d = self.downloading.lock().unwrap_or_else(|e| e.into_inner());
            if d.is_some() {
                return Err(Error::Busy("Voice's files are being downloaded already.".into()));
            }
            *d = Some(Progress { done: 0, total: files.iter().map(|f| f.size).sum(), file: String::new() });
        }
        let outcome = self.fetch_all(files).await;
        *self.downloading.lock().unwrap_or_else(|e| e.into_inner()) = None;
        match &outcome {
            Ok(()) => self.events.append("voice.downloaded", None, json!({ "files": files.len() }))?,
            Err(e) => self.events.append("voice.download_failed", None, json!({ "problem": e.to_string() }))?,
        };
        outcome
    }

    async fn fetch_all(&self, files: &[File]) -> Result<()> {
        let client = reqwest::Client::builder().user_agent(crate::web::name()).build().map_err(|e| Error::Other(e.to_string()))?;
        let mut done_before = 0u64;
        for f in files {
            let to = self.dir.join(f.path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            self.fetch_one(&client, f, &to, done_before).await?;
            if f.path == POCKET_ARCHIVE {
                let (archive, into) = (to.clone(), self.dir.join("pocket"));
                tokio::task::spawn_blocking(move || unpack_pocket(&archive, &into)).await.map_err(|e| Error::Other(e.to_string()))??;
                let _ = std::fs::remove_file(&to);
            }
            done_before += f.size;
        }
        Ok(())
    }

    /// One file, to `<to>.part` and moved into place only once its sum is the one its makers publish.
    async fn fetch_one(&self, client: &reqwest::Client, f: &File, to: &Path, done_before: u64) -> Result<()> {
        let part = to.with_extension(format!("{}part", to.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()));
        let mut response = client.get(f.url).send().await.map_err(|e| Error::Other(format!("{} could not be downloaded: {e}", f.path)))?;
        if !response.status().is_success() {
            return Err(Error::Other(format!("{} could not be downloaded: the site answered {}", f.path, response.status())));
        }
        let mut file = tokio::fs::File::create(&part).await?;
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| Error::Other(format!("{} stopped downloading: {e}", f.path)))? {
            got += chunk.len() as u64;
            if got > f.size {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(Error::Other(format!("{} is larger than its makers say it is, so it was not kept", f.path)));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            let mut d = self.downloading.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = d.as_mut() {
                p.done = done_before + got;
                p.file = f.path.to_string();
            }
        }
        file.flush().await?;
        drop(file);
        let sum: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if sum != f.sha256 || got != f.size {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::Other(format!("{} is not the file its makers published (its sum differs), so it was not kept", f.path)));
        }
        tokio::fs::rename(&part, to).await?;
        Ok(())
    }
}

/// Takes what Pocket TTS needs out of sherpa-onnx's package, whatever folder it sits in inside the archive.
fn unpack_pocket(archive: &Path, into: &Path) -> Result<()> {
    std::fs::create_dir_all(into)?;
    let reader = bzip2::read::BzDecoder::new(std::io::BufReader::new(std::fs::File::open(archive)?));
    let mut tar = tar::Archive::new(reader);
    let mut found = 0;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if POCKET_FILES.contains(&name.as_str()) && entry.header().entry_type().is_file() {
            entry.unpack(into.join(&name))?;
            found += 1;
        }
    }
    if found < POCKET_FILES.len() {
        return Err(Error::Other("Pocket TTS's package does not hold the files it should".into()));
    }
    Ok(())
}

/// Spoken words, without what a page shows and a voice should not say: Markdown's marks, a link's address,
/// a code block.
pub fn as_spoken(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let t = t.trim_start_matches('#').trim_start();
        let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
        let mut l = String::new();
        let mut rest = t;
        // [text](address) says its text.
        while let Some(open) = rest.find('[') {
            match (rest[open..].find("](").map(|i| open + i), rest[open..].find(')').map(|i| open + i)) {
                (Some(mid), Some(close)) if close > mid => {
                    l.push_str(&rest[..open]);
                    l.push_str(&rest[open + 1..mid]);
                    rest = &rest[close + 1..];
                }
                _ => break,
            }
        }
        l.push_str(rest);
        let l = l.replace("**", "").replace("__", "").replace(['`', '*'], "");
        if !l.trim().is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(l.trim());
        }
    }
    out
}

/// Where the first finished sentence of `text` ends: after `.`, `!`, `?` or a line's end, followed by space.
/// A number's point (3.5) and a short abbreviation (e.g.) do not end one.
pub fn sentence_end(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if c == '\n' && text[..i].trim().len() > 1 {
            return Some(i + 1);
        }
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let next = chars.get(k + 1).map(|&(_, n)| n);
        if !next.is_some_and(char::is_whitespace) {
            continue;
        }
        // "e.g." and "Dr." and the like: the word before is a letter or two.
        let word: String = chars[..k].iter().rev().map(|&(_, c)| c).take_while(|c| c.is_alphabetic() || *c == '.').collect();
        if c == '.' && word.chars().filter(|c| c.is_alphabetic()).count() <= 2 && !word.is_empty() && word.chars().any(|c| c == '.') {
            continue;
        }
        if text[..i].trim().len() < 2 {
            continue;
        }
        return Some(i + c.len_utf8());
    }
    None
}

/// What the person heard of an answer cut short: the sentences heard whole, and of the one cut short the part
/// in proportion to its time (V3).
pub fn heard_of(sentences: &[(String, u64)], cut_at: usize, played_samples: u64) -> String {
    let mut heard: Vec<String> = sentences.iter().take(cut_at).map(|(s, _)| s.clone()).collect();
    if let Some((text, samples)) = sentences.get(cut_at) {
        let words: Vec<&str> = text.split_whitespace().collect();
        let share = if *samples == 0 { 0.0 } else { (played_samples as f64 / *samples as f64).clamp(0.0, 1.0) };
        let n = (words.len() as f64 * share).floor() as usize;
        if n > 0 {
            heard.push(format!("{}…", words[..n].join(" ")));
        }
    }
    heard.join(" ")
}

/// A short check that the program runs at all, for the status: `demido voice-engine --help`.
pub fn engine_args(dir: &Path, voice: &Path, threads: u32) -> Vec<String> {
    vec!["voice-engine".into(), "--dir".into(), dir.display().to_string(), "--voice".into(), voice.display().to_string(), "--threads".into(), threads.to_string()]
}

pub(crate) fn event_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(json!({ "event": "error", "message": "the engine sent something that is not JSON" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_spoken_without_its_markdown() {
        assert_eq!(as_spoken("## Rome\n**Sunny**, 19 °C. See [the forecast](https://example.org/rome).\n```\ncode\n```\n- Wind: light"), "Rome Sunny, 19 °C. See the forecast. Wind: light");
        assert_eq!(as_spoken("Use `cargo test` to run *them*."), "Use cargo test to run them.");
    }

    #[test]
    fn sentences_end_where_a_person_would_hear_them_end() {
        assert_eq!(sentence_end("It is sunny. Tomorrow"), Some("It is sunny.".len()));
        assert_eq!(sentence_end("Is it? Yes"), Some("Is it?".len()));
        assert_eq!(sentence_end("It costs 3.5 euros"), None, "a number's point");
        assert_eq!(sentence_end("Bring e.g. water"), None, "an abbreviation");
        assert_eq!(sentence_end("A line\nAnother"), Some("A line\n".len()));
        assert_eq!(sentence_end("Not yet finished"), None);
        assert_eq!(sentence_end("Done."), None, "the end of the text is not yet the end of a sentence: more may come");
    }

    #[test]
    fn what_was_heard_of_a_cut_answer_is_what_was_played() {
        let sentences = vec![("It is sunny in Rome.".to_string(), 48_000), ("Tomorrow it rains all day long.".to_string(), 48_000)];
        assert_eq!(heard_of(&sentences, 1, 24_000), "It is sunny in Rome. Tomorrow it rains…", "half of the second sentence's time");
        assert_eq!(heard_of(&sentences, 0, 0), "", "cut before a word");
        assert_eq!(heard_of(&sentences, 2, 0), "It is sunny in Rome. Tomorrow it rains all day long.");
    }

    #[tokio::test]
    async fn a_file_is_kept_only_when_its_sum_is_its_makers() {
        let home = crate::testing::TempDir::new("demido-voice-download");
        let events = Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap())));
        let voice = Voice::new(home.join("voice"), None, events.clone());
        // A stand-in for the makers' site: one file, served as it is.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route("/vad.onnx", axum::routing::get(|| async { "the model" }));
            axum::serve(listener, app).await.unwrap();
        });
        let url: &'static str = Box::leak(format!("http://{at}/vad.onnx").into_boxed_str());
        let right = "8b2d6c1f6e3a4a6c5b1e9b79f2f0a5d0d9a9f6bd0a0b5d8e0bb7f0e9f9c9b2a1";
        let sum: String = Sha256::digest(b"the model").iter().map(|b| format!("{b:02x}")).collect();
        assert_ne!(sum, right);
        let wrong = File { path: "silero_vad.onnx", url, size: 9, sha256: right };
        assert!(voice.download_these(&[wrong]).await.is_err());
        assert!(!home.join("voice/silero_vad.onnx").exists() && !home.join("voice/silero_vad.onnx.part").exists(), "a file whose sum differs is not kept");
        let sum: &'static str = Box::leak(sum.into_boxed_str());
        let good = File { path: "silero_vad.onnx", url, size: 9, sha256: sum };
        voice.download_these(&[good]).await.unwrap();
        assert_eq!(std::fs::read_to_string(home.join("voice/silero_vad.onnx")).unwrap(), "the model");
        let status = voice.status();
        assert!(!status.ready && status.missing_bytes > 0 && status.problem.is_some(), "the rest is missing, and there is no program here");
        assert!(events.after(0, 50).unwrap().iter().any(|e| e.kind == "voice.download_failed") && events.after(0, 50).unwrap().iter().any(|e| e.kind == "voice.downloaded"));
    }
}
