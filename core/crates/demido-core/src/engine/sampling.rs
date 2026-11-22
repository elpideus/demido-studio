//! How a model on this computer picks its words: its sampling settings, chosen for each model
//! (DECISIONS K26 to K30, research note 19).
//!
//! What an answer is sent with, in this order:
//!
//! 1. **The person's own settings for the model**, for that way of answering (thinking or not), where they
//!    set one. Each setting they leave empty is the next one's.
//! 2. **What the model's maker recommends**, for that way of answering: from Demido's table (`MAKERS`), read
//!    from each model's card on a stated day, or else from the maker's `generation_config.json`, kept when the
//!    model was downloaded.
//! 3. Nothing at all: then the file's own settings apply (`general.sampling.*`), which llama.cpp takes for
//!    itself, and where the file has none, llama.cpp's defaults.
//!
//! The quiet questions Demido asks a model (which tool, which steps) keep their own settings, chosen and
//! measured for them (research note 13): this is for the answers the person reads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::{Error, Result, now_ms};

/// How a model picks each word. Each is `null` where nothing is said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Sampling {
    /// How freely it picks: 0 always the likeliest word, higher more varied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// It picks among the likeliest words that together make up this much of the chance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// It picks among this many of the likeliest words. 0: no such limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// A word is let in only if it is at least this likely, against the likeliest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    /// How much less it picks a word it has used at all already: against going round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// How much less it picks a word for each time it has used it lately. 1: not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f64>,
}

const fn set(temperature: f64, top_p: f64, top_k: u32, min_p: Option<f64>, presence_penalty: Option<f64>) -> Sampling {
    Sampling { temperature: Some(temperature), top_p: Some(top_p), top_k: Some(top_k), min_p, presence_penalty, repeat_penalty: None }
}

impl Sampling {
    pub fn is_empty(&self) -> bool {
        *self == Sampling::default()
    }

    /// Each setting of `self`, and where it has none, `under`'s.
    pub fn over(&self, under: &Sampling) -> Sampling {
        Sampling {
            temperature: self.temperature.or(under.temperature),
            top_p: self.top_p.or(under.top_p),
            top_k: self.top_k.or(under.top_k),
            min_p: self.min_p.or(under.min_p),
            presence_penalty: self.presence_penalty.or(under.presence_penalty),
            repeat_penalty: self.repeat_penalty.or(under.repeat_penalty),
        }
    }

    /// Writes what is said into a request for the runtime on this computer, by its own names.
    pub fn into_request(&self, body: &mut Value) {
        let mut put = |name: &str, v: Option<Value>| {
            if let Some(v) = v {
                body[name] = v;
            }
        };
        put("temperature", self.temperature.map(|v| json!(v)));
        put("top_p", self.top_p.map(|v| json!(v)));
        put("top_k", self.top_k.map(|v| json!(v)));
        put("min_p", self.min_p.map(|v| json!(v)));
        put("presence_penalty", self.presence_penalty.map(|v| json!(v)));
        put("repeat_penalty", self.repeat_penalty.map(|v| json!(v)));
    }

    /// Says no to a setting outside what makes sense.
    pub fn checked(&self) -> Result<()> {
        let within = |name: &str, v: Option<f64>, low: f64, high: f64| match v {
            Some(v) if !(low..=high).contains(&v) || v.is_nan() => Err(Error::Invalid(format!("{name} can be from {low} to {high}; {v} is not."))),
            _ => Ok(()),
        };
        within("Temperature", self.temperature, 0.0, 2.0)?;
        within("Top-p", self.top_p, 0.0, 1.0)?;
        within("Min-p", self.min_p, 0.0, 1.0)?;
        within("The presence penalty", self.presence_penalty, -2.0, 2.0)?;
        within("The repeat penalty", self.repeat_penalty, 0.5, 2.0)?;
        if self.top_k.is_some_and(|k| k > 1000) {
            return Err(Error::Invalid("Top-k can be from 0 to 1000.".into()));
        }
        Ok(())
    }
}

/// llama.cpp's own defaults, as its server's documentation gives them (research note 19).
pub const RUNTIME: Sampling = Sampling { temperature: Some(0.8), top_p: Some(0.95), top_k: Some(40), min_p: Some(0.05), presence_penalty: Some(0.0), repeat_penalty: Some(1.0) };

/// When the makers' cards were read for `MAKERS`.
pub const READ_ON: &str = "22 November 2026";

