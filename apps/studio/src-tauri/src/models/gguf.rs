//! Reads the metadata header of a GGUF file: architecture, name, size label, quantization,
//! trained context length, pooling and tags. Only the key/value section before the tokenizer is read,
//! so this takes milliseconds even for multi-gigabyte files.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GgufInfo {
    pub architecture: Option<String>,
    pub name: Option<String>,
    pub size_label: Option<String>,
    pub file_type: Option<u32>,
    pub context_length: Option<u64>,
    pub quantized_by: Option<String>,
    /// How an embedding model pools its tokens into one vector (llama.cpp's
    /// `llama_pooling_type`: 1 mean, 2 cls, 3 last, 4 rank). Chat models have none.
    pub pooling_type: Option<u32>,
    /// `general.tags`: what the model is for, in Hugging Face's task names.
    pub tags: Vec<String>,
}

impl GgufInfo {
    /// An embedding or reranking model: it turns text into vectors or scores, never answers.
    pub fn is_embedding(&self) -> bool {
        self.pooling_type.is_some_and(|p| p > 0)
    }

    /// A speech recognition model (the speech model among them): it writes down what it hears,
    /// and cannot chat.
    pub fn is_speech_recognition(&self) -> bool {
        self.tags.iter().any(|t| t == "automatic-speech-recognition")
    }
}

const MAGIC: &[u8; 4] = b"GGUF";

pub fn read(path: &Path) -> anyhow::Result<GgufInfo> {
    let file = File::open(path)?;
    let mut r = BufReader::with_capacity(64 * 1024, file);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    anyhow::ensure!(&magic == MAGIC, "not a GGUF file");
    let version = read_u32(&mut r)?;
    anyhow::ensure!((2..=3).contains(&version), "unsupported GGUF version {version}");
    let _tensors = read_u64(&mut r)?;
    let kv_count = read_u64(&mut r)?;

    let mut info = GgufInfo::default();
    for _ in 0..kv_count.min(100_000) {
        let key = read_string(&mut r)?;
        let ty = read_u32(&mut r)?;
        if key.starts_with("tokenizer.") {
            // Everything we need precedes the (large) tokenizer section.
            break;
        }
        match key.as_str() {
            "general.architecture" if ty == 8 => info.architecture = Some(read_string(&mut r)?),
            "general.name" if ty == 8 => info.name = Some(read_string(&mut r)?),
            "general.size_label" if ty == 8 => info.size_label = Some(read_string(&mut r)?),
            "general.quantized_by" if ty == 8 => info.quantized_by = Some(read_string(&mut r)?),
            "general.file_type" => info.file_type = read_int(&mut r, ty)?.map(|v| v as u32),
            "general.tags" if ty == 9 => info.tags = read_strings(&mut r)?,
            k if k.ends_with(".context_length")
                && info
                    .architecture
                    .as_deref()
                    .is_some_and(|a| k == format!("{a}.context_length")) =>
            {
                info.context_length = read_int(&mut r, ty)?;
            }
            k if k.ends_with(".pooling_type")
                && info
                    .architecture
                    .as_deref()
                    .is_some_and(|a| k == format!("{a}.pooling_type")) =>
            {
                info.pooling_type = read_int(&mut r, ty)?.map(|v| v as u32);
            }
            _ => skip_value(&mut r, ty)?,
        }
    }
    Ok(info)
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_string(r: &mut impl Read) -> anyhow::Result<String> {
    let len = read_u64(r)?;
    anyhow::ensure!(len < 16 * 1024 * 1024, "string too long");
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Reads an array; its strings when it holds strings, nothing otherwise.
fn read_strings(r: &mut (impl Read + Seek)) -> anyhow::Result<Vec<String>> {
    let inner = read_u32(r)?;
    let count = read_u64(r)?;
    if inner != 8 {
        for _ in 0..count {
            skip_value(r, inner)?;
        }
        return Ok(Vec::new());
    }
    anyhow::ensure!(count < 10_000, "too many strings");
    (0..count).map(|_| read_string(r)).collect()
}

/// Reads an integer of any GGUF integer type, or skips a non-integer value.
fn read_int(r: &mut (impl Read + Seek), ty: u32) -> anyhow::Result<Option<u64>> {
    Ok(match ty {
        0 | 1 => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            Some(b[0] as u64)
        }
        2 | 3 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            Some(u16::from_le_bytes(b) as u64)
        }
        4 | 5 => Some(read_u32(r)? as u64),
        10 | 11 => Some(read_u64(r)?),
        other => {
            skip_value(r, other)?;
            None
        }
    })
}

