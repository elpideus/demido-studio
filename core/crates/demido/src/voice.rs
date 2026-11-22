//! `demido voice-engine`: the speech parts of voice mode, in a process of their own (DECISIONS V7; research
//! note 28). The core starts it when voice mode opens and talks to it over standard input and output; a crash
//! in a speech engine cannot take the server down, and the core links none of them.
//!
//! What it does, on the processor:
//! - hears: Silero VAD finds speech; after 200 ms of silence Smart Turn v3.2 judges whether the sentence is
//!   finished (DECISIONS V2: when it is not, the turn goes on, with no time limit); a finished turn is
//!   written down by Parakeet TDT 0.6B v3, and by Whisper turbo when Parakeet hears nothing and Whisper is there;
//! - speaks: Pocket TTS, in a donated voice, a sentence at a time, sent as it is made, and stopped at once
//!   when the person speaks over it (V3) or the core says so.
//!
//! Frames, both ways: one byte for the kind, four for the length (little-endian), then the payload.
//! In:  `a` sound (16 kHz, mono, 16-bit), `s` a sentence to say (its number, four bytes, then UTF-8),
//!      `x` stop saying, `n` answer now (the turn is over whatever Smart Turn says), `p` whether the window is
//!      playing Demido's voice (one byte), `q` quit.
//! Out: `e` an event (JSON), `o` sound (the sentence's number, four bytes, then 24 kHz mono 16-bit).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

pub mod features;

const RATE: i32 = 16_000;
/// The silence after speech before Smart Turn is asked (research note 28).
const PAUSE_S: f32 = 0.2;
/// The most a turn holds: Smart Turn reads its last 8 seconds, Parakeet all of it.
const TURN_AT_MOST_S: usize = 60;

/// Where the engine's files are, as the core downloaded them (`demido_core::voice::Files`).
pub struct Paths {
    pub dir: PathBuf,
    pub voice: PathBuf,
    pub threads: i32,
}

impl Paths {
    fn at(&self, rel: &str) -> String {
        self.dir.join(rel).display().to_string()
    }
    fn whisper(&self) -> Option<(String, String, String)> {
        let enc = self.dir.join("whisper/turbo-encoder.int8.onnx");
        enc.is_file().then(|| (enc.display().to_string(), self.at("whisper/turbo-decoder.int8.onnx"), self.at("whisper/turbo-tokens.txt")))
    }
}

enum Out {
    Event(Value),
    Sound(u32, Vec<i16>),
}

fn frame(out: &mut impl Write, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    out.write_all(&[kind])?;
    out.write_all(&(payload.len() as u32).to_le_bytes())?;
    out.write_all(payload)?;
    out.flush()
}

/// Reads one frame. `None` at the end of the input.
fn read_frame(input: &mut impl Read) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match input.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "a frame larger than 16 MB"));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    Ok(Some((head[0], payload)))
}

fn wav_mono(path: &Path) -> Result<(Vec<f32>, i32)> {
    if !path.is_file() {
        return Err(anyhow!("there is no voice at {}", path.display()));
    }
    let wave = sherpa_onnx::Wave::read(&path.display().to_string()).ok_or_else(|| anyhow!("{} is not a WAV file this engine reads", path.display()))?;
    Ok((wave.samples().to_vec(), wave.sample_rate()))
}

/// Smart Turn v3.2, through `ort` on the onnxruntime sherpa-onnx brings: whether the speaker has finished.
pub struct SmartTurn {
    session: ort::session::Session,
    _environment: ort::environment::Environment,
}

impl SmartTurn {
    pub fn open(model: &Path, threads: i32) -> Result<Self> {
        let environment = ort::init().with_name("demido-smart-turn").build().map_err(|e| anyhow!("onnxruntime could not be started: {e}"))?;
        let session = ort::session::Session::builder(&environment)
            .map_err(|e| anyhow!("{e}"))?
            .with_intra_threads(threads.max(1) as usize)
            .map_err(|e| anyhow!("{e}"))?
            .with_inter_threads(1)
            .map_err(|e| anyhow!("{e}"))?
            .commit_from_file(model)
            .map_err(|e| anyhow!("Smart Turn could not be opened: {e}"))?;
        Ok(Self { session, _environment: environment })
    }

