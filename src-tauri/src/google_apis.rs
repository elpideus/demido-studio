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
    pub unread: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EmailPage {
    pub emails: Vec<EmailSummary>,
    pub next_page_token: Option<String>,
    pub result_size_estimate: Option<i64>,
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
    let result_size_estimate = list["resultSizeEstimate"].as_i64();

    let ids: Vec<String> = list["messages"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    // ponytail: fetch metadata for all messages concurrently instead of one
    // await per email — was N sequential round trips, now N parallel ones.
    let fetches = ids.iter().take(max as usize).map(|id| {
        let http = http.clone();
        let token = token.to_string();
        let id = id.clone();
        async move {
            let msg_url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
                id
            );
            let msg: serde_json::Value = http
                .get(&msg_url)
                .bearer_auth(&token)
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

            let unread = msg["labelIds"]
                .as_array()
                .map(|labels| labels.iter().any(|l| l.as_str() == Some("UNREAD")))
                .unwrap_or(false);

            Ok::<EmailSummary, String>(EmailSummary {
                id: id.clone(),
                subject: get_header("Subject"),
                from: get_header("From"),
                date: get_header("Date"),
                snippet: msg["snippet"].as_str().unwrap_or("").to_string(),
                unread,
            })
        }
    });
    let summaries: Vec<EmailSummary> = futures_util::future::try_join_all(fetches).await?;

    Ok(EmailPage { emails: summaries, next_page_token, result_size_estimate })
}

pub async fn trash_message(http: &reqwest::Client, token: &str, id: &str) -> Result<(), String> {
    let url = format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/trash", id);
    let resp = http.post(&url).bearer_auth(token).json(&serde_json::json!({})).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Gmail trash failed: {}", resp.status()));
    }
    Ok(())
}

pub async fn set_message_read(http: &reqwest::Client, token: &str, id: &str, read: bool) -> Result<(), String> {
    let url = format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/modify", id);
    let body = if read {
        serde_json::json!({ "removeLabelIds": ["UNREAD"] })
    } else {
        serde_json::json!({ "addLabelIds": ["UNREAD"] })
    };
    let resp = http.post(&url).bearer_auth(token).json(&body).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Gmail modify failed: {}", resp.status()));
    }
    Ok(())
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

    let body_out = if html {
        body
    } else {
        crate::web::strip_html(&body)
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

    // ponytail: one calendar's events per await, run concurrently instead of
    // sequentially — was N round trips in series, now N in parallel.
    let fetches = cal_entries.iter().map(|(cal_id, cal_color)| {
        let http = http.clone();
        let token = token.to_string();
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events\
            ?maxResults={}&singleEvents=true&orderBy=startTime&timeMin={}&timeMax={}",
            urlencoded(cal_id), per_cal, urlencoded(time_min), urlencoded(time_max),
        );
        let cal_color = cal_color.clone();
        async move {
            if let Ok(resp) = http.get(&url).bearer_auth(&token).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let items = json["items"].as_array().cloned().unwrap_or_default();
                    return parse_events_with_cal_color(&items, cal_color.as_deref());
                }
            }
            Vec::new()
        }
    });
    let mut all: Vec<CalendarEvent> = futures_util::future::join_all(fetches)
        .await
        .into_iter()
        .flatten()
        .collect();

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

