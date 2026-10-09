//! Trying a request again when the model is busy for a moment: overloaded, rate limited per
//! minute, or briefly unreachable. [`super::Client::stream`] waits and sends it again, up to
//! [`RETRIES`] times, before the error reaches the person.

use std::time::Duration;

use serde_json::Value;

/// Retries after the first attempt.
pub const RETRIES: u32 = 5;
/// Wait before the first retry. It doubles each time: 1, 2, 4, 8 and 16 seconds, about half a
/// minute in all, which is what a model that is busy "for a moment" usually needs.
const FIRST_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_secs(1)
};
/// Longest wait a server may ask for. One asking for more is out of quota (requests per day, say)
/// rather than busy, and waiting would only hold the chat up.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// How long to wait before retry `retry` (from 1): the backoff, a fifth more or less at random so
/// that requests turned away together do not all come back together, and never less than the
/// server asked for. `None` when the server asks for longer than is worth waiting.
pub fn wait_before(retry: u32, asked: Option<Duration>) -> Option<Duration> {
    if asked.is_some_and(too_long) {
        return None;
    }
    let backoff = FIRST_WAIT * 2u32.pow(retry.saturating_sub(1).min(10));
    let spread = (uuid::Uuid::new_v4().as_u128() % 401) as f64 / 1000.0 - 0.2;
    let jittered = backoff.mul_f64(1.0 + spread);
    Some(asked.map_or(jittered, |a| a.max(jittered)))
}

/// Whether a wait the server asks for is too long to make.
pub fn too_long(wait: Duration) -> bool {
    wait > LONGEST_WAIT
}

/// Whether an HTTP status means "busy, try again shortly". A local llama-server answers 503
/// while it loads; its other errors are the request's fault and would fail again.
pub fn busy_status(status: u16, local: bool) -> bool {
    if local {
        return status == 503;
    }
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// Whether a provider's words say it is busy, for errors that come without a usable status (in
/// the middle of a stream, say).
pub fn sounds_busy(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    [
        "overloaded",
        "high demand",
        "rate limit",
        "rate-limit",
        "temporarily",
        "try again later",
        "capacity",
        "unavailable",
        "too many requests",
    ]
    .iter()
    .any(|w| m.contains(w))
}

/// Whether a refusal lasts too long to wait out: a daily quota.
pub fn lasting(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    ["per-day", "per day", "perday", "daily"].iter().any(|w| m.contains(w))
}

/// The wait a response asks for: `Retry-After` (seconds or a date), or OpenRouter's
/// `X-RateLimit-Reset` (milliseconds since the epoch).
pub fn asked_by_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    if let Some(value) = text("retry-after") {
        if let Ok(seconds) = value.parse::<f64>() {
            return Some(Duration::from_secs_f64(seconds.max(0.0)));
        }
        if let Ok(at) = chrono::DateTime::parse_from_rfc2822(value) {
            return Some(until_ms(at.timestamp_millis()));
        }
    }
    text("x-ratelimit-reset")
        .and_then(|v| v.parse::<i64>().ok())
        .map(until_ms)
}

/// The wait an error body asks for: Gemini's `RetryInfo` detail ("retryDelay": "37s"), or the
/// rate limit headers OpenRouter copies into `metadata.headers`.
pub fn asked_by_body(error: &Value) -> Option<Duration> {
    let delay = error["details"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["retryDelay"].as_str())
        .and_then(|d| d.trim().trim_end_matches('s').parse::<f64>().ok());
    if let Some(seconds) = delay {
        return Some(Duration::from_secs_f64(seconds.max(0.0)));
    }
    let headers = &error["metadata"]["headers"];
    let reset = headers
        .get("X-RateLimit-Reset")
        .or_else(|| headers.get("x-ratelimit-reset"))?;
    let ms = reset.as_i64().or_else(|| reset.as_str()?.trim().parse().ok())?;
    Some(until_ms(ms))
}

fn until_ms(epoch_ms: i64) -> Duration {
    Duration::from_millis((epoch_ms - chrono::Utc::now().timestamp_millis()).max(0) as u64)
}

/// "about 5 hours", for a wait too long to make.
pub fn rough(wait: Duration) -> String {
    let s = wait.as_secs();
    match s {
        0..=119 => format!("{s} seconds"),
        120..=7_199 => format!("about {} minutes", s.div_ceil(60)),
        _ => format!("about {} hours", s.div_ceil(3600)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn waits_double_and_respect_what_the_server_asks() {
        for retry in 1..=RETRIES {
            let base = FIRST_WAIT * 2u32.pow(retry - 1);
            let wait = wait_before(retry, None).unwrap();
            assert!(
                wait >= base.mul_f64(0.79) && wait <= base.mul_f64(1.21),
                "{retry}: {wait:?}"
            );
        }
        assert!(wait_before(1, Some(Duration::from_secs(3))).unwrap() >= Duration::from_secs(3));
        assert_eq!(wait_before(1, Some(Duration::from_secs(3600))), None);
    }

    #[test]
    fn busy_is_told_apart() {
        assert!(busy_status(429, false) && busy_status(503, false) && busy_status(529, false));
        assert!(!busy_status(400, false) && !busy_status(401, false) && !busy_status(402, false));
        assert!(busy_status(503, true) && !busy_status(500, true));
        assert!(sounds_busy("This model is currently experiencing high demand."));
        assert!(sounds_busy("Venice: m:free is temporarily rate-limited upstream."));
        assert!(!sounds_busy("Function calling is not enabled for this model."));
        assert!(lasting("Rate limit exceeded: free-models-per-day."));
        assert!(!lasting("Rate limit exceeded: free-models-per-min."));
    }

    #[test]
    fn asked_waits_are_read() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "7".parse().unwrap());
        assert_eq!(asked_by_headers(&headers), Some(Duration::from_secs(7)));
        let gemini = json!({"code": 429, "details": [
            {"@type": "type.googleapis.com/google.rpc.QuotaFailure"},
            {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "37s"}
        ]});
        assert_eq!(asked_by_body(&gemini), Some(Duration::from_secs(37)));
        let reset = chrono::Utc::now().timestamp_millis() + 5 * 3_600_000;
        let openrouter = json!({"code": 429, "metadata": {"headers": {"X-RateLimit-Reset": reset.to_string()}}});
        let wait = asked_by_body(&openrouter).unwrap();
        assert!(wait > Duration::from_secs(4 * 3600), "{wait:?}");
        assert_eq!(rough(wait), "about 5 hours");
        assert_eq!(asked_by_body(&json!({"message": "x"})), None);
    }
}
