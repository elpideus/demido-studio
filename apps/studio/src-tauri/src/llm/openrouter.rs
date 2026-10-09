//! OpenRouter: models from many labs behind one OpenAI-compatible API. Chats go through
//! [`super::openai::OpenAiClient`] in its OpenRouter dialect; this module checks keys and reads
//! the list of models, which says what each model can do and what it costs.

use chrono::{DateTime, Datelike, Days, Months, NaiveTime, Utc};
use serde_json::Value;

use super::openai::error_message;
use super::{Allowance, AllowanceKind, CloudModel, LlmError};
use crate::models::capabilities::Capabilities;

pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// OpenRouter's own routers (`openrouter/auto`, `openrouter/free`...) pick another model for
/// each request.
pub fn is_router(model: &str) -> bool {
    model.starts_with("openrouter/")
}

/// Checks the key, then lists the models that answer in text.
pub async fn list_models(http: &reqwest::Client, base_url: &str, api_key: &str) -> Result<Vec<CloudModel>, LlmError> {
    let base = base_url.trim_end_matches('/');
    // Anyone can read the list of models, so the key is checked on its own.
    get(http, &format!("{base}/key"), api_key).await?;
    let list = get(http, &format!("{base}/models"), api_key).await?;
    Ok(parse_models(&list))
}

/// What the key has left, from `GET /key`: today's requests to free models and, when the key has
/// a credit limit, its credit. The account's own balance needs a management key, so a key without
/// a limit reports only the free requests.
pub async fn allowances(http: &reqwest::Client, base_url: &str, api_key: &str) -> Result<Vec<Allowance>, LlmError> {
    let key = get(http, &format!("{}/key", base_url.trim_end_matches('/')), api_key).await?;
    Ok(parse_allowances(&key["data"], Utc::now()))
}

fn parse_allowances(key: &Value, now: DateTime<Utc>) -> Vec<Allowance> {
    let mut allowances = Vec::new();
    let free = &key["free_model_daily_requests"];
    if let (Some(remaining), Some(limit)) = (free["remaining"].as_f64(), free["limit"].as_f64()) {
        allowances.push(Allowance {
            kind: AllowanceKind::FreeRequests,
            remaining,
            limit,
            resets_at: next_reset(Some("daily"), now),
        });
    }
    // A key without a limit has `null` for both.
    if let (Some(remaining), Some(limit)) = (key["limit_remaining"].as_f64(), key["limit"].as_f64()) {
        allowances.push(Allowance {
            kind: AllowanceKind::KeyCredit,
            remaining,
            limit,
            resets_at: next_reset(key["limit_reset"].as_str(), now),
        });
    }
    allowances
}

/// OpenRouter's limits reset at midnight UTC: every day, every Monday or on the first of the
/// month. `None` for a limit that never resets.
fn next_reset(period: Option<&str>, now: DateTime<Utc>) -> Option<i64> {
    let today = now.date_naive();
    let next = match period? {
        "daily" => today + Days::new(1),
        "weekly" => today + Days::new(7 - u64::from(today.weekday().num_days_from_monday())),
        "monthly" => today.with_day(1)? + Months::new(1),
        _ => return None,
    };
    Some(next.and_time(NaiveTime::MIN).and_utc().timestamp_millis())
}

async fn get(http: &reqwest::Client, url: &str, api_key: &str) -> Result<Value, LlmError> {
    let resp = http.get(url).bearer_auth(api_key).send().await?;
    let status = resp.status().as_u16();
    let text = resp.text().await?;
    if !(200..300).contains(&status) {
        return Err(LlmError::Provider(error_message(status, &text)));
    }
    serde_json::from_str(&text).map_err(|e| LlmError::Provider(e.to_string()))
}

fn parse_models(list: &Value) -> Vec<CloudModel> {
    let mut models: Vec<CloudModel> = list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_model)
        .collect();
    models.sort_by_cached_key(|m| m.display_name.to_lowercase());
    models
}

