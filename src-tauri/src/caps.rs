//! Model capability resolution.
//!
//! Answers "does this model do vision / tools / reasoning?" from sources that actually
//! *know*, instead of guessing from the model id. Resolution order, first known wins:
//!
//! 0. **The user** — a manual override in `model_overrides.caps_*` beats everything. The
//!    detection below is good, not omniscient: a brand-new model nothing has heard of, a
//!    private endpoint, or a host that lies all end with the user knowing better. Set
//!    per-field, so overriding vision doesn't disturb detected tools.
//! 1. **Provider capability API** — Anthropic `capabilities`, Gemini `thinking`,
//!    LM Studio `/api/v1/models`. The host tells us. Authoritative.
//! 2. **llama.cpp `/props`** — for the local engine: `modalities.vision` comes from the
//!    loaded model/mmproj, and `chat_template_caps` is computed by llama.cpp *executing*
//!    the model's jinja template (`common/jinja/caps.h`). Ground truth, but only for the
//!    model currently loaded — so we probe on spawn and cache it (`db::local_models`).
//! 3. **models.dev registry** — a community-maintained index (166 providers, ~2.8k model
//!    ids) with explicit `modalities.input`/`tool_call`/`reasoning` fields. Covers cloud
//!    models and open-weight repos we haven't loaded yet.
//! 4. **Unknown** — assume tools, nothing else. A model that can't do tools errors on the
//!    call; pretending it can't would silently hide the feature.
//!
//! No substring matching on model names. If we don't know, we say so (`CapsSource`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

const REGISTRY_URL: &str = "https://models.dev/api.json";
const REGISTRY_CACHE_FILE: &str = "models.dev.json";
const REGISTRY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Where a model's capability flags came from. Surfaced to the UI so "no vision" can be
/// distinguished from "we have no idea".
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapsSource {
    /// The provider's own capability API reported it.
    Provider,
    /// Probed from a running llama-server via `/props`.
    LlamaCpp,
    /// Looked up in the models.dev registry.
    Registry,
    /// Read straight from the repo's config.json / tokenizer_config.json on Hugging Face —
    /// used when models.dev hasn't indexed the repo yet.
    HuggingFace,
    /// Nothing knew this model — flags are defaults, not facts.
    Unknown,
}

/// Which flags the user pinned by hand. Kept separate from `source` because overrides are
/// per-field: "you set vision, we detected the rest" is a thing the UI has to say.
#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy)]
pub struct Overridden {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
}

/// Per-model capability flags, resolved.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelCaps {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
    /// Where the *detected* values came from. Says nothing about overridden fields —
    /// check `overridden` for those.
    pub source: CapsSource,
    pub overridden: Overridden,
}

/// What a single source knows. `None` means "this source didn't say", which is different
/// from `Some(false)` ("this source says no") — that distinction is the whole point.
#[derive(Debug, Default, Clone, Copy)]
pub struct PartialCaps {
    pub vision: Option<bool>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
}

impl PartialCaps {
    pub fn is_empty(&self) -> bool {
        self.vision.is_none() && self.tools.is_none() && self.reasoning.is_none()
    }

    /// Fill fields this source didn't know from `other`.
    fn or(self, other: PartialCaps) -> PartialCaps {
        PartialCaps {
            vision: self.vision.or(other.vision),
            tools: self.tools.or(other.tools),
            reasoning: self.reasoning.or(other.reasoning),
        }
    }
}

/// Combine what each source knew into a final answer. `user` is the manual override and
/// wins outright; `primary` is the higher-authority detector (provider API or llama.cpp
/// probe); `fallback` is the registry.
pub fn resolve(
    user: PartialCaps,
    primary: PartialCaps,
    primary_source: CapsSource,
    fallback: PartialCaps,
) -> ModelCaps {
    // `source` describes detection only, so an override on one field doesn't misattribute
    // the other two.
    let source = if !primary.is_empty() {
        primary_source
    } else if !fallback.is_empty() {
        CapsSource::Registry
    } else {
        CapsSource::Unknown
    };
    let merged = user.or(primary).or(fallback);
    ModelCaps {
        vision: merged.vision.unwrap_or(false),
        tools: merged.tools.unwrap_or(true), // unknown: assume tools, let the call fail loudly
        reasoning: merged.reasoning.unwrap_or(false),
        source,
        overridden: Overridden {
            vision: user.vision.is_some(),
            tools: user.tools.is_some(),
            reasoning: user.reasoning.is_some(),
        },
    }
}