const CONTACT_FIELDS: &str = "names,emailAddresses,phoneNumbers,photos,birthdays,addresses,organizations,urls,biographies,nicknames,relations,events";

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct LabeledValue {
    pub value: String,
    pub label: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct ContactAddress {
    pub street: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
    pub label: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Contact {
    pub id: String,
    pub etag: String,
    pub display_name: String,
    pub given_name: String,
    pub family_name: String,
    pub middle_name: String,
    pub name_prefix: String,
    pub name_suffix: String,
    pub nickname: String,
    pub emails: Vec<LabeledValue>,
    pub phones: Vec<LabeledValue>,
    pub addresses: Vec<ContactAddress>,
    pub organization: String,
    pub job_title: String,
    pub department: String,
    pub birthday: Option<String>,
    pub anniversary: Option<String>,
    pub website: String,
    pub notes: String,
    pub photo_url: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ContactsPage {
    pub contacts: Vec<Contact>,
    pub next_page_token: Option<String>,
}

fn parse_contact_date(b: &serde_json::Value) -> Option<String> {
    let d = &b["date"];
    let month = d["month"].as_i64()?;
    let day = d["day"].as_i64()?;
    Some(if let Some(y) = d["year"].as_i64() {
        format!("{}-{:02}-{:02}", y, month, day)
    } else {
        format!("--{:02}-{:02}", month, day)
    })
}

fn parse_person(p: &serde_json::Value) -> Contact {
    let id = p["resourceName"].as_str().unwrap_or("").to_string();
    let etag = p["etag"].as_str().unwrap_or("").to_string();

    let name_obj = p["names"].as_array().and_then(|a| a.first());
    let display_name = name_obj.and_then(|n| n["displayName"].as_str()).unwrap_or("(no name)").to_string();
    let given_name = name_obj.and_then(|n| n["givenName"].as_str()).unwrap_or("").to_string();
    let family_name = name_obj.and_then(|n| n["familyName"].as_str()).unwrap_or("").to_string();
    let middle_name = name_obj.and_then(|n| n["middleName"].as_str()).unwrap_or("").to_string();
    let name_prefix = name_obj.and_then(|n| n["honorificPrefix"].as_str()).unwrap_or("").to_string();
    let name_suffix = name_obj.and_then(|n| n["honorificSuffix"].as_str()).unwrap_or("").to_string();

    let nickname = p["nicknames"].as_array().and_then(|a| a.first())
        .and_then(|n| n["value"].as_str()).unwrap_or("").to_string();

    let emails = p["emailAddresses"].as_array().unwrap_or(&vec![]).iter().map(|e| LabeledValue {
        value: e["value"].as_str().unwrap_or("").to_string(),
        label: e["formattedType"].as_str().or_else(|| e["type"].as_str()).unwrap_or("Other").to_string(),
    }).collect();

    let phones = p["phoneNumbers"].as_array().unwrap_or(&vec![]).iter().map(|e| LabeledValue {
        value: e["value"].as_str().unwrap_or("").to_string(),
        label: e["formattedType"].as_str().or_else(|| e["type"].as_str()).unwrap_or("Other").to_string(),
    }).collect();

    let addresses = p["addresses"].as_array().unwrap_or(&vec![]).iter().map(|a| ContactAddress {
        street: a["streetAddress"].as_str().unwrap_or("").to_string(),
        city: a["city"].as_str().unwrap_or("").to_string(),
        region: a["region"].as_str().unwrap_or("").to_string(),
        postal_code: a["postalCode"].as_str().unwrap_or("").to_string(),
        country: a["country"].as_str().unwrap_or("").to_string(),
        label: a["formattedType"].as_str().or_else(|| a["type"].as_str()).unwrap_or("Other").to_string(),
    }).collect();

    let org = p["organizations"].as_array().and_then(|a| a.first());
    let organization = org.and_then(|o| o["name"].as_str()).unwrap_or("").to_string();
    let job_title = org.and_then(|o| o["title"].as_str()).unwrap_or("").to_string();
    let department = org.and_then(|o| o["department"].as_str()).unwrap_or("").to_string();

    let birthday = p["birthdays"].as_array().and_then(|a| a.first()).and_then(parse_contact_date);

    let anniversary = p["events"].as_array().and_then(|a| {
        a.iter().find(|e| e["type"].as_str() == Some("anniversary"))
    }).and_then(parse_contact_date);

    let website = p["urls"].as_array().and_then(|a| a.first())
        .and_then(|u| u["value"].as_str()).unwrap_or("").to_string();

    let notes = p["biographies"].as_array().and_then(|a| a.first())
        .and_then(|b| b["value"].as_str()).unwrap_or("").to_string();

    let photo_url = p["photos"].as_array()
        .and_then(|a| a.iter().find(|ph| ph["default"].as_bool() != Some(true)))
        .and_then(|ph| ph["url"].as_str())
        .map(|s| s.to_string());

    Contact { id, etag, display_name, given_name, family_name, middle_name, name_prefix, name_suffix,
        nickname, emails, phones, addresses, organization, job_title, department,
        birthday, anniversary, website, notes, photo_url }
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
            ?personFields={}&pageSize={}&sortOrder=FIRST_NAME_ASCENDING{}",
            CONTACT_FIELDS, max, pt
        )
    } else {
        format!(
            "https://people.googleapis.com/v1/people:searchContacts\
            ?query={}&readMask={}&pageSize={}{}",
            urlencoded(query), CONTACT_FIELDS, max, pt
        )
    };

    let resp: serde_json::Value = http.get(&url).bearer_auth(token).send().await
        .map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;

    let next_page_token = resp["nextPageToken"].as_str().map(|s| s.to_string());

    let items: Vec<serde_json::Value> = if query.is_empty() {
        resp["connections"].as_array().cloned().unwrap_or_default()
    } else {
        resp["results"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .filter_map(|r| r["person"].as_object().map(|o| serde_json::Value::Object(o.clone())))
            .collect()
    };

    let contacts = items.iter().map(parse_person).collect();
    Ok(ContactsPage { contacts, next_page_token })
}

pub async fn get_contact(http: &reqwest::Client, token: &str, resource_name: &str) -> Result<Contact, String> {
    let url = format!(
        "https://people.googleapis.com/v1/{}?personFields={}",
        resource_name, CONTACT_FIELDS,
    );
    let p: serde_json::Value = http.get(&url).bearer_auth(token).send().await
        .map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    Ok(parse_person(&p))
}

pub async fn update_contact(http: &reqwest::Client, token: &str, contact: &Contact) -> Result<Contact, String> {
    let update_mask = "names,emailAddresses,phoneNumbers,addresses,organizations,birthdays,urls,biographies,nicknames";
    let url = format!(
        "https://people.googleapis.com/v1/{}:updateContact?updatePersonFields={}",
        contact.id, update_mask,
    );

    fn parse_date(s: &str) -> serde_json::Value {
        let parts: Vec<&str> = s.trim_start_matches('-').split('-').collect();
        let has_year = !s.starts_with("--");
        match parts.as_slice() {
            [y, m, d] if has_year => serde_json::json!({"year": y.parse::<i64>().unwrap_or(0), "month": m.parse::<i64>().unwrap_or(0), "day": d.parse::<i64>().unwrap_or(0)}),
            [m, d] => serde_json::json!({"month": m.parse::<i64>().unwrap_or(0), "day": d.parse::<i64>().unwrap_or(0)}),
            _ => serde_json::json!({}),
        }
    }

    let mut body = serde_json::json!({
        "etag": contact.etag,
        "names": [{"givenName": contact.given_name, "familyName": contact.family_name, "middleName": contact.middle_name, "honorificPrefix": contact.name_prefix, "honorificSuffix": contact.name_suffix}],
        "nicknames": if contact.nickname.is_empty() { serde_json::json!([]) } else { serde_json::json!([{"value": contact.nickname}]) },
        "emailAddresses": contact.emails.iter().map(|e| serde_json::json!({"value": e.value, "type": e.label.to_lowercase()})).collect::<Vec<_>>(),
        "phoneNumbers": contact.phones.iter().map(|p| serde_json::json!({"value": p.value, "type": p.label.to_lowercase()})).collect::<Vec<_>>(),
        "addresses": contact.addresses.iter().map(|a| serde_json::json!({"streetAddress": a.street, "city": a.city, "region": a.region, "postalCode": a.postal_code, "country": a.country, "type": a.label.to_lowercase()})).collect::<Vec<_>>(),
        "organizations": if contact.organization.is_empty() && contact.job_title.is_empty() { serde_json::json!([]) } else { serde_json::json!([{"name": contact.organization, "title": contact.job_title, "department": contact.department}]) },
        "urls": if contact.website.is_empty() { serde_json::json!([]) } else { serde_json::json!([{"value": contact.website}]) },
        "biographies": if contact.notes.is_empty() { serde_json::json!([]) } else { serde_json::json!([{"value": contact.notes, "contentType": "TEXT_PLAIN"}]) },
    });

    let mut birthdays = vec![];
    if let Some(ref bday) = contact.birthday {
        birthdays.push(serde_json::json!({"date": parse_date(bday)}));
    }
    body["birthdays"] = serde_json::json!(birthdays);

    let resp: serde_json::Value = http.patch(&url).bearer_auth(token).json(&body)
        .send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;

    if let Some(err) = resp["error"].as_object() {
        return Err(err["message"].as_str().unwrap_or("update failed").to_string());
    }
    Ok(parse_person(&resp))
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
