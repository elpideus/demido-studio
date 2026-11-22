//! Finding models on Hugging Face, and downloading them into Demido's models folder.
//!
//! Brief 4.7. Hugging Face is where model makers publish, and where the GGUF files llama.cpp runs are. Demido
//! asks its public interface, under Demido's own name, and nothing else: no account, no key (research note 17).
//!
//! - **Searching** lists GGUF repositories only: those are what the runtime runs. Each comes with what its
//!   own files say: how large it is, what it was trained to do (tools, thinking, images), its licence.
//! - **A model's page** lists its files, grouped into quantizations (one file each, or several shards), and
//!   says for each how much memory it would need here and whether it fits this computer. One is recommended:
//!   the best quality that fits on the graphics card with the memory kept free.
//! - **Downloading** is done as the runtime's own download is done (`runtime::fetch`): from the publisher's
//!   address, checked against the SHA-256 the publisher lists, going on from where it stopped. A model with
//!   a vision part, or in several shards, becomes a folder; the runtime reads it as one model.
//!
//! What is kept: searches for ten minutes, a model's page for an hour (the caching rule, DECISIONS P). When
//! Hugging Face says it has been asked too much, it is left alone for as long as it says.
//!
//! Not built: models that need signing in to Hugging Face (gated ones: that needs the place where keys are
//! kept), and other sources than Hugging Face.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::runtime::RemoteFile;
use crate::{Error, Result, now_ms};

const MIB: u64 = 1024 * 1024;
const SEARCH_IS_FRESH: Duration = Duration::from_secs(10 * 60);
const PAGE_IS_FRESH: Duration = Duration::from_secs(60 * 60);
/// How many models a page of search results has.
const A_PAGE: usize = 20;
/// What a model card is cut to.
const MOST_OF_A_CARD: usize = 30_000;
/// A download needs this much room on the disk beyond its own size.
const ROOM_TO_SPARE: u64 = 512 * MIB;
/// The folder downloads are made in, inside the models folder. A dot folder, so that neither Demido nor the
/// runtime takes what is in it for a model until it is complete.
const INCOMING: &str = ".incoming";
/// The files of one quantization, under the name they share (a file's, or its shards' without their numbers).
type Group = (String, Vec<HubFile>);
/// An answer kept: when it came, what it said, and its `Link` header.
type Kept = (Instant, Value, Option<String>);

/// Where it is written down which model came from where.
const SOURCES: &str = ".sources.json";

/// What of this computer decides whether a model fits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, ToSchema)]
pub struct Machine {
    /// The graphics card's own memory, in MiB. 0 without one. For graphics that share the computer's
    /// memory (Apple), the part of it they may use.
    pub card_mb: u64,
    pub ram_mb: u64,
    /// "Memory kept free", in MiB.
    pub margin_mb: u64,
    /// The context Demido loads models with, in tokens.
    pub context: u64,
}

/// About how much memory a model of this size needs at this context, in MiB. An estimate: the runtime
/// measures the real figure when it loads the model, and Demido shows that from then on.
///
/// The file itself, the cache of what it holds in mind (about 300 MB a billion parameters at 8,192 tokens,
/// as measured for Qwen3 4B, and no more than 4 GB however large the model: large models share that cache
/// across more heads), and 300 MB for the runtime's own work.
pub fn needs_mb(file_bytes: u64, params: Option<u64>, context: u64) -> u64 {
    let file_mb = file_bytes.div_ceil(MIB);
    let billions = params.map_or(file_mb as f64 / 1024.0 * 1.6, |p| p as f64 / 1e9);
    let cache = (billions * 300.0).min(4096.0) * context as f64 / 8192.0;
    file_mb + cache.round() as u64 + 300
}

/// Whether a model that needs this much fits: `fits` (on the graphics card, with the memory kept free),
/// `slow` (only with part of it on the processor, which is several times slower), or `no`.
pub fn fits(needs: u64, machine: &Machine) -> &'static str {
    if machine.card_mb > 0 && needs + machine.margin_mb <= machine.card_mb {
        "fits"
    } else if needs + machine.margin_mb <= machine.card_mb + machine.ram_mb / 2 {
        "slow"
    } else {
        "no"
    }
}

