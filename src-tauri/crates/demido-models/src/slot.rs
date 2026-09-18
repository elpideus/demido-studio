//! What one generation slot reserves on the card, read from the header.
//!
//! Brief B63: "Also user should be able to manually set the amount of parallel agents they want to run at the same time"
//!
//! A sub-agent is a second `llama.cpp` slot on the conversation's own weights,
//! so what parallelism costs is one slot's KV cache, and the geometry that
//! cache is sized from is written into the file: the layers, which of them
//! attend over a sliding window, how many key and value heads each has and how
//! wide they are. This is the producer `demido_chat::Pool` was missing
//! ([#105](https://github.com/elpideus/demido-studio/issues/105)), and the
//! context half of the fit verdict for a model on disk.
//!
//! **Only an architecture this reading knows is priced.** A cache layout is a
//! property of the architecture, not of the keys a header happens to carry: a
//! hybrid model with a recurrent state has `head_count_kv` too, and pricing it
//! as though every layer attended would be a number nobody measured. So
//! [`Geometry::of`] answers `None` for an architecture outside [`KNOWN`], and
//! `None` is `demido_vram::Queued::Unmeasured` rather than a guess.
//!
//! **Known means checked against the pinned build's own accounting.** Every
//! architecture in [`KNOWN`] had its price compared, to the MiB, with the
//! `llama_kv_cache: size` lines the pinned `llama-server` logs at load
//! (`docs/rules/done.md`). Adding one is adding that check, not a line.
//!
//! What is priced is the KV cache and nothing else. The compute buffers beside
//! it are sized by the scheduler from the graph rather than stated by the
//! file, and on the pinned build they do not grow with a second slot: the
//! development model's measured 552 MiB for a second slot at 32k is its KV to
//! the MiB.

use std::path::Path;

use crate::gguf::{Header, Value};
use crate::parts::{shard_of, stem_of};

/// What loading the model at `path` costs, as far as its files say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    /// Every piece of the model on disk, added up. More than lands on the card
    /// when a model keeps some of its tensors in system memory, which errs
    /// towards refusing a slot rather than towards admitting one.
    pub weights: u64,
    /// What one slot reserves at the context asked for. `None` is not priced:
    /// an architecture this reading does not know, or a header it could not
    /// read.
    pub per_slot: Option<u64>,
}

/// Price the model at `path`, the first piece of a split one, at a context of
/// `context` tokens per slot.
pub fn price(path: &Path, context: u32) -> Price {
    Price {
        weights: on_disk(path),
        per_slot: Geometry::read(path).map(|geometry| geometry.per_slot(context)),
    }
}

/// The model's bytes: the file, or every piece of a split model beside it.
fn on_disk(path: &Path) -> u64 {
    let length = |path: &Path| std::fs::metadata(path).map_or(0, |meta| meta.len());
    let (label, Some(first)) = shard_of(stem_of(path)) else {
        return length(path);
    };
    let Some(Ok(siblings)) = path.parent().map(std::fs::read_dir) else {
        return length(path);
    };
    siblings
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|piece| {
            let (name, shard) = shard_of(stem_of(piece));
            name == label && shard.is_some_and(|shard| shard.total == first.total)
        })
        .map(|piece| length(&piece))
        .fold(0, u64::saturating_add)
}

/// The architectures whose cache layout this reading knows. See the module
/// note for what it takes to add one.
pub const KNOWN: &[&str] = &["gemma4"];

/// What one cell of a cache element costs: `llama.cpp` keeps K and V in F16
/// unless told otherwise, and Demido never tells it otherwise.
const F16: u64 = 2;

/// Cells are allocated in multiples of this. The pinned build asked for a
/// context of 10000 allocates 10240 cells.
const PAD: u64 = 256;

/// The micro-batch a sliding-window cache is widened by, so a batch can be
/// written while the window is still being read. `llama.cpp`'s default, which
/// Demido never overrides.
const UBATCH: u64 = 512;

/// The attention geometry of a model: one entry per layer that holds a cache
/// of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geometry {
    layers: Vec<Layer>,
}

/// One layer's cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layer {
    /// Bytes one token costs this layer: its K and V heads, at F16.
    per_token: u64,
    /// The sliding window it attends over, or `None` for the whole context.
    window: Option<u64>,
}

impl Geometry {
    /// Read the geometry of the model at `path`. `None` when the file cannot
    /// be read or its architecture is not one this reading knows.
    pub fn read(path: &Path) -> Option<Geometry> {
        Geometry::of(&Header::read(path).ok()?)
    }

