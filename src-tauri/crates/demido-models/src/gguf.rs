//! What a GGUF file says about itself, read from its header.
//!
//! Two jobs, and both are about the file rather than about a listing:
//!
//! - **Facts.** The architecture, the trained context and the chat template are
//!   written into the file by whoever converted it. For a model on disk those
//!   are facts, which is the half of #36's capability rule that renders as
//!   tags rather than as *unknown*.
//! - **Whether the file is whole.** The header lists every tensor with its
//!   offset, shape and type, so the header alone says how long the file has to
//!   be. A download cut off at 98 per cent is a file shorter than its own
//!   header claims, and that is caught here rather than by a backend that
//!   fails to load it with a message about a tensor.
//!
//! **Only the header is read.** Tensor data is never touched, so a scan of a
//! library of 15 GB files costs a few megabytes of reads, most of it the
//! tokenizer's vocabulary being skipped over.
//!
//! The format is ggml's GGUF, versions 2 and 3. Version 1 used 32-bit counts
//! and predates every file anybody is still running; it is refused by name
//! rather than misread.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The four bytes every GGUF starts with.
const MAGIC: [u8; 4] = *b"GGUF";

/// Where tensor data is aligned to when the file does not say.
const ALIGNMENT: u64 = 32;

/// The ceilings on what a header may claim, so a file of random bytes that
/// happens to start with the magic is refused as malformed rather than read as
/// a request for a trillion tensors. Each is far above anything published.
const MAX_TENSORS: u64 = 1 << 20;
const MAX_KEYS: u64 = 1 << 20;
const MAX_STRING: u64 = 64 << 20;
const MAX_ARRAY: u64 = 1 << 28;

/// The longest array of scalars kept. A per-layer array is one entry per
/// block, which is a few hundred at most; the tokenizer's are one per token,
/// hundreds of thousands, and are stepped over.
const MAX_KEPT: u64 = 4096;

/// One metadata value this reader keeps.
///
/// An array is kept when it is short and of scalars, which is what a per-layer
/// value is: which layers attend over a sliding window, and how many KV heads
/// each has (`crate::slot`). Arrays of strings and long arrays are the
/// tokenizer's, and are skipped.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Uint(u64),
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    List(Vec<Value>),
}

/// One tensor, as far as the length of the file is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tensor {
    /// From the start of the data section.
    pub offset: u64,
    /// How many bytes it occupies, when its type is one this reader knows the
    /// block size of. `None` is a type newer than this table, and it is not
    /// guessed at.
    pub bytes: Option<u64>,
}

/// A parsed header.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: u32,
    pub metadata: BTreeMap<String, Value>,
    pub tensors: Vec<Tensor>,
    /// Where tensor data begins, aligned.
    pub data_start: u64,
}

/// Why a file is not a model that can be offered.
///
/// Carried to the window as data, and it writes the sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Damage {
    /// It does not start with `GGUF`. An HTML login page saved under a `.gguf`
    /// name is the case this exists for.
    #[error("not a GGUF file")]
    NotGguf,
    #[error("GGUF version {version} is not one this build reads")]
    Unsupported { version: u32 },
    /// The header contradicts itself or claims something no real file does.
    #[error("the header is malformed: {reason}")]
    Malformed { reason: String },
    /// Shorter than its own header says it is. `needs` is absent when the file
    /// ends inside the header itself, before the length could be worked out.
    #[error("the file is cut short: {has} bytes on disk")]
    Truncated { needs: Option<u64>, has: u64 },
    /// One piece of a split model is not on disk.
    #[error("piece {index} of {total} is missing")]
    MissingPiece { index: u32, total: u32 },
    #[error("could not be read: {reason}")]
    Unreadable { reason: String },
}

impl Value {
    /// A count, from either integer type: a header writes `u32` and `i32`
    /// interchangeably for the same key.
    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Value::Uint(number) => Some(*number),
            Value::Int(number) => u64::try_from(*number).ok(),
            _ => None,
        }
    }
}

impl Header {
    /// Read the header of the file at `path`.
    pub fn read(path: &Path) -> Result<Header, Damage> {
        let file = File::open(path).map_err(unreadable)?;
        let has = file.metadata().map_err(unreadable)?.len();
        let mut reader = Reader {
            inner: BufReader::with_capacity(64 * 1024, file),
        };
        match parse(&mut reader) {
            Ok(header) => Ok(header),
            Err(Stop::Eof) => Err(Damage::Truncated { needs: None, has }),
            Err(Stop::Damage(damage)) => Err(damage),
        }
    }

    /// How long the file has to be for every tensor it lists to be in it.
    ///
    /// A tensor of a type this reader cannot size counts as one byte past its
    /// offset: the least it can be, which is a lower bound rather than a
    /// guess.
    pub fn extent(&self) -> u64 {
        self.tensors
            .iter()
            .map(|tensor| {
                self.data_start
                    .saturating_add(tensor.offset)
                    .saturating_add(tensor.bytes.unwrap_or(1))
            })
            .max()
            .unwrap_or(self.data_start)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.metadata.get(key) {
            Some(Value::Text(text)) => Some(text),
            _ => None,
        }
    }

