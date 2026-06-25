use crate::db::accounts::Account;
use std::sync::{Arc, Mutex};

/// Refreshes the access token if expired. Returns updated token.
/// Takes `Arc<Mutex<Connection>>` and drops the lock before any await.
pub async fn ensure_token(
    http: &reqwest::Client,
    conn: &Arc<Mutex<rusqlite::Connection>>,
    account: &mut Account,
    client_id: &str,
    client_secret: &str,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp();
    let expiry = account.token_expiry.unwrap_or(0);
    if expiry > now + 60 {
        return Ok(());
    }
    let refresh = account
        .refresh_token
        .as_deref()
        .ok_or("No refresh token stored")?
        .to_string();

    let resp: serde_json::Value = http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(e) = resp.get("error") {
        return Err(format!("Token refresh error: {}", e));
    }

    let new_token = resp["access_token"]
        .as_str()
        .ok_or("Missing access_token in refresh response")?
        .to_string();
    let expires_in = resp["expires_in"].as_i64().unwrap_or(3600);

    account.access_token = new_token.clone();
    account.token_expiry = Some(now + expires_in);

    // Persist updated token (lock briefly, then release)
    {
        let conn_guard = conn.lock().unwrap();
        crate::db::accounts::upsert(&conn_guard, account).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Find first account that has the given service.
pub fn find_account_for_service(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    service: &str,
) -> Result<Option<Account>, String> {
    let conn_guard = conn.lock().unwrap();
    let accounts = crate::db::accounts::list(&conn_guard).map_err(|e| e.to_string())?;
    Ok(accounts.into_iter().find(|a| a.services.contains(&service.to_string())))
}

// ── Gmail ─────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct EmailSummary {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub snippet: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EmailPage {
    pub emails: Vec<EmailSummary>,
    pub next_page_token: Option<String>,
}

pub async fn list_emails(
    http: &reqwest::Client,
    token: &str,
    query: &str,
    max: u64,
    page_token: Option<&str>,
) -> Result<EmailPage, String> {
    let pt = page_token.map(|t| format!("&pageToken={}", urlencoded(t))).unwrap_or_default();
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}&q={}{}",
        max,
        urlencoded(query),
        pt,
    );
    let list: serde_json::Value = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let next_page_token = list["nextPageToken"].as_str().map(|s| s.to_string());

    let ids: Vec<String> = list["messages"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    let mut summaries = Vec::new();
    for id in ids.iter().take(max as usize) {
        let msg_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
            id
        );
        let msg: serde_json::Value = http
            .get(&msg_url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let headers = msg["payload"]["headers"].as_array();
        let get_header = |name: &str| -> String {
            headers
                .and_then(|hs| {
                    hs.iter().find(|h| {
                        h["name"].as_str().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false)
                    })
                })
                .and_then(|h| h["value"].as_str())
                .unwrap_or("")
                .to_string()
        };

        summaries.push(EmailSummary {
            id: id.clone(),
            subject: get_header("Subject"),
            from: get_header("From"),
            date: get_header("Date"),
            snippet: msg["snippet"].as_str().unwrap_or("").to_string(),
        });
    }

    Ok(EmailPage { emails: summaries, next_page_token })
}

pub async fn get_email_body(
    http: &reqwest::Client,
    token: &str,
    id: &str,
    html: bool,
) -> Result<String, String> {
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
        id
    );
    let msg: serde_json::Value = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Walk MIME parts; prefer `want` mime, fall back to the other text part.
    fn find_body(part: &serde_json::Value, want: &str) -> Option<String> {
        let mime = part["mimeType"].as_str().unwrap_or("");
        if mime == want {
            if let Some(data) = part["body"]["data"].as_str() {
                // Gmail returns base64url; some bodies carry `=` padding, others don't.
                // Decode padding-indifferently so a stray `=` doesn't blank the body.
                use base64::Engine;
                use base64::engine::{general_purpose::GeneralPurpose, GeneralPurposeConfig, DecodePaddingMode};
                let engine = GeneralPurpose::new(
                    &base64::alphabet::URL_SAFE,
                    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
                );
                if let Ok(bytes) = engine.decode(data) {
                    return Some(String::from_utf8_lossy(&bytes).into_owned());
                }
            }
        }
        if let Some(parts) = part["parts"].as_array() {
            for p in parts {
                if let Some(body) = find_body(p, want) {
                    return Some(body);
                }
            }
        }
        None
    }

    let primary = if html { "text/html" } else { "text/plain" };
    let fallback = if html { "text/plain" } else { "text/html" };
    let body = find_body(&msg["payload"], primary)
        .or_else(|| find_body(&msg["payload"], fallback))
        .unwrap_or_else(|| msg["snippet"].as_str().unwrap_or("(no body)").to_string());

    // For UI (html), return raw HTML untouched; for agent, strip tags + truncate.
    let body_out = if html {
        body
    } else {
        let text = crate::web::strip_html(&body);
        if text.len() > 8000 { format!("{}…[truncated]", &text[..8000]) } else { text }
    };

    let headers = msg["payload"]["headers"].as_array();
    let get_header = |name: &str| -> String {
        headers
            .and_then(|hs| hs.iter().find(|h| h["name"].as_str().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false)))
            .and_then(|h| h["value"].as_str())
            .unwrap_or("")
            .to_string()
    };

    Ok(format!(
        "From: {}\nTo: {}\nSubject: {}\nDate: {}\n\n{}",
        get_header("From"),
        get_header("To"),
        get_header("Subject"),
        get_header("Date"),
        body_out
    ))

}