/// The quantizations, from the best quality to the least. A name that is not here ranks last.
const QUANTS: [&str; 45] = [
    "F32", "BF16", "F16", "Q8_K_XL", "Q8_0", "Q6_K_XL", "Q6_K_L", "Q6_K", "Q5_K_XL", "Q5_K_L", "Q5_K_M", "Q5_K_S", "Q5_1", "Q5_0", "Q4_K_XL", "Q4_K_L", "Q4_K_M", "Q4_K_S", "Q4_1", "IQ4_NL", "IQ4_XS",
    "Q4_0", "MXFP4", "Q3_K_XL", "Q3_K_L", "Q3_K_M", "IQ3_M", "IQ3_S", "Q3_K_S", "IQ3_XS", "IQ3_XXS", "Q2_K_XL", "Q2_K_L", "Q2_K", "IQ2_M", "IQ2_S", "IQ2_XS", "IQ2_XXS", "IQ1_M", "IQ1_S", "TQ2_0",
    "TQ1_0", "Q4_0_4_4", "Q4_0_4_8", "Q4_0_8_8",
];
/// What models are made for that the runtime does not do: making pictures, films or speech. Their GGUF files
/// are for other programs, so a search leaves them out.
const NOT_FOR_THIS_RUNTIME: [&str; 7] = ["text-to-image", "image-to-image", "image-to-video", "text-to-video", "unconditional-image-generation", "text-to-speech", "text-to-audio"];
/// Kept for converting and for measuring, not for running: never recommended when anything smaller fits.
const UNQUANTIZED: [&str; 3] = ["F32", "BF16", "F16"];

/// The quantization a file name says, as written in `QUANTS`. `None` when it names none of them.
pub fn quant_of(name: &str) -> Option<&'static str> {
    let upper = name.to_uppercase();
    let parts: Vec<&str> = upper.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').collect();
    let mut by_length: Vec<&'static str> = QUANTS.to_vec();
    by_length.sort_by_key(|q| std::cmp::Reverse(q.len()));
    // "Qwen3-4B-Q4_K_M", "model.Q4_K_M", "UD-Q4_K_XL": the quantization stands as a part of its own.
    by_length.into_iter().find(|q| parts.iter().any(|part| part == q || part.ends_with(&format!("_{q}"))))
}

/// About how many bits a weight takes in a quantization, from its name.
fn bits_of(quant: &str) -> Option<f64> {
    let table: [(&str, f64); 16] = [
        ("F32", 32.0),
        ("BF16", 16.0),
        ("F16", 16.0),
        ("Q8", 8.0),
        ("Q6", 6.5),
        ("Q5", 5.5),
        ("MXFP4", 4.2),
        ("IQ4", 4.2),
        ("Q4", 4.5),
        ("IQ3", 3.0),
        ("Q3", 3.4),
        ("IQ2", 2.0),
        ("Q2", 2.6),
        ("IQ1", 1.5),
        ("TQ1", 1.6),
        ("TQ2", 2.0),
    ];
    table.iter().find(|(prefix, _)| quant.starts_with(prefix)).map(|(_, bits)| *bits)
}

fn rank_of(quant: &str) -> usize {
    QUANTS.iter().position(|q| *q == quant).unwrap_or(QUANTS.len())
}

/// A file's name without its shard number: `model-00001-of-00003.gguf` is a part of `model.gguf`.
fn without_shard(path: &str) -> String {
    let stem = path.strip_suffix(".gguf").unwrap_or(path);
    let parts: Vec<&str> = stem.rsplitn(4, '-').collect();
    // "…-00001-of-00003": the last three parts are a number, "of", a number.
    if parts.len() == 4 && parts[1] == "of" && parts[0].chars().all(|c| c.is_ascii_digit()) && parts[2].chars().all(|c| c.is_ascii_digit()) {
        return format!("{}.gguf", parts[3]);
    }
    path.to_string()
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

// ----- what is shown --------------------------------------------------------------------------------------

/// A model as a search lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HubModel {
    /// `owner/name`, as on Hugging Face.
    pub id: String,
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    pub trending: f64,
    /// When it last changed, as Hugging Face writes it.
    pub updated: Option<String>,
    /// How many parameters, as its files say.
    pub params: Option<u64>,
    pub architecture: Option<String>,
    /// The most it can hold in mind, in tokens, as its files say.
    pub context_length: Option<u64>,
    pub licence: Option<String>,
    /// The model it was made from, as its card names it (`owner/name`): for a GGUF, the original.
    #[serde(default)]
    pub base_model: Option<String>,
    /// It needs signing in to Hugging Face and accepting its maker's terms. Not downloadable yet.
    pub gated: bool,
    /// What it takes in besides text, as its files say: `images`, `sound`.
    pub takes: Vec<String>,
    /// What it was made to do, as its files say: `tools`, `thinking`, `embedding`.
    pub does: Vec<String>,
    /// About how large its most usual quantization (Q4_K_M) would be, in bytes, from its parameters. The
    /// model's page has the real files.
    pub usual_size: Option<u64>,
    /// Whether that would fit this computer: `fits`, `slow`, `no`. `null` when its size is not known.
    pub fits: Option<String>,
    /// It is made to make pictures, films or speech, which the runtime does not do.
    #[serde(default)]
    pub makes_other_things: bool,
}

