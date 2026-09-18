//! Real GGUF files, small enough to write in a test.
//!
//! A library is only interesting against real bytes: a header that parses, a
//! tensor table that says how long the file must be, and a file that is or is
//! not that long. So the tests write the format rather than a placeholder, and
//! a truncated model is a real model with its tail cut off.

#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What goes into one file.
#[derive(Default, Clone)]
pub struct Spec {
    pub architecture: Option<&'static str>,
    pub context: Option<u32>,
    pub template: Option<&'static str>,
    /// `clip.has_vision_encoder` and `clip.has_audio_encoder`, for a projector.
    pub sees: Option<bool>,
    pub hears: Option<bool>,
    /// How many F32 values of tensor data the file carries.
    pub weights: u64,
    /// Any other metadata, written after the architecture: what an attention
    /// geometry is read from.
    pub keys: Vec<(String, Key)>,
}

/// A metadata value beyond the ones `Spec` names.
#[derive(Clone)]
pub enum Key {
    Uint(u32),
    /// An array of `u32`, one per layer.
    Uints(Vec<u32>),
    /// An array of booleans, one per layer.
    Flags(Vec<bool>),
}

impl Spec {
    pub fn model(architecture: &'static str) -> Self {
        Spec {
            architecture: Some(architecture),
            context: Some(8192),
            weights: 64,
            ..Spec::default()
        }
    }
}

fn string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u64).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn key(out: &mut Vec<u8>, name: &str, kind: u32) {
    string(out, name);
    out.extend_from_slice(&kind.to_le_bytes());
}

/// Write a GGUF at `path`, making its folder, and return how long it is.
pub fn write(path: &Path, spec: &Spec) -> u64 {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("made the folder");
    let mut out = Vec::new();
    out.extend_from_slice(b"GGUF");
    out.extend_from_slice(&3u32.to_le_bytes());
    let tensors: u64 = u64::from(spec.weights > 0);
    out.extend_from_slice(&tensors.to_le_bytes());

    let mut keys = Vec::new();
    let mut count = 0u64;
    if let Some(architecture) = spec.architecture {
        key(&mut keys, "general.architecture", 8);
        string(&mut keys, architecture);
        count += 1;
        if let Some(context) = spec.context {
            key(&mut keys, &format!("{architecture}.context_length"), 4);
            keys.extend_from_slice(&context.to_le_bytes());
            count += 1;
        }
    }
    for (name, value) in &spec.keys {
        match value {
            Key::Uint(number) => {
                key(&mut keys, name, 4);
                keys.extend_from_slice(&number.to_le_bytes());
            }
            Key::Uints(numbers) => {
                key(&mut keys, name, 9);
                keys.extend_from_slice(&4u32.to_le_bytes());
                keys.extend_from_slice(&(numbers.len() as u64).to_le_bytes());
                for number in numbers {
                    keys.extend_from_slice(&number.to_le_bytes());
                }
            }
            Key::Flags(flags) => {
                key(&mut keys, name, 9);
                keys.extend_from_slice(&7u32.to_le_bytes());
                keys.extend_from_slice(&(flags.len() as u64).to_le_bytes());
                keys.extend(flags.iter().map(|flag| u8::from(*flag)));
            }
        }
        count += 1;
    }
    // A tokenizer vocabulary, which is what a real header spends its bytes on
    // and what the reader has to step over.
    key(&mut keys, "tokenizer.ggml.tokens", 9);
    keys.extend_from_slice(&8u32.to_le_bytes());
    keys.extend_from_slice(&3u64.to_le_bytes());
    for token in ["<s>", "</s>", "hello"] {
        string(&mut keys, token);
    }
    count += 1;
    if let Some(template) = spec.template {
        key(&mut keys, "tokenizer.chat_template", 8);
        string(&mut keys, template);
        count += 1;
    }
    for (name, flag) in [
        ("clip.has_vision_encoder", spec.sees),
        ("clip.has_audio_encoder", spec.hears),
    ] {
        if let Some(flag) = flag {
            key(&mut keys, name, 7);
            keys.push(u8::from(flag));
            count += 1;
        }
    }
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&keys);

    if spec.weights > 0 {
        string(&mut out, "token_embd.weight");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&spec.weights.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // F32
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    while out.len() % 32 != 0 {
        out.push(0);
    }
    out.resize(out.len() + (spec.weights as usize) * 4, 0);
    std::fs::write(path, &out).expect("wrote the model");
    out.len() as u64
}

/// Cut a file short by `bytes`, the way a dropped connection leaves one.
pub fn truncate(path: &Path, bytes: u64) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("opened");
    let length = file.metadata().expect("read").len();
    file.set_len(length - bytes).expect("cut");
}

/// Everything under a folder: path, length, last modified. Two snapshots that
/// are equal mean nothing was written, created, moved or deleted.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut seen = BTreeMap::new();
    walk(root, &mut seen);
    seen
}

fn walk(folder: &Path, seen: &mut BTreeMap<PathBuf, (u64, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = entry.metadata().expect("read");
        if meta.is_dir() {
            seen.insert(path.clone(), (0, SystemTime::UNIX_EPOCH));
            walk(&path, seen);
        } else {
            seen.insert(path, (meta.len(), meta.modified().expect("a time")));
        }
    }
}