    /// The geometry a header states, when its architecture is in [`KNOWN`] and
    /// every key the layout needs is there.
    pub fn of(header: &Header) -> Option<Geometry> {
        let architecture = header.text("general.architecture")?;
        if !KNOWN.contains(&architecture) {
            return None;
        }
        let key = |name: &str| format!("{architecture}.{name}");

        let blocks = header.uint(&key("block_count"))?;
        // Layers past this one read an earlier layer's cache rather than
        // keeping their own: gemma 4's E models share the last few.
        let shared = header.uint(&key("attention.shared_kv_layers")).unwrap_or(0);
        let owned = blocks.checked_sub(shared)?;

        let heads = header.uint(&key("attention.head_count"))?;
        let width = header.uint(&key("embedding_length"))?;
        let key_length = header
            .uint(&key("attention.key_length"))
            .or_else(|| width.checked_div(heads))?;
        let value_length = header
            .uint(&key("attention.value_length"))
            .unwrap_or(key_length);
        let window = header.uint(&key("attention.sliding_window"));
        let pattern = header.list(&key("attention.sliding_window_pattern"));
        let kv_heads =
            PerLayer::of(header, &key("attention.head_count_kv")).unwrap_or(PerLayer::All(heads));

        let layers = (0..owned)
            .map(|layer| {
                let at = usize::try_from(layer).ok()?;
                let sliding = match (window, pattern) {
                    (Some(_), Some(pattern)) => matches!(pattern.get(at)?, Value::Bool(true)),
                    _ => false,
                };
                let (k, v) = if sliding {
                    (
                        header
                            .uint(&key("attention.key_length_swa"))
                            .unwrap_or(key_length),
                        header
                            .uint(&key("attention.value_length_swa"))
                            .unwrap_or(value_length),
                    )
                } else {
                    (key_length, value_length)
                };
                Some(Layer {
                    per_token: kv_heads
                        .at(at)?
                        .saturating_mul(k.saturating_add(v))
                        .saturating_mul(F16),
                    window: if sliding { window } else { None },
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Geometry { layers })
    }

    /// What one slot's cache reserves at a context of `context` tokens.
    ///
    /// A whole-context layer holds the context, rounded up to the cells the
    /// build allocates. A sliding-window layer holds its window and one
    /// micro-batch, and never more than the context.
    #[must_use]
    pub fn per_slot(&self, context: u32) -> u64 {
        let cells = pad(u64::from(context));
        self.layers
            .iter()
            .map(|layer| {
                let held = layer.window.map_or(cells, |window| {
                    cells.min(pad(window.saturating_add(UBATCH)))
                });
                held.saturating_mul(layer.per_token)
            })
            .fold(0, u64::saturating_add)
    }
}

/// A key a header states either once for every layer or once per layer.
enum PerLayer<'a> {
    All(u64),
    Each(&'a [Value]),
}

impl<'a> PerLayer<'a> {
    fn of(header: &'a Header, key: &str) -> Option<PerLayer<'a>> {
        header
            .uint(key)
            .map(PerLayer::All)
            .or_else(|| header.list(key).map(PerLayer::Each))
    }

    fn at(&self, layer: usize) -> Option<u64> {
        match self {
            PerLayer::All(value) => Some(*value),
            PerLayer::Each(values) => values.get(layer)?.as_uint(),
        }
    }
}

fn pad(cells: u64) -> u64 {
    cells.div_ceil(PAD).saturating_mul(PAD)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use std::collections::BTreeMap;

    use super::*;

    const MIB: u64 = 1024 * 1024;

    /// A header holding `keys`, and nothing a price reads besides.
    fn header(keys: &[(&str, Value)]) -> Header {
        Header {
            version: 3,
            metadata: keys
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
            tensors: Vec::new(),
            data_start: 0,
        }
    }

    fn pattern(layers: u64) -> Value {
        // Five sliding-window layers, then one over the whole context.
        Value::List(
            (0..layers)
                .map(|layer| Value::Bool(layer % 6 != 5))
                .collect(),
        )
    }

    /// The rig's development model, `gemma-4-E4B-it` Q8_0, as its header
    /// states it: 42 blocks of which the last 18 read an earlier cache.
    fn development() -> Header {
        header(&[
            ("general.architecture", Value::Text("gemma4".into())),
            ("gemma4.block_count", Value::Uint(42)),
            ("gemma4.embedding_length", Value::Uint(2560)),
            ("gemma4.attention.head_count", Value::Uint(8)),
            ("gemma4.attention.head_count_kv", Value::Uint(2)),
            ("gemma4.attention.key_length", Value::Uint(512)),
            ("gemma4.attention.value_length", Value::Uint(512)),
            ("gemma4.attention.key_length_swa", Value::Uint(256)),
            ("gemma4.attention.value_length_swa", Value::Uint(256)),
            ("gemma4.attention.sliding_window", Value::Uint(512)),
            ("gemma4.attention.shared_kv_layers", Value::Uint(18)),
            ("gemma4.attention.sliding_window_pattern", pattern(42)),
        ])
    }

    /// The rig's reference model, `gemma-4-26B-A4B-it-UD` IQ2_M: a KV head
    /// count per layer rather than one for all of them, and a wider window.
    fn reference() -> Header {
        let kv_heads = (0..30)
            .map(|layer| Value::Uint(if layer % 6 == 5 { 2 } else { 8 }))
            .collect();
        header(&[
            ("general.architecture", Value::Text("gemma4".into())),
            ("gemma4.block_count", Value::Uint(30)),
            ("gemma4.embedding_length", Value::Uint(2816)),
            ("gemma4.attention.head_count", Value::Uint(16)),
            ("gemma4.attention.head_count_kv", Value::List(kv_heads)),
            ("gemma4.attention.key_length", Value::Uint(512)),
            ("gemma4.attention.value_length", Value::Uint(512)),
            ("gemma4.attention.key_length_swa", Value::Uint(256)),
            ("gemma4.attention.value_length_swa", Value::Uint(256)),
            ("gemma4.attention.sliding_window", Value::Uint(1024)),
            ("gemma4.attention.shared_kv_layers", Value::Uint(0)),
            ("gemma4.attention.sliding_window_pattern", pattern(30)),
        ])
    }

    /// Every row is what the pinned `llama-server` logged as its KV cache
    /// (`llama_kv_cache: size`, whole-context and sliding-window caches added)
    /// for that model at that context on one slot. The price is the build's
    /// own accounting, to the MiB.
    #[test]
    fn a_slot_is_priced_at_what_the_pinned_build_allocates() {
        let development = Geometry::of(&development()).expect("gemma4 is known");
        let reference = Geometry::of(&reference()).expect("gemma4 is known");

        for (what, geometry, context, logged) in [
            ("development at 32k: 512 + 40", &development, 32768, 552),
            (
                "development at 10000, padded to 10240: 160 + 40",
                &development,
                10000,
                200,
            ),
            (
                "development at 700, the window wider than the context: 12 + 30",
                &development,
                700,
                42,
            ),
            (
                "reference at 32k, KV heads per layer: 640 + 300",
                &reference,
                32768,
                940,
            ),
        ] {
            assert_eq!(geometry.per_slot(context), logged * MIB, "{what}");
        }
    }

    /// `docs/rules/done.md` read one slot of the development model at 32k two
    /// ways before anything could price it: 563 MiB derived out of a total,
    /// and 620 weighed around two loads on #65. The header's 552 is under
    /// both, and it is the one this build's own card agrees with: weighed
    /// again on #105, around one slot and two of the same load, a second slot
    /// took 552 MiB of NVML's free reading, with no compute buffer growing
    /// beside it. And the three decide every parallelism the ladder allows
    /// identically: on the 5583 MiB the development model leaves at 32k, the
    /// seven slots beside the conversation that `tools.parallel_agents`'
    /// ceiling of eight can ask for fit by each reading.
    #[test]
    fn the_price_is_the_slot_done_md_read_two_ways() {
        let priced = Geometry::of(&development()).unwrap().per_slot(32768);
        let (derived, weighed) = (563 * MIB, 620 * MIB);
        assert_eq!(priced, 552 * MIB);
        assert!(priced < derived && priced < weighed);

        let left = 5583 * MIB;
        for reading in [priced, derived, weighed] {
            assert!(left / reading >= 7, "{} MiB a slot", reading / MIB);
        }
    }

    /// A hybrid with a recurrent state states `head_count_kv` too, and
    /// pricing it as though every layer attended would be a guess.
    #[test]
    fn an_architecture_this_reading_does_not_know_is_not_priced() {
        let qwen35 = header(&[
            ("general.architecture", Value::Text("qwen35".into())),
            ("qwen35.block_count", Value::Uint(32)),
            ("qwen35.embedding_length", Value::Uint(4096)),
            ("qwen35.attention.head_count", Value::Uint(16)),
            ("qwen35.attention.head_count_kv", Value::Uint(4)),
            ("qwen35.attention.key_length", Value::Uint(256)),
            ("qwen35.attention.value_length", Value::Uint(256)),
        ]);
        assert_eq!(Geometry::of(&qwen35), None);
        assert_eq!(Geometry::of(&header(&[])), None, "no architecture at all");
    }

    /// A known architecture missing a key its layout needs is not priced
    /// either: a layer with no pattern entry is not assumed to be one or the
    /// other.
    #[test]
    fn a_known_architecture_missing_its_geometry_is_not_priced() {
        let mut short = development();
        short.metadata.insert(
            "gemma4.attention.sliding_window_pattern".into(),
            pattern(10),
        );
        assert_eq!(
            Geometry::of(&short),
            None,
            "a pattern shorter than the layers"
        );

        let mut headless = development();
        headless.metadata.remove("gemma4.block_count");
        assert_eq!(Geometry::of(&headless), None);
    }

    /// An absurd context in a settings file prices an absurd slot, never a
    /// small one that wrapped.
    #[test]
    fn the_price_saturates_rather_than_wrapping() {
        let geometry = Geometry::of(&development()).unwrap();
        assert!(geometry.per_slot(u32::MAX) > geometry.per_slot(32768));
    }
}