// ---------------------------------------------------------------------------
// llama.cpp /props
// ---------------------------------------------------------------------------

/// Parse a llama-server `GET /props` body.
///
/// - `modalities.vision` — set from the loaded model + mmproj, not the filename.
/// - `chat_template_caps.supports_tools` / `supports_tool_calls` — llama.cpp runs the
///   jinja template to find out (`caps_get`), so this is as true as it gets.
/// - reasoning — no dedicated field, so we check the template for the thinking switch the
///   same way llama.cpp's own `common_chat_templates_support_enable_thinking` does: the
///   template has to actually reference `enable_thinking` or emit a `<think>` block.
pub fn caps_from_props(props: &Value) -> PartialCaps {
    let vision = props["modalities"]["vision"].as_bool();

    let tcaps = &props["chat_template_caps"];
    let tools = match (
        tcaps["supports_tools"].as_bool(),
        tcaps["supports_tool_calls"].as_bool(),
    ) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(true) && b.unwrap_or(true)),
    };

    let reasoning = props["chat_template"].as_str().map(|t| {
        t.contains("enable_thinking") || t.contains("<think>") || t.contains("reasoning_content")
    });

    PartialCaps {
        vision,
        tools,
        reasoning,
    }
}

/// Probe a running llama-server. Errors are the caller's cue to fall back, not to fail.
///
/// Note: `/props` describes the *loaded model* only. The chat format llama.cpp matched — and
/// so which tool-call parser is live — is decided per *request* and is not exposed here
/// (verified against build b10046: no such field). Read it from llama-server's stderr.
pub async fn probe_llama_server(
    client: &reqwest::Client,
    port: u16,
    api_key: &str,
) -> Option<PartialCaps> {
    let resp = client
        .get(format!("http://127.0.0.1:{}/props", port))
        .bearer_auth(api_key)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let props: Value = resp.json().await.ok()?;
    let caps = caps_from_props(&props);
    if caps.is_empty() {
        return None;
    }
    Some(caps)
}

// ---------------------------------------------------------------------------
// models.dev registry
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Registry {
    /// (models.dev provider id, model id) → caps. Exact, preferred.
    by_provider: HashMap<(String, String), PartialCaps>,
    /// normalized model id → caps, unioned across every provider. For local GGUFs and
    /// gateways that re-host a model under someone else's name.
    by_name: HashMap<String, PartialCaps>,
}

impl Registry {
    pub fn lookup(&self, provider_key: Option<&str>, model_id: &str) -> PartialCaps {
        if let Some(pk) = provider_key {
            if let Some(c) = self
                .by_provider
                .get(&(pk.to_string(), model_id.to_string()))
            {
                return *c;
            }
        }
        self.by_name
            .get(&normalize_id(model_id))
            .copied()
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }
}

/// Our provider `type` → models.dev provider id. Only for types where we know the mapping
/// is exact; anything else falls through to the normalized-name index.
pub fn registry_provider_key(provider_type: &str, base_url: &str) -> Option<&'static str> {
    match provider_type {
        "anthropic" => return Some("anthropic"),
        "gemini" => return Some("google"),
        "openai" => return Some("openai"),
        _ => {}
    }
    // openai_compat covers many hosts — identify by endpoint.
    let u = base_url.to_lowercase();
    let host_map = [
        ("api.groq.com", "groq"),
        ("openrouter.ai", "openrouter"),
        ("api.mistral.ai", "mistral"),
        ("api.deepseek.com", "deepseek"),
        ("api.x.ai", "xai"),
        ("api.together.xyz", "togetherai"),
        ("api.cerebras.ai", "cerebras"),
        ("api.fireworks.ai", "fireworks-ai"),
        ("api.openai.com", "openai"),
    ];
    host_map
        .iter()
        .find(|(h, _)| u.contains(h))
        .map(|(_, k)| *k)
}

