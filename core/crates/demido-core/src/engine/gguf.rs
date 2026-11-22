//! What a GGUF file says about itself before its weights: its names, and the sampling settings its makers
//! put in it (research note 19). Only the first part of the file is read, and only up to the tokenizer's
//! long lists: a few kilobytes.

use std::io::{BufReader, Read};
use std::path::Path;

use super::sampling::Sampling;

/// A GGUF file's own names for itself, and its settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    /// `qwen3`, `qwen35`, `gemma4`, `granite`…
    pub architecture: Option<String>,
    /// "Qwen3", "gemma-4".
    pub basename: Option<String>,
    /// "1.7B".
    pub size_label: Option<String>,
    /// What it was trained for: "Instruct", "it", "Base".
    pub finetune: Option<String>,
    /// The repository of the model it was made from, when it says (`owner/name`).
    pub base_repo: Option<String>,
    /// `general.sampling.*`: what llama.cpp takes for itself when it is not told otherwise.
    pub sampling: Sampling,
}

/// The most read from a file before giving up on finding the end of its names: far beyond any real one.
const AT_MOST: u64 = 16 * 1024 * 1024;

enum Got {
    Text(String),
    Number(f64),
    Other,
}

fn take<const N: usize>(r: &mut impl Read) -> Option<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b).ok()?;
    Some(b)
}

fn u32_of(r: &mut impl Read) -> Option<u32> {
    take::<4>(r).map(u32::from_le_bytes)
}

fn u64_of(r: &mut impl Read) -> Option<u64> {
    take::<8>(r).map(u64::from_le_bytes)
}