    pub fn uint(&self, key: &str) -> Option<u64> {
        self.metadata.get(key)?.as_uint()
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.metadata.get(key) {
            Some(Value::Bool(flag)) => Some(*flag),
            _ => None,
        }
    }

    pub fn list(&self, key: &str) -> Option<&[Value]> {
        match self.metadata.get(key) {
            Some(Value::List(values)) => Some(values),
            _ => None,
        }
    }
}

/// Read the header and check the file is as long as it says. What "verified"
/// means for a model on disk.
pub fn verify(path: &Path) -> Result<Header, Damage> {
    let header = Header::read(path)?;
    let has = std::fs::metadata(path).map_err(unreadable)?.len();
    let needs = header.extent();
    if has < needs {
        return Err(Damage::Truncated {
            needs: Some(needs),
            has,
        });
    }
    Ok(header)
}

fn unreadable(error: std::io::Error) -> Damage {
    Damage::Unreadable {
        reason: error.to_string(),
    }
}

/// Why parsing stopped: the bytes ran out, or they said something wrong.
enum Stop {
    Eof,
    Damage(Damage),
}

impl From<std::io::Error> for Stop {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Stop::Eof
        } else {
            Stop::Damage(unreadable(error))
        }
    }
}

fn malformed(reason: &str) -> Stop {
    Stop::Damage(Damage::Malformed {
        reason: reason.to_owned(),
    })
}

struct Reader<R> {
    inner: BufReader<R>,
}

impl<R: Read + Seek> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], Stop> {
        let mut buffer = [0u8; N];
        self.inner.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    fn u32(&mut self) -> Result<u32, Stop> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn u64(&mut self) -> Result<u64, Stop> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }

    fn position(&mut self) -> Result<u64, Stop> {
        Ok(self.inner.stream_position()?)
    }

    /// Move forward without reading. Past the end of the file is not an error
    /// for a seek, so the next read is what notices.
    fn skip(&mut self, count: u64) -> Result<(), Stop> {
        let count = i64::try_from(count).map_err(|_| malformed("a length past any file"))?;
        self.inner.seek_relative(count)?;
        Ok(())
    }

    fn length(&mut self, ceiling: u64, what: &str) -> Result<u64, Stop> {
        let length = self.u64()?;
        if length > ceiling {
            return Err(malformed(what));
        }
        Ok(length)
    }

    fn string(&mut self) -> Result<String, Stop> {
        let length = self.length(MAX_STRING, "a string past any header")?;
        let mut buffer = vec![0u8; usize::try_from(length).unwrap_or(usize::MAX)];
        self.inner.read_exact(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }

    fn skip_string(&mut self) -> Result<(), Stop> {
        let length = self.length(MAX_STRING, "a string past any header")?;
        self.skip(length)
    }

    /// One value of type `kind`, kept when it is a scalar or a string.
    fn value(&mut self, kind: u32) -> Result<Option<Value>, Stop> {
        Ok(Some(match kind {
            0 => Value::Uint(u64::from(self.bytes::<1>()?[0])),
            1 => Value::Int(i64::from(i8::from_le_bytes(self.bytes()?))),
            2 => Value::Uint(u64::from(u16::from_le_bytes(self.bytes()?))),
            3 => Value::Int(i64::from(i16::from_le_bytes(self.bytes()?))),
            4 => Value::Uint(u64::from(self.u32()?)),
            5 => Value::Int(i64::from(i32::from_le_bytes(self.bytes()?))),
            6 => Value::Float(f64::from(f32::from_le_bytes(self.bytes()?))),
            7 => Value::Bool(self.bytes::<1>()?[0] != 0),
            8 => Value::Text(self.string()?),
            9 => return self.array(),
            10 => Value::Uint(self.u64()?),
            11 => Value::Int(i64::from_le_bytes(self.bytes()?)),
            12 => Value::Float(f64::from_le_bytes(self.bytes()?)),
            _ => return Err(malformed("a value of unknown type")),
        }))
    }

    /// An array, kept when it is a short one of scalars and skipped otherwise.
    fn array(&mut self) -> Result<Option<Value>, Stop> {
        let kind = self.u32()?;
        let count = self.length(MAX_ARRAY, "an array past any header")?;
        if matches!(kind, 8 | 9) || count > MAX_KEPT {
            self.skip_elements(kind, count)?;
            return Ok(None);
        }
        let mut values = Vec::new();
        for _ in 0..count {
            if let Some(value) = self.value(kind)? {
                values.push(value);
            }
        }
        Ok(Some(Value::List(values)))
    }

    fn skip_array(&mut self) -> Result<(), Stop> {
        let kind = self.u32()?;
        let count = self.length(MAX_ARRAY, "an array past any header")?;
        self.skip_elements(kind, count)
    }

    fn skip_elements(&mut self, kind: u32, count: u64) -> Result<(), Stop> {
        match kind {
            8 => {
                for _ in 0..count {
                    self.skip_string()?;
                }
                Ok(())
            }
            9 => {
                for _ in 0..count {
                    self.skip_array()?;
                }
                Ok(())
            }
            scalar => {
                let width = match scalar {
                    0 | 1 | 7 => 1,
                    2 | 3 => 2,
                    4..=6 => 4,
                    10..=12 => 8,
                    _ => return Err(malformed("an array of unknown type")),
                };
                self.skip(count.saturating_mul(width))
            }
        }
    }
}