// ── Calendar ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub start: String,
    pub end: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>, // hex background color, event-level overrides calendar-level
}

fn parse_events_with_cal_color(items: &[serde_json::Value], cal_color: Option<&str>) -> Vec<CalendarEvent> {
    items.iter().map(|e| {
        // Event-level colorId → hex takes priority; then event backgroundColor; then calendar color
        let color = e["colorId"].as_str()
            .and_then(google_color_id_to_hex)
            .map(|s| s.to_string())
            .or_else(|| e["backgroundColor"].as_str().map(|s| s.to_string()))
            .or_else(|| cal_color.map(|s| s.to_string()));
        CalendarEvent {
            id: e["id"].as_str().unwrap_or("").to_string(),
            summary: e["summary"].as_str().unwrap_or("(no title)").to_string(),
            start: e["start"]["dateTime"]
                .as_str()
                .or_else(|| e["start"]["date"].as_str())
                .unwrap_or("")
                .to_string(),
            end: e["end"]["dateTime"]
                .as_str()
                .or_else(|| e["end"]["date"].as_str())
                .unwrap_or("")
                .to_string(),
            location: e["location"].as_str().map(|s| s.to_string()),
            description: e["description"].as_str().map(|s| s.to_string()),
            color,
        }
    }).collect()
}

fn parse_events(items: &[serde_json::Value]) -> Vec<CalendarEvent> {
    parse_events_with_cal_color(items, None)
}

fn google_color_id_to_hex(id: &str) -> Option<&'static str> {
    // Source: GET https://www.googleapis.com/calendar/v3/colors (event palette)
    match id {
        "1"  => Some("#a4bdfc"), // Lavender
        "2"  => Some("#7ae28c"), // Sage
        "3"  => Some("#dbadff"), // Grape
        "4"  => Some("#ff887c"), // Flamingo
        "5"  => Some("#fbd75b"), // Banana
        "6"  => Some("#ffb878"), // Tangerine
        "7"  => Some("#46d6db"), // Peacock
        "8"  => Some("#e1e1e1"), // Graphite
        "9"  => Some("#5484ed"), // Blueberry
        "10" => Some("#51b749"), // Basil
        "11" => Some("#dc2127"), // Tomato
        _    => None,
    }
}

async fn fetch_calendar(
    http: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    time_min: &str,
    time_max: &str,
    max: u64,
) -> Result<Vec<CalendarEvent>, String> {
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events\
        ?maxResults={}&singleEvents=true&orderBy=startTime&timeMin={}&timeMax={}",
        urlencoded(calendar_id),
        max,
        urlencoded(time_min),
        urlencoded(time_max),
    );
    let resp: serde_json::Value = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let items = resp["items"].as_array().cloned().unwrap_or_default();
    Ok(parse_events(&items))
}

pub async fn list_events_all_calendars(
    http: &reqwest::Client,
    token: &str,
    time_min: &str,
    time_max: &str,
    max: u64,
) -> Result<Vec<CalendarEvent>, String> {
    // Get all calendar IDs the user has
    let cal_list: serde_json::Value = http
        .get("https://www.googleapis.com/calendar/v3/users/me/calendarList?maxResults=50")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let cal_entries: Vec<(String, Option<String>)> = cal_list["items"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|c| {
            c["id"].as_str().map(|id| (
                id.to_string(),
                c["backgroundColor"].as_str().map(|s| s.to_string()),
            ))
        })
        .collect();

    let per_cal = (max / cal_entries.len().max(1) as u64).max(50);
    let mut all: Vec<CalendarEvent> = Vec::new();

    for (cal_id, cal_color) in &cal_entries {
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events\
            ?maxResults={}&singleEvents=true&orderBy=startTime&timeMin={}&timeMax={}",
            urlencoded(cal_id), per_cal, urlencoded(time_min), urlencoded(time_max),
        );
        if let Ok(resp) = http.get(&url).bearer_auth(token).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let items = json["items"].as_array().cloned().unwrap_or_default();
                let mut evs = parse_events_with_cal_color(&items, cal_color.as_deref());
                all.append(&mut evs);
            }
        }
    }

    // Sort by start time
    all.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(all)
}