fn text(r: &mut impl Read) -> Option<String> {
    let n = u64_of(r)?;
    if n > 1024 * 1024 {
        return None;
    }
    let mut b = vec![0u8; n as usize];
    r.read_exact(&mut b).ok()?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// The size of a fixed-size value of a GGUF type, in bytes.
fn width(kind: u32) -> Option<u64> {
    Some(match kind {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        _ => return None,
    })
}

/// A number of any of GGUF's number types.
fn number(r: &mut impl Read, kind: u32) -> Option<f64> {
    Some(match kind {
        0 | 7 => take::<1>(r)?[0] as f64,
        1 => take::<1>(r)?[0] as i8 as f64,
        2 => u16::from_le_bytes(take::<2>(r)?) as f64,
        3 => i16::from_le_bytes(take::<2>(r)?) as f64,
        4 => u32::from_le_bytes(take::<4>(r)?) as f64,
        5 => i32::from_le_bytes(take::<4>(r)?) as f64,
        6 => f32::from_le_bytes(take::<4>(r)?) as f64,
        10 => u64::from_le_bytes(take::<8>(r)?) as f64,
        11 => i64::from_le_bytes(take::<8>(r)?) as f64,
        12 => f64::from_le_bytes(take::<8>(r)?),
        _ => return None,
    })
}

fn value(r: &mut impl Read, kind: u32) -> Option<Got> {
    match kind {
        8 => text(r).map(Got::Text),
        9 => {
            let (inner, count) = (u32_of(r)?, u64_of(r)?);
            if count > 50_000_000 {
                return None;
            }
            for _ in 0..count {
                match inner {
                    8 => {
                        text(r)?;
                    }
                    _ => {
                        let w = width(inner)?;
                        std::io::copy(&mut (&mut *r).take(w), &mut std::io::sink()).ok()?;
                    }
                }
            }
            Some(Got::Other)
        }
        _ => number(r, kind).map(Got::Number),
    }
}

/// `https://huggingface.co/owner/name` as `owner/name`.
fn repo_of(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://huggingface.co/")?.trim_end_matches('/');
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

/// Reads a GGUF file's names and settings. `None` when it is not a GGUF file, or cannot be read.
pub fn read(path: &Path) -> Option<Header> {
    let file = std::fs::File::open(path).ok()?;
    let mut r = BufReader::new(file).take(AT_MOST);
    if &take::<4>(&mut r)? != b"GGUF" {
        return None;
    }
    let version = u32_of(&mut r)?;
    if !(2..=3).contains(&version) {
        return None;
    }
    let (_tensors, keys) = (u64_of(&mut r)?, u64_of(&mut r)?);
    let mut h = Header::default();
    for _ in 0..keys.min(100_000) {
        let key = text(&mut r)?;
        // The tokenizer's lists come next, and nothing wanted here comes after them.
        if key.starts_with("tokenizer.") {
            break;
        }
        let kind = u32_of(&mut r)?;
        let got = value(&mut r, kind)?;
        match (key.as_str(), got) {
            ("general.architecture", Got::Text(t)) => h.architecture = Some(t),
            ("general.basename", Got::Text(t)) => h.basename = Some(t),
            ("general.size_label", Got::Text(t)) => h.size_label = Some(t),
            ("general.finetune", Got::Text(t)) => h.finetune = Some(t),
            ("general.base_model.0.repo_url", Got::Text(t)) => h.base_repo = repo_of(&t),
            ("general.sampling.temp", Got::Number(n)) => h.sampling.temperature = Some(n),
            ("general.sampling.top_p", Got::Number(n)) => h.sampling.top_p = Some(n),
            ("general.sampling.top_k", Got::Number(n)) if n >= 0.0 => h.sampling.top_k = Some(n as u32),
            ("general.sampling.min_p", Got::Number(n)) => h.sampling.min_p = Some(n),
            ("general.sampling.penalty_repeat", Got::Number(n)) => h.sampling.repeat_penalty = Some(n),
            _ => {}
        }
    }
    Some(h)
}

/// Writes a small GGUF file with these string and float keys, and a tokenizer list after them. For tests.
#[cfg(test)]
pub(crate) fn write_for_test(path: &Path, texts: &[(&str, &str)], floats: &[(&str, f32)]) {
    let mut b: Vec<u8> = b"GGUF".to_vec();
    b.extend(3u32.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend(((texts.len() + floats.len() + 1) as u64).to_le_bytes());
    let key = |b: &mut Vec<u8>, k: &str| {
        b.extend((k.len() as u64).to_le_bytes());
        b.extend(k.as_bytes());
    };
    for (k, v) in texts {
        key(&mut b, k);
        b.extend(8u32.to_le_bytes());
        b.extend((v.len() as u64).to_le_bytes());
        b.extend(v.as_bytes());
    }
    for (k, v) in floats {
        key(&mut b, k);
        b.extend(6u32.to_le_bytes());
        b.extend(v.to_le_bytes());
    }
    key(&mut b, "tokenizer.ggml.tokens");
    b.extend(9u32.to_le_bytes());
    b.extend(8u32.to_le_bytes());
    b.extend(2u64.to_le_bytes());
    for t in ["a", "b"] {
        b.extend((t.len() as u64).to_le_bytes());
        b.extend(t.as_bytes());
    }
    std::fs::write(path, b).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_names_and_settings_are_read_and_its_lists_are_not() {
        let dir = std::env::temp_dir().join(format!("demido-gguf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gguf");
        write_for_test(
            &path,
            &[("general.architecture", "qwen3"), ("general.basename", "Qwen3"), ("general.size_label", "1.7B"), ("general.base_model.0.repo_url", "https://huggingface.co/Qwen/Qwen3-1.7B-Base")],
            &[("general.sampling.temp", 0.6), ("general.sampling.top_k", 20.0)],
        );
        let h = read(&path).unwrap();
        assert_eq!((h.architecture.as_deref(), h.basename.as_deref(), h.size_label.as_deref(), h.base_repo.as_deref()), (Some("qwen3"), Some("Qwen3"), Some("1.7B"), Some("Qwen/Qwen3-1.7B-Base")));
        assert_eq!((h.sampling.temperature.map(|t| (t * 100.0).round() / 100.0), h.sampling.top_k), (Some(0.6), Some(20)));
        std::fs::write(dir.join("not.gguf"), b"not a model at all").unwrap();
        assert_eq!(read(&dir.join("not.gguf")), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