    /// The probability that the sentence is finished, from 16 kHz speech that ends where the person stopped.
    pub fn finished(&mut self, speech: &[f32]) -> Result<f32> {
        let mut heard = speech.to_vec();
        heard.extend(std::iter::repeat_n(0.0, (PAUSE_S * RATE as f32) as usize));
        let input = features::smart_turn_input(&heard);
        let tensor = ort::value::Tensor::from_array(([1usize, features::MELS, features::FRAMES], input)).map_err(|e| anyhow!("{e}"))?;
        let outputs = self.session.run(ort::inputs!["input_features" => tensor]).map_err(|e| anyhow!("Smart Turn failed: {e}"))?;
        let (_, probability) = outputs[0].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
        Ok(probability.first().copied().unwrap_or(1.0))
    }
}

fn recognizer(transducer: Option<(String, String, String, String)>, whisper: Option<(String, String, String)>, threads: i32) -> Result<sherpa_onnx::OfflineRecognizer> {
    let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
    config.model_config.num_threads = threads;
    config.decoding_method = Some("greedy_search".into());
    if let Some((encoder, decoder, joiner, tokens)) = transducer {
        config.model_config.transducer.encoder = Some(encoder);
        config.model_config.transducer.decoder = Some(decoder);
        config.model_config.transducer.joiner = Some(joiner);
        config.model_config.tokens = Some(tokens);
        config.model_config.model_type = Some("nemo_transducer".into());
    } else if let Some((encoder, decoder, tokens)) = whisper {
        config.model_config.whisper.encoder = Some(encoder);
        config.model_config.whisper.decoder = Some(decoder);
        config.model_config.whisper.language = Some("en".into());
        config.model_config.whisper.task = Some("transcribe".into());
        config.model_config.tokens = Some(tokens);
    }
    sherpa_onnx::OfflineRecognizer::create(&config).ok_or_else(|| anyhow!("a speech-to-text model could not be opened"))
}

fn transcribe(recognizer: &sherpa_onnx::OfflineRecognizer, speech: &[f32]) -> String {
    let stream = recognizer.create_stream();
    stream.accept_waveform(RATE, speech);
    recognizer.decode(&stream);
    stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
}