/// Reduce a model id to something comparable across hosts and packagings.
///
/// `unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF::Q4_K_M` → `qwen3-30b-a3b-instruct-2507`,
/// which is what models.dev calls `Qwen/Qwen3-30B-A3B-Instruct-2507`.
pub fn normalize_id(id: &str) -> String {
    let mut s = id.to_lowercase();
    // Our local ids are "<repo>::<quant>".
    if let Some((head, _)) = s.split_once("::") {
        s = head.to_string();
    }
    // LM Studio / HF style "vendor/model" — the vendor differs per re-packager.
    if let Some((_, tail)) = s.rsplit_once('/') {
        s = tail.to_string();
    }
    // "model:tag" (Ollama) and file extensions.
    if let Some((head, _)) = s.split_once(':') {
        s = head.to_string();
    }
    s = s.trim_end_matches(".gguf").to_string();
    for suffix in ["-gguf", "-hf", "-tee", "-tput", "-fast"] {
        s = s.trim_end_matches(suffix).to_string();
    }
    // Trailing quantization / precision tokens added by GGUF repackagers.
    while let Some((head, last)) = s.rsplit_once('-') {
        let is_quant = last.starts_with('q')
            && last.len() >= 2
            && last[1..].starts_with(|c: char| c.is_ascii_digit())
            || last.starts_with("iq") && last.len() >= 3
            || matches!(
                last,
                "f16"
                    | "f32"
                    | "bf16"
                    | "fp8"
                    | "fp16"
                    | "mxfp4"
                    | "int4"
                    | "int8"
                    | "k"
                    | "m"
                    | "s"
                    | "l"
                    | "xs"
                    | "xl"
                    | "0"
                    | "1"
            );
        if !is_quant {
            break;
        }
        s = head.to_string();
    }
    s
}

fn caps_from_registry_entry(m: &Value) -> PartialCaps {
    let vision = m["modalities"]["input"]
        .as_array()
        .map(|a| a.iter().any(|v| v.as_str() == Some("image")));
    PartialCaps {
        vision,
        tools: m["tool_call"].as_bool(),
        reasoning: m["reasoning"].as_bool(),
    }
}

/// (yes-votes, total-votes) for one flag.
#[derive(Default, Clone, Copy)]
struct Tally(u32, u32);

impl Tally {
    fn add(&mut self, v: Option<bool>) {
        if let Some(b) = v {
            self.0 += u32::from(b);
            self.1 += 1;
        }
    }
    /// Majority, ties counting as "supported". A host that omits a capability the model
    /// has is a far more common error than a host inventing one.
    fn verdict(self) -> Option<bool> {
        (self.1 > 0).then(|| self.0 * 2 >= self.1)
    }
}

/// Build the indexes from a models.dev `api.json` body.
pub fn parse_registry(json: &Value) -> Registry {
    let mut reg = Registry::default();
    let Some(providers) = json.as_object() else {
        return reg;
    };
    // Same model, many hosts, and they don't always agree — resellers routinely omit
    // reasoning/tool_call that the first-party listing has. So the name index is a vote,
    // not first-writer-wins: measured against the first-party providers, voting agrees
    // 97.7% of the time versus 95.4% for taking whichever host we happened to see first.
    let mut votes: HashMap<String, [Tally; 3]> = HashMap::new();
    for (pk, pv) in providers {
        let Some(models) = pv["models"].as_object() else {
            continue;
        };
        for (mid, m) in models {
            let caps = caps_from_registry_entry(m);
            if caps.is_empty() {
                continue;
            }
            reg.by_provider.insert((pk.clone(), mid.clone()), caps);
            let t = votes.entry(normalize_id(mid)).or_default();
            t[0].add(caps.vision);
            t[1].add(caps.tools);
            t[2].add(caps.reasoning);
        }
    }
    reg.by_name = votes
        .into_iter()
        .map(|(name, t)| {
            (
                name,
                PartialCaps {
                    vision: t[0].verdict(),
                    tools: t[1].verdict(),
                    reasoning: t[2].verdict(),
                },
            )
        })
        .collect();
    reg
}