fn parse<R: Read + Seek>(reader: &mut Reader<R>) -> Result<Header, Stop> {
    if reader.bytes::<4>()? != MAGIC {
        return Err(Stop::Damage(Damage::NotGguf));
    }
    let version = reader.u32()?;
    if !(2..=3).contains(&version) {
        return Err(Stop::Damage(Damage::Unsupported { version }));
    }
    let tensor_count = reader.length(MAX_TENSORS, "too many tensors")?;
    let key_count = reader.length(MAX_KEYS, "too many metadata keys")?;

    let mut metadata = BTreeMap::new();
    for _ in 0..key_count {
        let key = reader.string()?;
        let kind = reader.u32()?;
        if let Some(value) = reader.value(kind)? {
            metadata.insert(key, value);
        }
    }

    let mut tensors = Vec::new();
    for _ in 0..tensor_count {
        reader.skip_string()?;
        let dimensions = reader.u32()?;
        if dimensions > 8 {
            return Err(malformed("too many tensor dimensions"));
        }
        let mut elements: u64 = 1;
        for _ in 0..dimensions {
            elements = elements.saturating_mul(reader.u64()?);
        }
        let kind = reader.u32()?;
        let offset = reader.u64()?;
        tensors.push(Tensor {
            offset,
            bytes: size_of(kind, elements),
        });
    }

    let alignment = match metadata.get("general.alignment") {
        Some(Value::Uint(alignment)) if *alignment > 0 => *alignment,
        Some(_) => return Err(malformed("a non-positive alignment")),
        None => ALIGNMENT,
    };
    let end = reader.position()?;
    let data_start = end.div_ceil(alignment).saturating_mul(alignment);

    Ok(Header {
        version,
        metadata,
        tensors,
        data_start,
    })
}

/// How many bytes `elements` values of ggml type `kind` occupy.
///
/// Each type is stored in blocks: `(elements per block, bytes per block)`.
/// The numbers are ggml's own, from the block structs in `ggml-common.h`, and
/// a type missing here answers `None` rather than an estimate.
fn size_of(kind: u32, elements: u64) -> Option<u64> {
    let (per_block, block_bytes): (u64, u64) = match kind {
        0 => (1, 4),      // F32
        1 => (1, 2),      // F16
        2 => (32, 18),    // Q4_0
        3 => (32, 20),    // Q4_1
        6 => (32, 22),    // Q5_0
        7 => (32, 24),    // Q5_1
        8 => (32, 34),    // Q8_0
        9 => (32, 36),    // Q8_1
        10 => (256, 84),  // Q2_K
        11 => (256, 110), // Q3_K
        12 => (256, 144), // Q4_K
        13 => (256, 176), // Q5_K
        14 => (256, 210), // Q6_K
        15 => (256, 292), // Q8_K
        16 => (256, 66),  // IQ2_XXS
        17 => (256, 74),  // IQ2_XS
        18 => (256, 98),  // IQ3_XXS
        19 => (256, 50),  // IQ1_S
        20 => (32, 18),   // IQ4_NL
        21 => (256, 110), // IQ3_S
        22 => (256, 82),  // IQ2_S
        23 => (256, 136), // IQ4_XS
        24 => (1, 1),     // I8
        25 => (1, 2),     // I16
        26 => (1, 4),     // I32
        27 => (1, 8),     // I64
        28 => (1, 8),     // F64
        29 => (256, 56),  // IQ1_M
        30 => (1, 2),     // BF16
        34 => (256, 54),  // TQ1_0
        35 => (256, 66),  // TQ2_0
        39 => (32, 17),   // MXFP4
        _ => return None,
    };
    Some(elements.div_ceil(per_block).saturating_mul(block_bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_block_type_is_sized_by_its_blocks_rather_than_by_its_bits() {
        // Q4_K is 144 bytes per 256 weights, which is 4.5 bits: the digit in
        // the name is not the size.
        assert_eq!(size_of(12, 256), Some(144));
        assert_eq!(size_of(12, 512), Some(288));
        assert_eq!(size_of(0, 10), Some(40));
        assert_eq!(size_of(8, 64), Some(68));
    }

    #[test]
    fn a_type_this_table_does_not_know_is_not_guessed() {
        assert_eq!(size_of(4, 256), None, "Q4_2 was removed from ggml");
        assert_eq!(size_of(999, 256), None);
    }
}