fn scalar_size(ty: u32) -> Option<u64> {
    match ty {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn skip_value(r: &mut (impl Read + Seek), ty: u32) -> anyhow::Result<()> {
    if let Some(n) = scalar_size(ty) {
        r.seek(SeekFrom::Current(n as i64))?;
        return Ok(());
    }
    match ty {
        8 => {
            let len = read_u64(r)?;
            r.seek(SeekFrom::Current(len as i64))?;
        }
        9 => {
            let inner = read_u32(r)?;
            let count = read_u64(r)?;
            if let Some(n) = scalar_size(inner) {
                r.seek(SeekFrom::Current((n * count) as i64))?;
            } else {
                for _ in 0..count {
                    skip_value(r, inner)?;
                }
            }
        }
        other => anyhow::bail!("unknown GGUF value type {other}"),
    }
    Ok(())
}

/// Quantization name, preferring the one in the file name (which carries unsloth's `UD-`
/// prefix and `_XL` variants) over the header's file type.
pub fn quant_label(file_name: &str, file_type: Option<u32>) -> Option<String> {
    let upper = file_name.to_ascii_uppercase();
    let stem = upper.trim_end_matches(".GGUF");
    let re = regex::Regex::new(
        r"(UD-)?(IQ[1-4]_(XXS|XS|S|M|NL)|Q[2-8]_K(_[SMLX]{1,2})?|Q[4-8]_[01]|Q8_K_XL|MXFP4(_MOE)?|NVFP4|BF16|F16|F32)",
    )
    .expect("valid regex");
    if let Some(m) = re.find_iter(stem).last() {
        return Some(m.as_str().to_string());
    }
    let named = match file_type? {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        23 => "IQ3_XXS",
        25 => "IQ4_NL",
        30 => "IQ4_XS",
        32 => "BF16",
        38 => "MXFP4",
        _ => return None,
    };
    Some(named.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_kv_string(out: &mut Vec<u8>, key: &str, value: &str) {
        out.extend((key.len() as u64).to_le_bytes());
        out.extend(key.as_bytes());
        out.extend(8u32.to_le_bytes());
        out.extend((value.len() as u64).to_le_bytes());
        out.extend(value.as_bytes());
    }

    #[test]
    fn reads_a_synthetic_header() {
        let mut b = Vec::new();
        b.extend(b"GGUF");
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(8u64.to_le_bytes());
        write_kv_string(&mut b, "general.architecture", "qwen35");
        write_kv_string(&mut b, "general.name", "Qwen3.5 9B");
        // An array of u32 to skip.
        let key = "qwen35.some_array";
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(9u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(3u64.to_le_bytes());
        b.extend([0u8; 12]);
        let key = "qwen35.context_length";
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(262144u32.to_le_bytes());
        let key = "general.file_type";
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(18u32.to_le_bytes());
        let key = "qwen35.pooling_type";
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        let key = "general.tags";
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(28u64.to_le_bytes());
        b.extend(b"automatic-speech-recognition");
        write_kv_string(&mut b, "tokenizer.ggml.model", "gpt2");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.gguf");
        std::fs::File::create(&path).unwrap().write_all(&b).unwrap();
        let info = read(&path).unwrap();
        assert_eq!(info.architecture.as_deref(), Some("qwen35"));
        assert_eq!(info.context_length, Some(262144));
        assert_eq!(info.file_type, Some(18));
        assert_eq!(info.pooling_type, Some(3));
        assert!(info.is_embedding());
        assert_eq!(info.tags, ["automatic-speech-recognition"]);
        assert!(info.is_speech_recognition());
        assert!(!GgufInfo::default().is_speech_recognition());
    }

    #[test]
    fn quant_from_file_name_wins() {
        assert_eq!(
            quant_label("Qwen3.5-9B-UD-Q6_K_XL.gguf", Some(18)).as_deref(),
            Some("UD-Q6_K_XL")
        );
        assert_eq!(
            quant_label("gemma-4-12B-it-qat-UD-Q4_K_XL.gguf", None).as_deref(),
            Some("UD-Q4_K_XL")
        );
        assert_eq!(quant_label("model.gguf", Some(15)).as_deref(), Some("Q4_K_M"));
        assert_eq!(quant_label("x-IQ4_XS.gguf", None).as_deref(), Some("IQ4_XS"));
    }
}