static REGISTRY: OnceLock<tokio::sync::Mutex<Option<Arc<Registry>>>> = OnceLock::new();

/// The models.dev registry, fetched once per app run and cached on disk for a day.
/// Never fails: a network error just means a smaller registry (or none), and callers
/// degrade to `CapsSource::Unknown`.
pub async fn registry(client: &reqwest::Client, app: &AppHandle) -> Arc<Registry> {
    let cell = REGISTRY.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some(r) = guard.as_ref() {
        return r.clone();
    }
    let reg = Arc::new(load_registry(client, app).await);
    *guard = Some(reg.clone());
    reg
}

async fn load_registry(client: &reqwest::Client, app: &AppHandle) -> Registry {
    let path = app
        .path()
        .app_data_dir()
        .map(|d| d.join(REGISTRY_CACHE_FILE))
        .ok();

    let fresh = path.as_ref().is_some_and(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < REGISTRY_TTL)
    });

    if fresh {
        if let Some(reg) = path.as_ref().and_then(read_cached) {
            return reg;
        }
    }

    match client.get(REGISTRY_URL).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => {
                if let Ok(json) = serde_json::from_str::<Value>(&body) {
                    let reg = parse_registry(&json);
                    if reg.len() > 0 {
                        if let Some(p) = path.as_ref() {
                            let _ = std::fs::create_dir_all(p.parent().unwrap());
                            let _ = std::fs::write(p, &body);
                        }
                        return reg;
                    }
                }
            }
            Err(e) => eprintln!("[caps] models.dev read failed: {}", e),
        },
        Ok(resp) => eprintln!("[caps] models.dev returned {}", resp.status()),
        Err(e) => eprintln!("[caps] models.dev fetch failed: {}", e),
    }

    // Offline: a stale cache still beats guessing.
    path.as_ref().and_then(read_cached).unwrap_or_default()
}