/// What a maker recommends for some of its models, as their cards say.
pub struct Maker {
    /// Who says so, for a person: "Qwen".
    pub who: &'static str,
    /// The models it is said of, by their repositories.
    pub models: &'static [&'static str],
    pub thinking: Sampling,
    pub plain: Sampling,
}

/// Research note 19's table. Only models whose own card was read are here.
pub const MAKERS: &[Maker] = &[
    Maker {
        who: "Qwen",
        models: &["Qwen/Qwen3-0.6B", "Qwen/Qwen3-1.7B", "Qwen/Qwen3-4B", "Qwen/Qwen3-8B", "Qwen/Qwen3-14B", "Qwen/Qwen3-32B", "Qwen/Qwen3-30B-A3B"],
        thinking: set(0.6, 0.95, 20, Some(0.0), None),
        plain: set(0.7, 0.8, 20, Some(0.0), None),
    },
    Maker { who: "Qwen", models: &["Qwen/Qwen3.5-0.8B", "Qwen/Qwen3.5-2B"], thinking: set(1.0, 0.95, 20, Some(0.0), Some(1.5)), plain: set(1.0, 1.0, 20, Some(0.0), Some(2.0)) },
    Maker { who: "Qwen", models: &["Qwen/Qwen3.5-4B", "Qwen/Qwen3.5-9B", "Qwen/Qwen3.5-27B"], thinking: set(1.0, 0.95, 20, Some(0.0), Some(1.5)), plain: set(0.7, 0.8, 20, Some(0.0), Some(1.5)) },
    Maker { who: "Qwen", models: &["Qwen/Qwen3.6-35B-A3B"], thinking: set(1.0, 0.95, 20, Some(0.0), Some(1.5)), plain: set(0.7, 0.8, 20, Some(0.0), Some(1.5)) },
    Maker { who: "Qwen", models: &["Qwen/Qwen3.8-27B"], thinking: set(1.0, 0.95, 20, Some(0.0), Some(0.0)), plain: set(0.7, 0.8, 20, Some(0.0), Some(1.5)) },
    Maker {
        who: "Google",
        models: &["google/gemma-4-E2B-it", "google/gemma-4-E4B-it", "google/gemma-4-12B-it", "google/gemma-4-26B-A4B-it", "google/gemma-4-31B-it"],
        thinking: set(1.0, 0.95, 64, None, None),
        plain: set(1.0, 0.95, 64, None, None),
    },
];

/// What Demido knows of which model a file is.
#[derive(Clone, Debug, Default)]
pub struct Identity {
    /// The model it was made from, as the repository it was downloaded from names it (`owner/name`).
    pub repo: Option<String>,
    /// What the file says of itself.
    pub header: Option<super::gguf::Header>,
}

/// The maker's word for a model, from Demido's table, with the model it was found as: by the repository it
/// came from, or else by the names the file gives itself. A file made for something the table's model was
/// not (a base model, a later edition, code, pictures) is not taken for it.
pub fn maker_for(id: &Identity) -> Option<(&'static Maker, &'static str)> {
    let found = |is: &dyn Fn(&str) -> bool| MAKERS.iter().find_map(|m| m.models.iter().find(|r| is(r)).map(|r| (m, *r)));
    if let Some(repo) = &id.repo
        && let Some(found) = found(&|r: &str| r.eq_ignore_ascii_case(repo))
    {
        return Some(found);
    }
    let h = id.header.as_ref()?;
    let (base, size) = (h.basename.as_deref()?, h.size_label.as_deref()?);
    let finetune = h.finetune.as_deref().unwrap_or_default().to_lowercase();
    if ["base", "2507", "coder", "vl", "abliterated", "uncensored"].iter().any(|w| finetune.contains(w)) {
        return None;
    }
    let names = [format!("{base}-{size}"), format!("{base}-{size}-{finetune}")];
    found(&|r: &str| r.rsplit('/').next().is_some_and(|n| names.iter().any(|c| c.eq_ignore_ascii_case(n))))
}

/// The settings the person set for one model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Yours {
    #[serde(default)]
    pub thinking: Sampling,
    #[serde(default)]
    pub plain: Sampling,
    #[serde(default)]
    pub changed_at: i64,
}

/// What an answer is sent with for one way of answering, and why.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Chosen {
    /// What Demido sends. Empty: it sends nothing, and the file's or the runtime's settings apply.
    pub sent: Sampling,
    /// What the model works with in the end: what is sent, then the file's own, then the runtime's.
    pub in_effect: Sampling,
    /// `yours` (some of it, at least), `maker`, `file` or `runtime`.
    pub from: String,
}