/// One file of a repository.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HubFile {
    /// Its path in the repository.
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
}

/// One quantization of a model: a file, or several shards of one.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct HubQuant {
    /// `Q4_K_M`, or the file's own name when it names no known quantization.
    pub name: String,
    pub files: Vec<HubFile>,
    /// All its files, with the vision part when there is one, in bytes.
    pub size: u64,
    /// About how much memory it would need here, in MiB (`needs_mb`).
    pub needs_mb: u64,
    /// `fits`, `slow` or `no` (`fits`).
    pub fits: String,
    pub recommended: bool,
    /// It is in the models folder already, under the name it would be given.
    pub on_disk: bool,
    /// What it would be called in the models folder.
    pub local_id: String,
}

/// A model's page.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct HubDetail {
    pub model: HubModel,
    /// The commit its files are from: downloads ask for exactly these.
    pub revision: String,
    pub quants: Vec<HubQuant>,
    /// The part that lets it see images, downloaded with whichever quantization is chosen.
    pub vision_part: Option<HubFile>,
    /// GGUF files in it that are not this model: smaller models that come with it (a draft model, for
    /// example) and calibration data. Not offered.
    pub others: Vec<HubFile>,
    /// Its model card, as its maker wrote it (Markdown), cut to 30,000 characters.
    pub card: Option<String>,
    /// Its page on Hugging Face.
    pub page: String,
    pub machine: Machine,
}

/// A page of search results.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct HubPage {
    pub models: Vec<HubModel>,
    /// What to ask for the next page. `null` on the last.
    pub next: Option<String>,
    pub machine: Machine,
}

/// A download, as it goes.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Download {
    pub id: String,
    pub repo: String,
    pub quant: String,
    /// What the model will be called in the models folder.
    pub local_id: String,
    pub files: Vec<String>,
    /// In bytes.
    pub total: u64,
    pub done: u64,
    /// `waiting` (another download is going), `downloading`, `done`, `failed` or `stopped`.
    pub state: String,
    pub problem: Option<String>,
    pub started_at: i64,
}

/// Where a downloaded model came from, kept beside the models.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Source {
    repo: String,
    revision: String,
    quant: String,
    files: Vec<HubFile>,
    licence: Option<String>,
    downloaded_at: i64,
    /// The model it was made from, as its repository's card names it (`owner/name`).
    #[serde(default)]
    base_model: Option<String>,
    /// The settings in that model's own `generation_config.json`, read when it was downloaded
    /// (`engine::sampling`). `null` when there is none, or it says nothing of them.
    #[serde(default)]
    maker_sampling: Option<crate::engine::sampling::Sampling>,
}

// ----- reading Hugging Face's answers ---------------------------------------------------------------------

/// What a model's own files say it does and takes, from its chat template and its tags.
fn abilities(item: &Value) -> (Vec<String>, Vec<String>) {
    let text = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let template = item.pointer("/gguf/chat_template").and_then(Value::as_str).unwrap_or_default();
    let pipeline = text("pipeline_tag");
    let tags: Vec<String> = item.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(Value::as_str).map(str::to_lowercase).collect()).unwrap_or_default();
    let tagged = |word: &str| tags.iter().any(|t| t == word);
    let mut takes = Vec::new();
    if pipeline == "image-text-to-text" || tagged("vision") || tagged("multimodal") || template.contains("image_url") || template.contains("'image'") {
        takes.push("images".to_string());
    }
    if pipeline.starts_with("audio") || pipeline == "any-to-any" || template.contains("audio") {
        takes.push("sound".to_string());
    }
    let mut does = Vec::new();
    if template.contains("tools") || template.contains("tool_call") {
        does.push("tools".to_string());
    }
    if template.contains("<think>") || template.contains("enable_thinking") || template.contains("reasoning_content") {
        does.push("thinking".to_string());
    }
    if matches!(pipeline.as_str(), "feature-extraction" | "sentence-similarity") || tagged("sentence-transformers") || tagged("embeddings") {
        does.push("embedding".to_string());
    }
    (takes, does)
}