/// Hearing: the speech detector, the turn, and writing a finished turn down.
fn hear(paths: &Paths, sounds: mpsc::Receiver<Hear>, out: mpsc::Sender<Out>, playing: Arc<AtomicBool>) -> Result<()> {
    let mut vad_config = sherpa_onnx::VadModelConfig::default();
    vad_config.silero_vad.model = Some(paths.at("silero_vad.onnx"));
    vad_config.silero_vad.threshold = 0.5;
    vad_config.silero_vad.min_silence_duration = PAUSE_S;
    vad_config.silero_vad.min_speech_duration = 0.25;
    vad_config.silero_vad.window_size = 512;
    vad_config.silero_vad.max_speech_duration = 30.0;
    vad_config.sample_rate = RATE;
    vad_config.num_threads = 1;
    let vad = sherpa_onnx::VoiceActivityDetector::create(&vad_config, TURN_AT_MOST_S as f32).ok_or_else(|| anyhow!("the speech detector could not be opened"))?;
    let mut turn = SmartTurn::open(&paths.dir.join("smart-turn-v3.2-cpu.onnx"), paths.threads)?;
    let parakeet = recognizer(
        Some((paths.at("parakeet/encoder.int8.onnx"), paths.at("parakeet/decoder.int8.onnx"), paths.at("parakeet/joiner.int8.onnx"), paths.at("parakeet/tokens.txt"))),
        None,
        paths.threads,
    )?;
    let whisper = match paths.whisper() {
        Some(w) => Some(recognizer(None, Some(w), paths.threads)?),
        None => None,
    };
    let _ = out.send(Out::Event(json!({ "event": "listening", "whisper": whisper.is_some() })));
    let mut pending: Vec<f32> = Vec::new();
    let mut said: Vec<f32> = Vec::new();
    let mut speaking = false;
    let end_turn = |said: &mut Vec<f32>, out: &mpsc::Sender<Out>, forced: bool| {
        if said.is_empty() {
            if forced {
                let _ = out.send(Out::Event(json!({ "event": "heard_nothing" })));
            }
            return;
        }
        let started = std::time::Instant::now();
        let mut text = transcribe(&parakeet, said);
        let mut by = "parakeet";
        if text.is_empty()
            && let Some(w) = &whisper
        {
            text = transcribe(w, said);
            by = "whisper";
        }
        let ms = started.elapsed().as_millis() as u64;
        let seconds = said.len() as f32 / RATE as f32;
        said.clear();
        let _ = out.send(Out::Event(if text.is_empty() {
            json!({ "event": "heard_nothing", "seconds": seconds, "ms": ms })
        } else {
            json!({ "event": "heard", "text": text, "by": by, "seconds": seconds, "ms": ms })
        }));
    };
    for message in sounds {
        match message {
            Hear::AnswerNow => {
                // Speech still under way counts too.
                if vad.detected() {
                    vad.flush();
                    while let Some(segment) = vad.front() {
                        said.extend_from_slice(segment.samples());
                        vad.pop();
                    }
                }
                end_turn(&mut said, &out, true);
            }
            Hear::Sound(samples) => {
                pending.extend(samples);
                while pending.len() >= 512 {
                    let window: Vec<f32> = pending.drain(..512).collect();
                    vad.accept_waveform(&window);
                    let now = vad.detected();
                    if now && !speaking {
                        let over = playing.load(Ordering::Relaxed);
                        let _ = out.send(Out::Event(json!({ "event": "speech", "over_demido": over })));
                    }
                    speaking = now;
                    while let Some(segment) = vad.front() {
                        said.extend_from_slice(segment.samples());
                        vad.pop();
                        let keep = TURN_AT_MOST_S * RATE as usize;
                        if said.len() > keep {
                            said.drain(..said.len() - keep);
                        }
                        let started = std::time::Instant::now();
                        let p = turn.finished(&said).unwrap_or(1.0);
                        let ms = started.elapsed().as_millis() as u64;
                        if p > 0.5 {
                            let _ = out.send(Out::Event(json!({ "event": "finished", "probability": p, "ms": ms })));
                            end_turn(&mut said, &out, false);
                        } else {
                            let _ = out.send(Out::Event(json!({ "event": "waiting", "probability": p, "ms": ms })));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

enum Hear {
    Sound(Vec<f32>),
    AnswerNow,
}

struct Say {
    sentence: u32,
    text: String,
    epoch: u32,
}

/// Speaking: Pocket TTS, a sentence at a time, its sound sent as it is made.
fn speak(paths: &Paths, sentences: mpsc::Receiver<Say>, out: mpsc::Sender<Out>, epoch: Arc<AtomicU32>) -> Result<()> {
    let mut config = sherpa_onnx::OfflineTtsConfig::default();
    config.model.pocket.lm_flow = Some(paths.at("pocket/lm_flow.int8.onnx"));
    config.model.pocket.lm_main = Some(paths.at("pocket/lm_main.int8.onnx"));
    config.model.pocket.encoder = Some(paths.at("pocket/encoder.onnx"));
    config.model.pocket.decoder = Some(paths.at("pocket/decoder.int8.onnx"));
    config.model.pocket.text_conditioner = Some(paths.at("pocket/text_conditioner.onnx"));
    config.model.pocket.vocab_json = Some(paths.at("pocket/vocab.json"));
    config.model.pocket.token_scores_json = Some(paths.at("pocket/token_scores.json"));
    config.model.pocket.voice_embedding_cache_capacity = 4;
    config.model.num_threads = paths.threads;
    let tts = sherpa_onnx::OfflineTts::create(&config).ok_or_else(|| anyhow!("Pocket TTS could not be opened"))?;
    let (voice, voice_rate) = wav_mono(&paths.voice)?;
    let rate = tts.sample_rate();
    let _ = out.send(Out::Event(json!({ "event": "voice", "sample_rate": rate })));
    for say in sentences {
        if say.epoch != epoch.load(Ordering::Relaxed) {
            continue; // said before a stop: not said
        }
        let generation = sherpa_onnx::GenerationConfig { speed: 1.0, reference_audio: Some(voice.clone()), reference_sample_rate: voice_rate, ..Default::default() };
        let (sentence, mine) = (say.sentence, say.epoch);
        let (out_chunks, now) = (out.clone(), epoch.clone());
        let started = std::time::Instant::now();
        let first = Arc::new(AtomicBool::new(true));
        let first_seen = first.clone();
        let sent_samples = Arc::new(AtomicU32::new(0));
        let counted = sent_samples.clone();
        let callback = move |samples: &[f32], _progress: f32| -> bool {
            if now.load(Ordering::Relaxed) != mine {
                return false; // stopped: the person spoke, or the core said so
            }
            if first_seen.swap(false, Ordering::Relaxed) {
                let _ = out_chunks.send(Out::Event(json!({ "event": "speaking", "sentence": sentence, "first_ms": started.elapsed().as_millis() as u64 })));
            }
            let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
            counted.fetch_add(pcm.len() as u32, Ordering::Relaxed);
            let _ = out_chunks.send(Out::Sound(sentence, pcm));
            true
        };
        let made = tts.generate_with_config(&say.text, &generation, Some(callback));
        if epoch.load(Ordering::Relaxed) != mine {
            continue;
        }
        let samples = sent_samples.load(Ordering::Relaxed);
        // Sound the callback did not hand over (the end of a sentence) goes out now.
        if let Some(audio) = made {
            let all = audio.samples();
            if all.len() > samples as usize {
                let pcm: Vec<i16> = all[samples as usize..].iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                let _ = out.send(Out::Sound(sentence, pcm));
            }
            let _ = out.send(Out::Event(json!({ "event": "said", "sentence": sentence, "samples": all.len(), "ms": started.elapsed().as_millis() as u64 })));
        } else {
            let _ = out.send(Out::Event(json!({ "event": "said", "sentence": sentence, "samples": samples, "failed": true })));
        }
    }
    Ok(())
}

/// The engine: reads frames until the input ends or `q`.
pub fn run(paths: Paths) -> Result<()> {
    let (out_tx, out_rx) = mpsc::channel::<Out>();
    let writer = std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        for message in out_rx {
            let ok = match message {
                Out::Event(e) => frame(&mut stdout, b'e', e.to_string().as_bytes()),
                Out::Sound(sentence, pcm) => {
                    let mut payload = sentence.to_le_bytes().to_vec();
                    payload.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
                    frame(&mut stdout, b'o', &payload)
                }
            };
            if ok.is_err() {
                break;
            }
        }
    });
    let paths = Arc::new(paths);
    let playing = Arc::new(AtomicBool::new(false));
    let epoch = Arc::new(AtomicU32::new(0));
    let (hear_tx, hear_rx) = mpsc::channel::<Hear>();
    let (say_tx, say_rx) = mpsc::channel::<Say>();
    let hearing = {
        let (paths, out, playing) = (paths.clone(), out_tx.clone(), playing.clone());
        std::thread::spawn(move || {
            if let Err(e) = hear(&paths, hear_rx, out.clone(), playing) {
                let _ = out.send(Out::Event(json!({ "event": "error", "part": "hearing", "message": format!("{e:#}") })));
            }
        })
    };
    let speaking = {
        let (paths, out, epoch) = (paths.clone(), out_tx.clone(), epoch.clone());
        std::thread::spawn(move || {
            if let Err(e) = speak(&paths, say_rx, out.clone(), epoch) {
                let _ = out.send(Out::Event(json!({ "event": "error", "part": "speaking", "message": format!("{e:#}") })));
            }
        })
    };
    let mut stdin = std::io::stdin().lock();
    while let Some((kind, payload)) = read_frame(&mut stdin)? {
        match kind {
            b'a' => {
                let samples: Vec<f32> = payload.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
                if hear_tx.send(Hear::Sound(samples)).is_err() {
                    break;
                }
            }
            b's' if payload.len() >= 4 => {
                let sentence = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let text = String::from_utf8_lossy(&payload[4..]).to_string();
                let _ = say_tx.send(Say { sentence, text, epoch: epoch.load(Ordering::Relaxed) });
            }
            b'x' => {
                epoch.fetch_add(1, Ordering::Relaxed);
            }
            b'n' => {
                let _ = hear_tx.send(Hear::AnswerNow);
            }
            b'p' => playing.store(payload.first().copied() == Some(1), Ordering::Relaxed),
            b'q' => break,
            _ => {}
        }
    }
    drop(hear_tx);
    drop(say_tx);
    epoch.fetch_add(1, Ordering::Relaxed);
    let _ = hearing.join();
    let _ = speaking.join();
    drop(out_tx);
    let _ = writer.join();
    Ok(())
}