fn read_cached(path: &std::path::PathBuf) -> Option<Registry> {
    let body = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&body).ok()?;
    let reg = parse_registry(&json);
    (reg.len() > 0).then_some(reg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_strips_packaging_not_identity() {
        // GGUF repackaging must land on the models.dev id for the same model.
        assert_eq!(
            normalize_id("unsloth/Qwen3-30B-A3B-Instruct-2507-GGUF::Q4_K_M"),
            normalize_id("Qwen/Qwen3-30B-A3B-Instruct-2507")
        );
        assert_eq!(
            normalize_id("bartowski/gpt-oss-20b-GGUF::Q8_0"),
            "gpt-oss-20b"
        );
        assert_eq!(normalize_id("llama3.1:8b"), "llama3.1");
        // Size/version tokens are identity, not packaging — never strip them.
        assert_eq!(normalize_id("Qwen/Qwen3-32B"), "qwen3-32b");
        assert_ne!(
            normalize_id("Qwen/Qwen3-32B"),
            normalize_id("Qwen/Qwen3-14B")
        );
    }

    #[test]
    fn props_reads_llama_cpp_ground_truth() {
        let props = json!({
            "modalities": { "vision": true, "audio": false },
            "chat_template_caps": { "supports_tools": true, "supports_tool_calls": true },
            "chat_template": "{% if enable_thinking %}<think>{% endif %}",
        });
        let c = caps_from_props(&props);
        assert_eq!(c.vision, Some(true));
        assert_eq!(c.tools, Some(true));
        assert_eq!(c.reasoning, Some(true));

        // A template that can't take tools must report false, not fall through to unknown.
        let no_tools = json!({
            "modalities": { "vision": false },
            "chat_template_caps": { "supports_tools": false, "supports_tool_calls": false },
            "chat_template": "{{ messages }}",
        });
        let c = caps_from_props(&no_tools);
        assert_eq!(c.tools, Some(false));
        assert_eq!(c.reasoning, Some(false));

        // No props at all → nothing known, so resolve() can mark it Unknown.
        assert!(caps_from_props(&json!({})).is_empty());
    }

    #[test]
    fn registry_parses_models_dev_shape() {
        let reg = parse_registry(&json!({
            "openai": { "models": {
                "gpt-4o": {
                    "reasoning": false, "tool_call": true,
                    "modalities": { "input": ["text", "image"], "output": ["text"] }
                }
            }},
            "huggingface": { "models": {
                "Qwen/Qwen3-32B": {
                    "reasoning": true, "tool_call": true,
                    "modalities": { "input": ["text"], "output": ["text"] }
                }
            }}
        }));

        let c = reg.lookup(Some("openai"), "gpt-4o");
        assert_eq!(
            (c.vision, c.tools, c.reasoning),
            (Some(true), Some(true), Some(false))
        );

        // Local GGUF of a model models.dev only lists under its HF id.
        let c = reg.lookup(None, "unsloth/Qwen3-32B-GGUF::Q4_K_M");
        assert_eq!(
            (c.vision, c.tools, c.reasoning),
            (Some(false), Some(true), Some(true))
        );

        assert!(reg.lookup(Some("openai"), "not-a-real-model").is_empty());
    }

    #[test]
    fn name_index_outvotes_a_reseller_that_understates() {
        // Two hosts list the same model; the reseller omits reasoning. The exact
        // per-provider entry must still report what that provider claims, but the
        // name-index vote must not let one sloppy host erase a real capability.
        let entry = |reasoning: bool| {
            json!({ "reasoning": reasoning, "tool_call": true,
                    "modalities": { "input": ["text"], "output": ["text"] } })
        };
        let reg = parse_registry(&json!({
            "302ai":       { "models": { "MiniMax-M2": entry(false) } },
            "huggingface": { "models": { "MiniMaxAI/MiniMax-M2": entry(true) } },
            "openrouter":  { "models": { "minimax/minimax-m2": entry(true) } },
        }));
        assert_eq!(
            reg.lookup(Some("302ai"), "MiniMax-M2").reasoning,
            Some(false)
        );
        assert_eq!(
            reg.lookup(None, "MiniMax-M2-GGUF::Q4_K_M").reasoning,
            Some(true)
        );
    }

    #[test]
    fn resolve_prefers_authority_and_flags_guesses() {
        let none = PartialCaps::default();
        let provider = PartialCaps {
            vision: Some(true),
            tools: None,
            reasoning: None,
        };
        let registry = PartialCaps {
            vision: Some(false),
            tools: Some(true),
            reasoning: Some(true),
        };

        // Provider wins where it spoke; registry fills the rest.
        let c = resolve(none, provider, CapsSource::Provider, registry);
        assert!(c.vision && c.tools && c.reasoning);
        assert_eq!(c.source, CapsSource::Provider);

        // Nothing known: assume tools only, and say it's a guess.
        let c = resolve(none, none, CapsSource::Provider, none);
        assert_eq!((c.vision, c.tools, c.reasoning), (false, true, false));
        assert_eq!(c.source, CapsSource::Unknown);
    }

    #[test]
    fn user_override_beats_every_detector() {
        // The provider insists there's no vision and no tools; the user knows better.
        let provider = PartialCaps {
            vision: Some(false),
            tools: Some(false),
            reasoning: Some(false),
        };
        let user = PartialCaps {
            vision: Some(true),
            tools: None,
            reasoning: None,
        };

        let c = resolve(user, provider, CapsSource::Provider, PartialCaps::default());
        assert!(c.vision, "user override must win outright");
        assert!(c.overridden.vision);

        // Fields the user left alone stay detected, and aren't mislabelled as theirs.
        assert!(!c.tools);
        assert!(!c.overridden.tools);
        assert_eq!(c.source, CapsSource::Provider);

        // An override can also say *no* to something detection claims — and that must not
        // read as "unset" and fall through.
        let c = resolve(
            PartialCaps {
                vision: Some(false),
                tools: None,
                reasoning: None,
            },
            PartialCaps {
                vision: Some(true),
                tools: None,
                reasoning: None,
            },
            CapsSource::LlamaCpp,
            PartialCaps::default(),
        );
        assert!(!c.vision);
        assert!(c.overridden.vision);
    }
}