/// A model of a search, or of a page.
fn model_from(item: &Value, machine: &Machine) -> Option<HubModel> {
    let id = item.get("id").and_then(Value::as_str)?.to_string();
    let number = |pointer: &str| item.pointer(pointer).and_then(Value::as_u64);
    let params = number("/gguf/total").filter(|p| *p > 0);
    let licence = item.pointer("/cardData/license").and_then(|l| l.as_str().map(str::to_string).or_else(|| l.as_array().and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string)));
    let base_model = item.pointer("/cardData/base_model").and_then(|b| b.as_str().map(str::to_string).or_else(|| b.as_array().and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string)));
    let (takes, does) = abilities(item);
    // Q4_K_M is about 4.85 bits a weight.
    let usual_size = params.map(|p| (p as f64 * 0.606) as u64);
    Some(HubModel {
        author: item.get("author").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| id.split('/').next().unwrap_or_default().to_string()),
        downloads: number("/downloads").unwrap_or(0),
        likes: number("/likes").unwrap_or(0),
        trending: item.get("trendingScore").and_then(Value::as_f64).unwrap_or(0.0),
        updated: item.get("lastModified").and_then(Value::as_str).map(str::to_string),
        params,
        architecture: item.pointer("/gguf/architecture").and_then(Value::as_str).map(str::to_string),
        context_length: number("/gguf/context_length"),
        licence,
        base_model,
        gated: !matches!(item.get("gated"), None | Some(Value::Bool(false)) | Some(Value::Null)),
        takes,
        does,
        fits: usual_size.map(|size| fits(needs_mb(size, params, machine.context), machine).to_string()),
        makes_other_things: item.get("pipeline_tag").and_then(Value::as_str).is_some_and(|p| NOT_FOR_THIS_RUNTIME.contains(&p)),
        usual_size,
        id,
    })
}

/// The quantizations of a repository, and its vision part, from its list of files.
fn quants_from(siblings: &[Value], params: Option<u64>, machine: &Machine, on_disk: &dyn Fn(&str) -> bool) -> (Vec<HubQuant>, Option<HubFile>, Vec<HubFile>) {
    let files: Vec<HubFile> = siblings
        .iter()
        .filter_map(|s| {
            let path = s.get("rfilename").and_then(Value::as_str)?.to_string();
            path.to_lowercase().ends_with(".gguf").then(|| HubFile {
                size: s.get("size").and_then(Value::as_u64).or_else(|| s.pointer("/lfs/size").and_then(Value::as_u64)).unwrap_or(0),
                sha256: s.pointer("/lfs/sha256").and_then(Value::as_str).map(str::to_string),
                path,
            })
        })
        .collect();
    // Calibration data for making quantizations is a GGUF file too, and no model.
    let (mut others, files): (Vec<HubFile>, Vec<HubFile>) = files.into_iter().partition(|f| f.path.to_lowercase().contains("imatrix"));
    let (vision, models): (Vec<HubFile>, Vec<HubFile>) = files.into_iter().partition(|f| f.path.to_lowercase().contains("mmproj"));
    // Of several vision parts, the one in half precision: the size llama.cpp's own builds use.
    let vision_part = ["F16", "BF16", "F32"].iter().find_map(|kind| vision.iter().find(|f| f.path.to_uppercase().contains(kind)).cloned()).or_else(|| vision.first().cloned());
    let mut groups: Vec<Group> = Vec::new();
    for file in models {
        let key = without_shard(&file.path);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(file),
            None => groups.push((key, vec![file])),
        }
    }
    // A file much smaller than this model would be in its quantization is another model that comes with it:
    // a draft model for speeding up answers, for example. Seen on Hugging Face: a 26B model's repository with
    // a 1 GB "Q8_0" in a folder of its own, which is not the 26B model at all.
    let (groups, smaller): (Vec<Group>, Vec<Group>) = groups.into_iter().partition(|(key, files)| {
        let size: u64 = files.iter().map(|f| f.size).sum();
        match (params, quant_of(key).and_then(bits_of)) {
            (Some(params), Some(bits)) => size as f64 >= params as f64 * bits / 8.0 * 0.6,
            _ => true,
        }
    });
    others.extend(smaller.into_iter().flat_map(|(_, files)| files));
    let mut quants: Vec<HubQuant> = groups
        .into_iter()
        .map(|(key, mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            let size = files.iter().map(|f| f.size).sum::<u64>() + vision_part.as_ref().map_or(0, |v| v.size);
            let needs = needs_mb(size, params, machine.context);
            // One file stays a file; a model of several becomes a folder named after it.
            let several = files.len() > 1 || vision_part.is_some();
            let stem = base_name(&key).trim_end_matches(".gguf").to_string();
            HubQuant {
                name: quant_of(&key).map(str::to_string).unwrap_or_else(|| stem.clone()),
                needs_mb: needs,
                fits: fits(needs, machine).to_string(),
                recommended: false,
                on_disk: on_disk(&if several { stem.clone() } else { format!("{stem}.gguf") }),
                local_id: stem,
                size,
                files,
            }
        })
        .collect();
    quants.sort_by_key(|q| (rank_of(&q.name), q.size));
    // The best quality that fits on the card. Unquantized files only when nothing else fits.
    let best = |allow_unquantized: bool| quants.iter().position(|q| q.fits == "fits" && (allow_unquantized || !UNQUANTIZED.contains(&q.name.as_str())));
    let chosen = best(false).or_else(|| best(true)).or_else(|| {
        // Nothing fits on the card: the smallest that runs at all.
        quants.iter().enumerate().filter(|(_, q)| q.fits == "slow").min_by_key(|(_, q)| q.size).map(|(i, _)| i)
    });
    if let Some(i) = chosen {
        quants[i].recommended = true;
    }
    (quants, vision_part, others)
}