/// `None` for a model that draws or speaks: a chat shows only its text.
fn parse_model(m: &Value) -> Option<CloudModel> {
    fn strings(v: &Value) -> Vec<&str> {
        v.as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }
    let id = m["id"].as_str()?;
    let input = strings(&m["architecture"]["input_modalities"]);
    let output = strings(&m["architecture"]["output_modalities"]);
    let params = strings(&m["supported_parameters"]);
    // A router lists everything the models it picks from can make; it answers a chat in text.
    if !output.contains(&"text") || (!is_router(id) && output.iter().any(|o| *o != "text")) {
        return None;
    }
    // Prices are per token, as text; a router of paid models has "-1" (it varies).
    let price = |key: &str| m["pricing"][key].as_str().and_then(|p| p.parse::<f64>().ok());
    let thinking = params.contains(&"reasoning") || params.contains(&"include_reasoning");
    Some(CloudModel {
        id: id.to_string(),
        display_name: m["name"].as_str().unwrap_or(id).to_string(),
        description: m["description"].as_str().unwrap_or_default().to_string(),
        input_token_limit: m["context_length"].as_u64().unwrap_or(0),
        output_token_limit: m["top_provider"]["max_completion_tokens"].as_u64().unwrap_or(0),
        thinking,
        always_thinks: m["reasoning"]["mandatory"].as_bool().unwrap_or(false),
        capabilities: Some(Capabilities {
            vision: Some(input.contains(&"image")),
            audio: Some(input.contains(&"audio")),
            tools: Some(params.contains(&"tools")),
            thinking: Some(thinking),
        }),
        free: price("prompt") == Some(0.0) && price("completion") == Some(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_what_models_can_do_and_cost() {
        // Trimmed from GET /api/v1/models, October 2026.
        let list = json!({"data": [
            {
                "id": "stepfun/step-5-preview",
                "name": "StepFun: Step 5 Preview",
                "description": "Flagship agentic model.",
                "context_length": 1000000,
                "architecture": {"input_modalities": ["text", "image", "video"], "output_modalities": ["text"]},
                "pricing": {"prompt": "0.000001", "completion": "0.0000027"},
                "top_provider": {"context_length": 1000000, "max_completion_tokens": 64000},
                "supported_parameters": ["max_tokens", "reasoning", "temperature", "tools"],
                "reasoning": {"mandatory": true, "supported_efforts": ["high", "medium", "low"]}
            },
            {
                "id": "google/gemma-4-31b-it:free",
                "name": "Google: Gemma 4 31B (free)",
                "context_length": 262144,
                "architecture": {"input_modalities": ["image", "text", "video"], "output_modalities": ["text"]},
                "pricing": {"prompt": "0", "completion": "0"},
                "top_provider": {"max_completion_tokens": 32768},
                "supported_parameters": ["include_reasoning", "max_tokens", "reasoning", "tools"],
                "reasoning": {"mandatory": false, "default_enabled": false}
            },
            {
                "id": "openrouter/auto",
                "name": "Auto Router",
                "context_length": 2000000,
                "architecture": {"input_modalities": ["text", "image", "audio"], "output_modalities": ["text", "image"]},
                "pricing": {"prompt": "-1", "completion": "-1"},
                "supported_parameters": ["tools", "temperature"]
            },
            {
                "id": "google/gemini-3-pro-image",
                "name": "Google: Nano Banana Pro",
                "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["image", "text"]},
                "pricing": {"prompt": "0.000002", "completion": "0.000012"}
            },
            {
                "id": "openai/gpt-audio",
                "name": "OpenAI: GPT Audio",
                "architecture": {"input_modalities": ["text", "audio"], "output_modalities": ["text", "audio"]},
                "pricing": {"prompt": "0.0000025", "completion": "0.00001"}
            }
        ]});
        let models = parse_models(&list);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "openrouter/auto",
                "google/gemma-4-31b-it:free",
                "stepfun/step-5-preview"
            ]
        );

        let step = &models[2];
        assert!(!step.free && step.thinking && step.always_thinks);
        assert_eq!((step.input_token_limit, step.output_token_limit), (1_000_000, 64_000));
        assert_eq!(
            step.capabilities,
            Some(Capabilities {
                vision: Some(true),
                audio: Some(false),
                tools: Some(true),
                thinking: Some(true)
            })
        );

        let gemma = &models[1];
        assert!(gemma.free && gemma.thinking && !gemma.always_thinks);

        let auto = &models[0];
        assert!(!auto.free && !auto.thinking);
        assert_eq!(auto.capabilities.and_then(|c| c.audio), Some(true));
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    fn ms(rfc3339: &str) -> Option<i64> {
        Some(at(rfc3339).timestamp_millis())
    }

    #[test]
    fn reads_what_a_key_has_left() {
        let now = at("2026-10-09T15:30:00Z");
        let limited = json!({
            "label": "sk-or-v1-abc...xyz",
            "limit": 10, "limit_remaining": 7.25, "limit_reset": "monthly",
            "usage": 42.5, "usage_daily": 0.5, "is_free_tier": false,
            "free_model_daily_requests": {"used": 12, "limit": 1000, "remaining": 988}
        });
        assert_eq!(
            parse_allowances(&limited, now),
            [
                Allowance {
                    kind: AllowanceKind::FreeRequests,
                    remaining: 988.0,
                    limit: 1000.0,
                    resets_at: ms("2026-10-10T00:00:00Z"),
                },
                Allowance {
                    kind: AllowanceKind::KeyCredit,
                    remaining: 7.25,
                    limit: 10.0,
                    resets_at: ms("2026-11-01T00:00:00Z"),
                },
            ]
        );

        let unlimited = json!({"limit": null, "limit_remaining": null, "limit_reset": null,
            "free_model_daily_requests": {"used": 50, "limit": 50, "remaining": 0}});
        let only_free = parse_allowances(&unlimited, now);
        assert_eq!(only_free.len(), 1);
        assert_eq!((only_free[0].remaining, only_free[0].limit), (0.0, 50.0));

        assert!(parse_allowances(&json!({"label": "old"}), now).is_empty());
    }

    #[test]
    fn limits_reset_at_midnight_utc() {
        // A Friday.
        let now = at("2026-10-09T23:59:59Z");
        assert_eq!(next_reset(Some("daily"), now), ms("2026-10-10T00:00:00Z"));
        assert_eq!(next_reset(Some("weekly"), now), ms("2026-10-12T00:00:00Z"));
        assert_eq!(
            next_reset(Some("monthly"), at("2026-12-31T08:00:00Z")),
            ms("2027-01-01T00:00:00Z")
        );
        // On a Monday the week has just begun.
        assert_eq!(
            next_reset(Some("weekly"), at("2026-10-12T00:00:00Z")),
            ms("2026-10-19T00:00:00Z")
        );
        assert_eq!(next_reset(None, now), None);
        assert_eq!(next_reset(Some("yearly"), now), None);
    }
}