/// What a maker says of a model, as Demido found it.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct MakerSays {
    pub who: String,
    /// Where it was read.
    pub source: String,
    /// When: the day Demido's table was read, or the day the model was downloaded.
    pub read_on: Option<String>,
    /// `card` (Demido's table, from the model's card) or `generation_config` (the maker's file).
    pub how: String,
    pub thinking: Sampling,
    pub plain: Sampling,
}

/// A model's settings, as the person sees them.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ModelSettings {
    pub model: String,
    pub thinking: Chosen,
    pub plain: Chosen,
    /// What its maker recommends, when Demido knows.
    pub maker: Option<MakerSays>,
    /// What the person set, for each way of answering.
    pub yours: Yours,
    /// What the file itself carries, which llama.cpp takes when it is sent nothing.
    pub in_file: Sampling,
    /// llama.cpp's own defaults.
    pub runtime: Sampling,
}

/// The person's settings for each model, kept in Demido's own folder (not beside the models: that folder
/// may be shared). One file, written whole.
pub struct Kept {
    path: PathBuf,
    writing: Mutex<()>,
}

impl Kept {
    pub fn open(path: PathBuf) -> Self {
        Self { path, writing: Mutex::default() }
    }

    fn all(&self) -> HashMap<String, Yours> {
        std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn of(&self, model: &str) -> Yours {
        self.all().remove(model).unwrap_or_default()
    }

    /// Keeps the person's settings for a model; both empty forgets them.
    pub fn set(&self, model: &str, thinking: Sampling, plain: Sampling) -> Result<Yours> {
        thinking.checked()?;
        plain.checked()?;
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = self.all();
        let yours = Yours { thinking, plain, changed_at: now_ms() };
        if thinking.is_empty() && plain.is_empty() {
            all.remove(model);
        } else {
            all.insert(model.to_string(), yours.clone());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let new = self.path.with_extension(format!("new-{}", uuid::Uuid::new_v4()));
        std::fs::write(&new, serde_json::to_vec_pretty(&all)?)?;
        std::fs::rename(&new, &self.path)?;
        Ok(yours)
    }
}

/// Decides what each way of answering is sent with: the person's, over the maker's; or nothing.
pub fn decide(model: &str, yours: Yours, maker: Option<MakerSays>, in_file: Sampling) -> ModelSettings {
    let file_and_runtime = in_file.over(&RUNTIME);
    let choose = |mine: &Sampling, makers: Option<&Sampling>| {
        let sent = mine.over(makers.unwrap_or(&Sampling::default()));
        let from = if !mine.is_empty() {
            "yours"
        } else if makers.is_some_and(|m| !m.is_empty()) {
            "maker"
        } else if !in_file.is_empty() {
            "file"
        } else {
            "runtime"
        };
        Chosen { sent, in_effect: sent.over(&file_and_runtime), from: from.into() }
    };
    let thinking = choose(&yours.thinking, maker.as_ref().map(|m| &m.thinking));
    let plain = choose(&yours.plain, maker.as_ref().map(|m| &m.plain));
    ModelSettings { model: model.into(), thinking, plain, maker, yours, in_file, runtime: RUNTIME }
}

/// The maker's word from Demido's table, as the person is shown it: with the card of the model it is.
pub fn says(maker: &Maker, model: &str) -> MakerSays {
    MakerSays { who: maker.who.into(), source: format!("https://huggingface.co/{model}"), read_on: Some(READ_ON.into()), how: "card".into(), thinking: maker.thinking, plain: maker.plain }
}

/// The settings in a maker's `generation_config.json`. They are the maker's one set, for every way of answering.
pub fn from_generation_config(config: &Value) -> Sampling {
    let f = |name: &str| config.get(name).and_then(Value::as_f64);
    Sampling {
        temperature: f("temperature"),
        top_p: f("top_p"),
        top_k: f("top_k").filter(|k| *k >= 0.0).map(|k| k as u32),
        min_p: f("min_p"),
        presence_penalty: None,
        repeat_penalty: f("repetition_penalty"),
    }
}

/// The first GGUF file of a model on disk: the file, or the first part in its folder.
pub fn first_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let mut parts: Vec<PathBuf> = std::fs::read_dir(path)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "gguf") && !p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("mmproj")))
        .collect();
    parts.sort();
    parts.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::gguf::Header;

    fn named(base: &str, size: &str, finetune: &str) -> Identity {
        Identity { repo: None, header: Some(Header { basename: Some(base.into()), size_label: Some(size.into()), finetune: Some(finetune.into()), ..Header::default() }) }
    }

    #[test]
    fn a_maker_is_found_by_the_repository_or_by_the_files_own_names() {
        // The four test models: three Qwen3, one Granite, whose maker says nothing.
        for size in ["0.6B", "1.7B", "4B"] {
            let (m, model) = maker_for(&named("Qwen3", size, "Instruct")).unwrap();
            assert_eq!((m.thinking.temperature, m.plain.top_p, model), (Some(0.6), Some(0.8), format!("Qwen/Qwen3-{size}").as_str()), "{size}");
        }
        assert!(maker_for(&named("Qwen3", "4B", "Instruct-awq")).is_some());
        assert!(maker_for(&named("Granite-3.3", "2B", "instruct")).is_none());
        // By the repository: exact, and sizes of one family that their maker treats apart stay apart.
        let by = |repo: &str| maker_for(&Identity { repo: Some(repo.into()), header: None });
        assert_eq!(by("Qwen/Qwen3.5-2B").unwrap().0.plain.presence_penalty, Some(2.0));
        assert_eq!(by("qwen/qwen3.5-9b").unwrap().0.plain.presence_penalty, Some(1.5));
        assert!(by("Qwen/Qwen3-4B-Instruct-2507").is_none(), "a later edition has its own card, not read");
        // A file made for something else is not taken for the model it was made from.
        assert!(maker_for(&named("Qwen3", "4B", "Base")).is_none());
        assert!(maker_for(&named("Qwen3", "4B", "Instruct-2507")).is_none());
    }

    #[test]
    fn the_persons_settings_go_over_the_makers_and_nothing_is_sent_without_either() {
        let (maker, model) = maker_for(&named("Qwen3", "1.7B", "Instruct")).unwrap();
        let qwen = says(maker, model);
        let mine = Yours { plain: Sampling { temperature: Some(0.3), ..Sampling::default() }, ..Yours::default() };
        let s = decide("Qwen3-1.7B-Q8_0", mine, Some(qwen.clone()), Sampling::default());
        assert_eq!((s.plain.from.as_str(), s.plain.sent.temperature, s.plain.sent.top_p, s.plain.sent.top_k), ("yours", Some(0.3), Some(0.8), Some(20)));
        assert_eq!((s.thinking.from.as_str(), s.thinking.sent.temperature), ("maker", Some(0.6)));
        // What is not said is the runtime's, in the end.
        assert_eq!(s.thinking.in_effect.repeat_penalty, Some(1.0));
        // Neither: nothing is sent, and the file's own or the runtime's apply.
        let file = Sampling { temperature: Some(0.5), ..Sampling::default() };
        let none = decide("granite", Yours::default(), None, file);
        assert!(none.plain.sent.is_empty() && none.plain.from == "file" && none.plain.in_effect.temperature == Some(0.5) && none.plain.in_effect.top_k == Some(40));
        assert_eq!(decide("granite", Yours::default(), None, Sampling::default()).thinking.from, "runtime");
    }

    #[test]
    fn settings_out_of_range_are_refused_and_what_is_sent_is_named_as_the_runtime_names_it() {
        assert!(Sampling { temperature: Some(3.0), ..Sampling::default() }.checked().is_err());
        assert!(Sampling { top_k: Some(5000), ..Sampling::default() }.checked().is_err());
        assert!(Sampling { top_p: Some(f64::NAN), ..Sampling::default() }.checked().is_err());
        let mut body = json!({});
        set(0.7, 0.8, 20, Some(0.0), Some(1.5)).into_request(&mut body);
        assert_eq!(body, json!({ "temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0.0, "presence_penalty": 1.5 }));
        assert_eq!(from_generation_config(&json!({ "temperature": 0.6, "top_k": 20, "top_p": 0.95, "do_sample": true })), set(0.6, 0.95, 20, None, None));
    }

    #[test]
    fn the_persons_settings_are_kept_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("demido-sampling-{}", uuid::Uuid::new_v4()));
        let kept = Kept::open(dir.join("engine").join("model-settings.json"));
        assert_eq!(kept.of("m"), Yours::default());
        let t = Sampling { temperature: Some(0.2), ..Sampling::default() };
        kept.set("m", t, Sampling::default()).unwrap();
        assert_eq!(Kept::open(dir.join("engine").join("model-settings.json")).of("m").thinking, t);
        assert!(kept.set("m", Sampling { min_p: Some(4.0), ..Sampling::default() }, Sampling::default()).is_err());
        kept.set("m", Sampling::default(), Sampling::default()).unwrap();
        assert_eq!(kept.of("m"), Yours::default());
        std::fs::remove_dir_all(dir).ok();
    }
}