/// A model card without its block of settings at the top, cut to size.
fn card_from(text: &str) -> String {
    let text = text.trim_start();
    let body = match text.strip_prefix("---") {
        Some(rest) => rest.split_once("\n---").map_or(text, |(_, after)| after),
        None => text,
    }
    .trim();
    if body.chars().count() <= MOST_OF_A_CARD { body.to_string() } else { format!("{}\n\n… (the rest of the card is on Hugging Face)", body.chars().take(MOST_OF_A_CARD).collect::<String>()) }
}

/// The value of `cursor` in the address of the next page, from a `Link` header.
fn next_cursor(link: &str) -> Option<String> {
    let next = link.split(',').find(|part| part.contains("rel=\"next\""))?;
    let address = next.split(['<', '>']).nth(1)?;
    url::Url::parse(address).ok()?.query_pairs().find(|(k, _)| k == "cursor").map(|(_, v)| v.into_owned())
}

/// Whether a repository's name is one: `owner/name`, of letters, digits and `-_.`.
fn is_repo(id: &str) -> bool {
    let mut parts = id.split('/');
    let ok = |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

// ----- the hub --------------------------------------------------------------------------------------------

pub struct Hub {
    http: reqwest::Client,
    /// `https://huggingface.co`. A test points it at a stand-in.
    base: String,
    models_dir: PathBuf,
    events: Arc<EventLog>,
    kept: Mutex<HashMap<String, Kept>>,
    /// Hugging Face asked to be left alone until then.
    paused_until: Mutex<Option<Instant>>,
    downloads: Mutex<Vec<Download>>,
    stops: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Downloads go one after another: a second one waits.
    one_at_a_time: Arc<tokio::sync::Semaphore>,
}

impl Hub {
    pub fn new(models_dir: PathBuf, events: Arc<EventLog>) -> Self {
        Self::at("https://huggingface.co".into(), models_dir, events)
    }

    /// The same, asking another address: for a test and its stand-in.
    pub fn at(base: String, models_dir: PathBuf, events: Arc<EventLog>) -> Self {
        let http = reqwest::Client::builder().user_agent(crate::web::name()).connect_timeout(Duration::from_secs(15)).build().expect("a client can be made");
        Self {
            http,
            base,
            models_dir,
            events,
            kept: Mutex::default(),
            paused_until: Mutex::default(),
            downloads: Mutex::default(),
            stops: Mutex::default(),
            one_at_a_time: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    /// Asks Hugging Face for JSON, or gives what was kept. The second value is the `Link` header.
    async fn ask(&self, address: &str, fresh_for: Duration) -> Result<(Value, Option<String>)> {
        if let Some((at, value, link)) = self.kept.lock().expect("not poisoned").get(address).cloned()
            && at.elapsed() < fresh_for
        {
            return Ok((value, link));
        }
        if let Some(until) = *self.paused_until.lock().expect("not poisoned")
            && until > Instant::now()
        {
            return Err(Error::Busy(format!("Hugging Face asked to be left alone for a while. It can be asked again in {} seconds.", (until - Instant::now()).as_secs() + 1)));
        }
        let got = self
            .http
            .get(address)
            .header("accept", "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::Net(format!("Hugging Face could not be reached. Check the connection. ({})", crate::runtime::root_cause(&e))))?;
        let header = |name: &str| got.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let link = header("link");
        match got.status().as_u16() {
            200 => {}
            429 => {
                // "api";r=0;t=120: the seconds until it may be asked again.
                let wait = header("ratelimit").and_then(|r| r.split(';').find_map(|p| p.trim().strip_prefix("t=").and_then(|t| t.parse::<u64>().ok()))).unwrap_or(300);
                *self.paused_until.lock().expect("not poisoned") = Some(Instant::now() + Duration::from_secs(wait));
                return Err(Error::Busy(format!("Hugging Face says it has been asked too much from this address. It can be asked again in {wait} seconds.")));
            }
            401 | 403 => return Err(Error::NotFound("Hugging Face does not show this model without signing in.".into())),
            404 => return Err(Error::NotFound("Hugging Face has no such model.".into())),
            other => return Err(Error::Net(format!("Hugging Face answered {other}."))),
        }
        let value: Value = got.json().await.map_err(|_| Error::Net("Hugging Face's answer could not be read.".into()))?;
        self.kept.lock().expect("not poisoned").insert(address.to_string(), (Instant::now(), value.clone(), link.clone()));
        Ok((value, link))
    }

    /// Asks for a text file of a repository, through the address files are downloaded from.
    async fn text_of(&self, repo: &str, revision: &str, path: &str) -> Option<String> {
        let address = format!("{}/{repo}/resolve/{revision}/{path}", self.base);
        if let Some((at, value, _)) = self.kept.lock().expect("not poisoned").get(&address).cloned()
            && at.elapsed() < PAGE_IS_FRESH
        {
            return value.as_str().map(str::to_string);
        }
        let got = self.http.get(&address).timeout(Duration::from_secs(30)).send().await.ok()?;
        if !got.status().is_success() {
            return None;
        }
        let text = got.text().await.ok()?;
        self.kept.lock().expect("not poisoned").insert(address, (Instant::now(), Value::String(text.clone()), None));
        Some(text)
    }

    /// One page of GGUF models. `sort`: `trending`, `downloads`, `likes` or `updated`. `takes` and `does`
    /// keep only models that say so; `fits_only` only those whose usual size would fit this computer.
    #[allow(clippy::too_many_arguments)]
    pub async fn search(&self, words: &str, sort: &str, takes: &[String], does: &[String], fits_only: bool, cursor: Option<&str>, machine: Machine) -> Result<HubPage> {
        let mut address = url::Url::parse(&format!("{}/api/models", self.base)).map_err(|_| Error::Other("Hugging Face's address is wrong".into()))?;
        {
            let mut q = address.query_pairs_mut();
            q.append_pair("filter", "gguf").append_pair("limit", &A_PAGE.to_string()).append_pair("direction", "-1");
            q.append_pair(
                "sort",
                match sort {
                    "downloads" => "downloads",
                    "likes" => "likes",
                    "updated" => "lastModified",
                    _ => "trendingScore",
                },
            );
            let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
            if !words.is_empty() {
                q.append_pair("search", &words.chars().take(100).collect::<String>());
            }
            // Hugging Face can keep only one kind of model itself; the rest is kept here.
            if takes.iter().any(|t| t == "images") {
                q.append_pair("pipeline_tag", "image-text-to-text");
            } else if does.iter().any(|d| d == "embedding") {
                q.append_pair("pipeline_tag", "feature-extraction");
            }
            if let Some(cursor) = cursor {
                q.append_pair("cursor", cursor);
            }
            for field in ["author", "downloads", "likes", "trendingScore", "lastModified", "gguf", "gated", "cardData", "pipeline_tag", "tags"] {
                q.append_pair("expand[]", field);
            }
        }
        let (answer, link) = self.ask(address.as_str(), SEARCH_IS_FRESH).await?;
        let models = answer
            .as_array()
            .map(|items| items.iter().filter_map(|item| model_from(item, &machine)).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|m| !m.makes_other_things)
            .filter(|m| takes.iter().all(|t| m.takes.contains(t)) && does.iter().all(|d| m.does.contains(d)))
            .filter(|m| !fits_only || m.fits.as_deref() == Some("fits"))
            .collect();
        Ok(HubPage { models, next: link.as_deref().and_then(next_cursor), machine })
    }

    /// A model's page: its files by quantization, what each needs here, the one recommended, its card.
    pub async fn detail(&self, repo: &str, machine: Machine) -> Result<HubDetail> {
        if !is_repo(repo) {
            return Err(Error::Invalid(format!("\"{repo}\" is not a model's name on Hugging Face: it is written owner/name.")));
        }
        let address = format!(
            "{}/api/models/{repo}?blobs=true&expand[]=siblings&expand[]=gguf&expand[]=cardData&expand[]=gated&expand[]=sha&expand[]=pipeline_tag&expand[]=tags&expand[]=downloads&expand[]=likes&expand[]=lastModified&expand[]=author&expand[]=trendingScore",
            self.base
        );
        let (answer, _) = self.ask(&address, PAGE_IS_FRESH).await?;
        let model = model_from(&answer, &machine).ok_or_else(|| Error::Net("Hugging Face's answer could not be read.".into()))?;
        let revision = answer.get("sha").and_then(Value::as_str).unwrap_or("main").to_string();
        let siblings = answer.get("siblings").and_then(Value::as_array).cloned().unwrap_or_default();
        let on_disk = |name: &str| self.models_dir.join(name).exists();
        let (quants, vision_part, others) = quants_from(&siblings, model.params, &machine, &on_disk);
        let has_readme = siblings.iter().any(|s| s.get("rfilename").and_then(Value::as_str) == Some("README.md"));
        let card = if has_readme { self.text_of(repo, &revision, "README.md").await.map(|t| card_from(&t)) } else { None };
        let mut model = model;
        if vision_part.is_some() && !model.takes.contains(&"images".to_string()) {
            model.takes.insert(0, "images".into());
        }
        Ok(HubDetail { page: format!("https://huggingface.co/{repo}"), model, revision, quants, vision_part, others, card, machine })
    }

    /// The downloads of this run, the newest last.
    pub fn downloads(&self) -> Vec<Download> {
        self.downloads.lock().expect("not poisoned").clone()
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut Download)) -> Option<Download> {
        let mut all = self.downloads.lock().expect("not poisoned");
        let found = all.iter_mut().find(|d| d.id == id)?;
        change(found);
        Some(found.clone())
    }

    /// Free bytes on the disk the models folder is on. `None` when it cannot be told.
    fn room_on_disk(&self) -> Option<u64> {
        crate::system::free_bytes(&self.models_dir)
    }

    /// Starts downloading a quantization of a model. Gives the download back at once; it goes on by itself.
    pub async fn start(self: &Arc<Self>, repo: &str, quant: &str, machine: Machine) -> Result<Download> {
        let detail = self.detail(repo, machine).await?;
        if detail.model.gated {
            return Err(Error::Invalid(format!(
                "{repo} can only be downloaded after signing in to Hugging Face and accepting its maker's terms on its page. Demido cannot hold a Hugging Face key yet, so it cannot download this one."
            )));
        }
        let chosen = detail
            .quants
            .iter()
            .find(|q| q.name == quant)
            .ok_or_else(|| Error::NotFound(format!("{repo} has no {quant}. It has: {}.", detail.quants.iter().map(|q| q.name.as_str()).collect::<Vec<_>>().join(", "))))?;
        if chosen.on_disk {
            return Err(Error::Invalid(format!("{} is in the models folder already.", chosen.local_id)));
        }
        // The same download asked for again, while it goes or waits: the one there is.
        if let Some(going) = self.downloads().into_iter().find(|d| d.repo == repo && d.quant == quant && matches!(d.state.as_str(), "waiting" | "downloading")) {
            return Ok(going);
        }
        let mut files = chosen.files.clone();
        files.extend(detail.vision_part.clone());
        let total: u64 = files.iter().map(|f| f.size).sum();
        if let Some(room) = self.room_on_disk()
            && room < total + ROOM_TO_SPARE
        {
            return Err(Error::Invalid(format!(
                "There is not room for it: it needs {:.1} GB, and the disk the models folder is on has {:.1} GB free.",
                (total + ROOM_TO_SPARE) as f64 / 1e9,
                room as f64 / 1e9
            )));
        }
        let several = files.len() > 1;
        let download = Download {
            id: uuid::Uuid::now_v7().to_string(),
            repo: repo.to_string(),
            quant: quant.to_string(),
            local_id: chosen.local_id.clone(),
            files: files.iter().map(|f| base_name(&f.path).to_string()).collect(),
            total,
            done: 0,
            state: "waiting".into(),
            problem: None,
            started_at: now_ms(),
        };
        self.downloads.lock().expect("not poisoned").push(download.clone());
        let stop = Arc::new(AtomicBool::new(false));
        self.stops.lock().expect("not poisoned").insert(download.id.clone(), stop.clone());
        self.events.append("model.download.started", Some(&download.local_id), json!({ "repo": repo, "quant": quant, "files": download.files, "total": total }))?;
        let (hub, id, source) = (
            self.clone(),
            download.id.clone(),
            Source {
                repo: repo.into(),
                revision: detail.revision.clone(),
                quant: quant.into(),
                files: files.clone(),
                licence: detail.model.licence.clone(),
                downloaded_at: 0,
                base_model: detail.model.base_model.clone(),
                maker_sampling: None,
            },
        );
        tokio::spawn(async move { hub.carry_out(&id, files, several, source, &stop).await });
        Ok(download)
    }

    /// Does a download: each file in turn, then the whole into its place.
    async fn carry_out(&self, id: &str, files: Vec<HubFile>, several: bool, mut source: Source, stop: &AtomicBool) {
        let Ok(_turn) = self.one_at_a_time.clone().acquire_owned().await else { return };
        let Some(download) = self.update(id, |d| d.state = "downloading".into()) else { return };
        let incoming = self.models_dir.join(INCOMING).join(&download.local_id);
        let result: Result<()> = async {
            tokio::fs::create_dir_all(&incoming).await?;
            let mut before = 0u64;
            let mut told_at = Instant::now() - Duration::from_secs(1);
            for file in &files {
                let remote = RemoteFile {
                    name: base_name(&file.path).to_string(),
                    url: format!("{}/{}/resolve/{}/{}", self.base, source.repo, source.revision, file.path),
                    size: file.size,
                    sha256: file.sha256.clone(),
                };
                let to = incoming.join(&remote.name);
                if !to.exists() {
                    crate::runtime::download_file(&self.http, &remote, &to, stop, |bytes| {
                        let now = self.update(id, |d| d.done = before + bytes);
                        // Told twice a second at most: a download is millions of pieces.
                        if told_at.elapsed() >= Duration::from_millis(500)
                            && let Some(now) = now
                        {
                            told_at = Instant::now();
                            self.events.tell("model.download", Some(&now.local_id), json!(now));
                        }
                    })
                    .await?;
                }
                before += file.size;
                self.update(id, |d| d.done = before);
            }
            // Complete and checked: into its place, where Demido and the runtime see it.
            if several {
                tokio::fs::rename(&incoming, self.models_dir.join(&download.local_id)).await?;
            } else {
                tokio::fs::rename(incoming.join(&download.files[0]), self.models_dir.join(&download.files[0])).await?;
                tokio::fs::remove_dir(&incoming).await.ok();
            }
            source.downloaded_at = now_ms();
            source.maker_sampling = self.maker_sampling(source.base_model.as_deref()).await;
            self.note_source(&download.local_id, &source);
            Ok(())
        }
        .await;
        let (state, problem) = match result {
            Ok(()) => ("done", None),
            Err(Error::Stopped(why)) => ("stopped", Some(why)),
            Err(e) => ("failed", Some(e.to_string())),
        };
        if let Some(done) = self.update(id, |d| (d.state, d.problem) = (state.into(), problem.clone())) {
            self.events.tell("model.download", Some(&done.local_id), json!(done));
            let kind = match state {
                "done" => "model.downloaded",
                "stopped" => "model.download.stopped",
                _ => "model.download.failed",
            };
            let _ = self.events.append(kind, Some(&done.local_id), json!({ "repo": done.repo, "quant": done.quant, "problem": problem }));
        }
        self.stops.lock().expect("not poisoned").remove(id);
    }

    /// The settings a model's maker put in its `generation_config.json`, for the model a download was made
    /// from. One small file, once; nothing when it is not there or says nothing of them.
    async fn maker_sampling(&self, base_model: Option<&str>) -> Option<crate::engine::sampling::Sampling> {
        let base = base_model.filter(|b| b.split('/').count() == 2 && !b.contains(".."))?;
        let address = format!("{}/{base}/resolve/main/generation_config.json", self.base);
        let got = self.http.get(&address).timeout(Duration::from_secs(20)).send().await.ok()?;
        if !got.status().is_success() {
            return None;
        }
        let config: Value = got.json().await.ok()?;
        Some(crate::engine::sampling::from_generation_config(&config)).filter(|s| !s.is_empty())
    }

    /// Writes down where a model came from, beside the models.
    fn note_source(&self, local_id: &str, source: &Source) {
        let path = self.models_dir.join(SOURCES);
        let mut all: HashMap<String, Source> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        all.insert(local_id.to_string(), source.clone());
        if let Ok(text) = serde_json::to_vec_pretty(&all) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Stops a download that goes or waits; what was downloaded is kept, and asking again goes on from
    /// there. A download that has ended is forgotten. With `discard`, what was downloaded is removed too.
    pub fn stop(&self, id: &str, discard: bool) -> Result<Download> {
        let download = self.downloads().into_iter().find(|d| d.id == id).ok_or_else(|| Error::NotFound(format!("There is no download \"{id}\".")))?;
        if let Some(stop) = self.stops.lock().expect("not poisoned").get(id) {
            stop.store(true, Ordering::Relaxed);
        } else {
            self.downloads.lock().expect("not poisoned").retain(|d| d.id != id);
        }
        if discard && download.state != "done" {
            let _ = std::fs::remove_dir_all(self.models_dir.join(INCOMING).join(&download.local_id));
        }
        Ok(download)
    }

    /// Waits until a download has ended, for a test or the command line.
    pub async fn wait_for(&self, id: &str, at_most: Duration) -> Option<Download> {
        let started = Instant::now();
        while started.elapsed() < at_most {
            match self.downloads().into_iter().find(|d| d.id == id) {
                Some(d) if !matches!(d.state.as_str(), "waiting" | "downloading") => return Some(d),
                None => return None,
                _ => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