pub async fn list_events(
    http: &reqwest::Client,
    token: &str,
    time_min: &str,
    time_max: &str,
    max: u64,
) -> Result<Vec<CalendarEvent>, String> {
    fetch_calendar(http, token, "primary", time_min, time_max, max).await
}

pub async fn create_event(
    http: &reqwest::Client,
    token: &str,
    summary: &str,
    start: &str,
    end: &str,
    location: Option<&str>,
    description: Option<&str>,
    all_day: bool,
) -> Result<CalendarEvent, String> {
    let start_val = if all_day {
        serde_json::json!({ "date": start })
    } else {
        serde_json::json!({ "dateTime": start, "timeZone": "UTC" })
    };
    let end_val = if all_day {
        serde_json::json!({ "date": end })
    } else {
        serde_json::json!({ "dateTime": end, "timeZone": "UTC" })
    };
    let mut body = serde_json::json!({
        "summary": summary,
        "start": start_val,
        "end": end_val,
    });
    if let Some(loc) = location { body["location"] = serde_json::json!(loc); }
    if let Some(desc) = description { body["description"] = serde_json::json!(desc); }

    let resp: serde_json::Value = http
        .post("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(e) = resp.get("error") { return Err(format!("API error: {}", e)); }
    Ok(parse_events(&[resp])[0].clone())
}

pub async fn update_event(
    http: &reqwest::Client,
    token: &str,
    event_id: &str,
    summary: &str,
    start: &str,
    end: &str,
    location: Option<&str>,
    description: Option<&str>,
    all_day: bool,
) -> Result<CalendarEvent, String> {
    let start_val = if all_day {
        serde_json::json!({ "date": start })
    } else {
        serde_json::json!({ "dateTime": start, "timeZone": "UTC" })
    };
    let end_val = if all_day {
        serde_json::json!({ "date": end })
    } else {
        serde_json::json!({ "dateTime": end, "timeZone": "UTC" })
    };
    let mut body = serde_json::json!({
        "summary": summary,
        "start": start_val,
        "end": end_val,
    });
    if let Some(loc) = location { body["location"] = serde_json::json!(loc); }
    if let Some(desc) = description { body["description"] = serde_json::json!(desc); }

    let url = format!("https://www.googleapis.com/calendar/v3/calendars/primary/events/{}", urlencoded(event_id));
    let resp: serde_json::Value = http
        .put(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(e) = resp.get("error") { return Err(format!("API error: {}", e)); }
    Ok(parse_events(&[resp])[0].clone())
}

// ── Contacts ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Contact {
    pub name: String,
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub photo_url: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ContactsPage {
    pub contacts: Vec<Contact>,
    pub next_page_token: Option<String>,
}

pub async fn list_contacts(
    http: &reqwest::Client,
    token: &str,
    query: &str,
    max: u64,
    page_token: Option<&str>,
) -> Result<ContactsPage, String> {
    let pt = page_token.map(|t| format!("&pageToken={}", urlencoded(t))).unwrap_or_default();
    let url = if query.is_empty() {
        format!(
            "https://people.googleapis.com/v1/people/me/connections\
            ?personFields=names,emailAddresses,phoneNumbers,photos&pageSize={}&sortOrder=FIRST_NAME_ASCENDING{}",
            max, pt
        )
    } else {
        format!(
            "https://people.googleapis.com/v1/people:searchContacts\
            ?query={}&readMask=names,emailAddresses,phoneNumbers,photos&pageSize={}{}",
            urlencoded(query), max, pt
        )
    };

    let resp: serde_json::Value = http
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let next_page_token = resp["nextPageToken"].as_str().map(|s| s.to_string());

    let items = if query.is_empty() {
        resp["connections"].as_array().cloned().unwrap_or_default()
    } else {
        resp["results"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| r["person"].as_object().map(|o| serde_json::Value::Object(o.clone())))
            .collect()
    };

    let contacts = items.iter().map(|p| {
        let name = p["names"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|n| n["displayName"].as_str())
            .unwrap_or("(no name)")
            .to_string();
        let emails = p["emailAddresses"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|e| e["value"].as_str().map(|s| s.to_string()))
            .collect();
        let phones = p["phoneNumbers"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|e| e["value"].as_str().map(|s| s.to_string()))
            .collect();
        let photo_url = p["photos"]
            .as_array()
            .and_then(|a| a.iter().find(|ph| ph["default"].as_bool() != Some(true)))
            .and_then(|ph| ph["url"].as_str())
            .map(|s| s.to_string());
        Contact { name, emails, phones, photo_url }
    }).collect();
    Ok(ContactsPage { contacts, next_page_token })
}

fn urlencoded(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
