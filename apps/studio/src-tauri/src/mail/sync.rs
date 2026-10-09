//! Bringing a folder's cache up to date with as few requests as possible.
//!
//! A sync opens the folder (one EXAMINE, whose answer carries the message count, UIDNEXT and,
//! with CONDSTORE, HIGHESTMODSEQ) and asks for more only when that answer says something changed:
//! new UIDs are fetched from the last UIDNEXT, changed flags since the last mod-sequence, and
//! deletions are looked for only when the count dropped below what the additions explain. When
//! nothing changed a sync costs that one EXAMINE.

use std::collections::{HashMap, HashSet};

use async_imap::imap_proto::{MessageSection, SectionPath};

use super::body::{self, Body, Plan};
use super::imap::Conn;
use super::store::{DateRange, FolderState, Store, Summary};

/// Messages fetched at a time: the first look at a folder, and each page further back.
pub const PAGE: u32 = 50;

/// A message up to this size is fetched whole when opened; a larger one part by part.
const WHOLE_MESSAGE_LIMIT: u32 = 1_000_000;

/// Inline images fetched with a large message's text.
const INLINE_BUDGET: u64 = 8 * 1024 * 1024;

/// Whole messages fetched in one request when many are wanted at once, up to this many of them
/// and this many bytes, so each request ends well within the command timeout.
const BATCH_MESSAGES: usize = 100;
const BATCH_BYTES: u64 = 8 * 1024 * 1024;

/// Updates the cache of `path`. Returns whether anything in it changed.
pub async fn sync_folder(conn: &mut Conn, store: &Store, account: &str, path: &str) -> anyhow::Result<bool> {
    let prev = store.folder_state(account, path)?;
    let mbox = conn.examine(path).await?;
    let mut state = prev.clone();
    let mut changed = false;
    if prev.uidvalidity.is_some() && mbox.uid_validity.is_some() && prev.uidvalidity != mbox.uid_validity {
        store.clear_folder(account, path)?;
        state = FolderState::default();
        changed = true;
    }
    let mut modseq_moved = true;
    match state.low_uid {
        None => {
            if mbox.exists > 0 {
                let from = first_page_start(mbox.exists, PAGE);
                let list = conn.fetch_meta(&format!("{from}:{}", mbox.exists), false).await?;
                store.upsert(account, path, &list)?;
                state.low_uid = list.iter().map(|m| m.uid).min().or(mbox.uid_next);
                state.complete = from == 1;
            } else {
                state.low_uid = Some(mbox.uid_next.unwrap_or(1));
                state.complete = true;
            }
            changed = true;
        }
        Some(low) => {
            let next = match prev.uidnext {
                Some(n) => n,
                None => store.max_uid(account, path)?.map_or(low, |u| u + 1),
            };
            let mut added = 0;
            if mbox.uid_next.is_none_or(|n| n > next) {
                let list: Vec<_> = conn
                    .fetch_meta(&format!("{next}:*"), true)
                    .await?
                    .into_iter()
                    .filter(|m| m.uid >= next)
                    .collect();
                added = list.len() as u32;
                store.upsert(account, path, &list)?;
                changed |= added > 0;
            }
            let condstore = conn.caps.condstore && mbox.highest_modseq.is_some() && prev.modseq.is_some();
            if condstore {
                modseq_moved = mbox.highest_modseq != prev.modseq;
                if modseq_moved {
                    let changes = conn.fetch_flags(&format!("{low}:*"), prev.modseq).await?;
                    changed |= store.update_flags(account, path, &changes)?;
                }
                if removed_some(prev.exists, added, mbox.exists) {
                    let present: HashSet<u32> = conn.uid_search(&format!("UID {low}:*")).await?.into_iter().collect();
                    changed |= drop_gone(store, account, path, low, &present)?;
                }
            } else {
                // Without CONDSTORE the cached range's flags are read again; the same answer
                // shows which messages are gone.
                let current = conn.fetch_flags(&format!("{low}:*"), None).await?;
                changed |= store.update_flags(account, path, &current)?;
                let present: HashSet<u32> = current.iter().map(|c| c.uid).collect();
                changed |= drop_gone(store, account, path, low, &present)?;
            }
        }
    }
    if changed || modseq_moved || state.unseen.is_none() {
        state.unseen = conn.unseen(path).await.ok().flatten().or(state.unseen);
    }
    state.uidvalidity = mbox.uid_validity;
    state.uidnext = mbox.uid_next;
    state.modseq = mbox.highest_modseq;
    state.exists = Some(mbox.exists);
    state.checked_at = Some(crate::db::now_ms());
    store.save_folder_state(account, path, &state)?;
    Ok(changed)
}

fn drop_gone(store: &Store, account: &str, path: &str, low: u32, present: &HashSet<u32>) -> anyhow::Result<bool> {
    let gone = gone_uids(&store.uids_from(account, path, low)?, present);
    store.delete_uids(account, path, &gone)?;
    Ok(!gone.is_empty())
}

/// Fetches the page of messages just older than the cached ones. Returns how many arrived.
pub async fn load_older(conn: &mut Conn, store: &Store, account: &str, path: &str) -> anyhow::Result<u32> {
    // Fresh counts, and the folder open, for the sequence numbers below.
    sync_folder(conn, store, account, path).await?;
    let mut state = store.folder_state(account, path)?;
    let (Some(low), Some(exists)) = (state.low_uid, state.exists) else {
        return Ok(0);
    };
    if state.complete {
        return Ok(0);
    }
    let cached = store.count(account, path, low)?;
    let Some((from, to)) = older_range(exists, cached, PAGE) else {
        state.complete = true;
        store.save_folder_state(account, path, &state)?;
        return Ok(0);
    };
    let fetched = conn.fetch_meta(&format!("{from}:{to}"), false).await?;
    // `to` is the oldest cached message when the counts agree. If the page stops short of it
    // the counts were off, and the UIDs in between are fetched by UID so none is skipped.
    let reached = fetched.iter().any(|m| m.uid >= low);
    let mut older: Vec<_> = fetched.into_iter().filter(|m| m.uid < low).collect();
    if !reached && let Some(top) = older.iter().map(|m| m.uid).max() {
        let between = conn.uid_search(&format!("UID {}:{}", top + 1, low - 1)).await?;
        let between: Vec<u32> = between.into_iter().filter(|u| *u > top && *u < low).collect();
        if !between.is_empty() {
            older.extend(conn.fetch_meta(&uid_set(&between), true).await?);
        }
    }
    store.upsert(account, path, &older)?;
    if let Some(min) = older.iter().map(|m| m.uid).min() {
        state.low_uid = Some(min.min(low));
    }
    state.complete = from == 1;
    store.save_folder_state(account, path, &state)?;
    Ok(older.len() as u32)
}

/// Searches a folder on the server, for `text`, mail that arrived within `dates`, or both; the
/// newest `limit` results are fetched into the cache when they are not there yet.
pub async fn search(
    conn: &mut Conn,
    store: &Store,
    account: &str,
    path: &str,
    text: &str,
    dates: &DateRange,
    limit: usize,
) -> anyhow::Result<Vec<Summary>> {
    conn.ensure_selected(path).await?;
    let uids = conn.search_text(text, &date_criteria(dates)).await?;
    // The server matches whole days, in its own time zone, so it also finds mail from around the
    // range: the newest matches are read a page at a time until `limit` of them fall within it.
    let mut found = Vec::new();
    for page in uids.rchunks(limit.max(1)) {
        let cached: HashSet<u32> = store
            .summaries_by_uid(account, path, page)?
            .iter()
            .map(|s| s.uid)
            .collect();
        let missing: Vec<u32> = page.iter().copied().filter(|u| !cached.contains(u)).collect();
        if !missing.is_empty() {
            let list = conn.fetch_meta(&uid_set(&missing), true).await?;
            store.upsert(account, path, &list)?;
        }
        found.extend(
            store
                .summaries_by_uid(account, path, page)?
                .into_iter()
                .filter(|s| dates.contains(s.date)),
        );
        if found.len() >= limit {
            break;
        }
    }
    found.sort_by(|a, b| b.date.cmp(&a.date).then(b.uid.cmp(&a.uid)));
    found.truncate(limit);
    Ok(found)
}

/// IMAP search keys for mail that arrived within `dates`. IMAP compares whole days in the
/// server's time zone, so the days are widened by one on each side; `search` keeps the exact range.
fn date_criteria(dates: &DateRange) -> String {
    const DAY: i64 = 86_400_000;
    let day = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|d| d.format("%-d-%b-%Y").to_string())
            .unwrap_or_default()
    };
    let mut keys = Vec::new();
    if let Some(since) = dates.since {
        keys.push(format!("SINCE {}", day(since - DAY)));
    }
    if let Some(until) = dates.until {
        // BEFORE excludes its own day: the day after the widened end.
        keys.push(format!("BEFORE {}", day(until + 2 * DAY)));
    }
    keys.retain(|k| !k.ends_with(' '));
    keys.join(" ")
}

/// Whether a message's body is fetched by fetching the whole message; a large one with parts is
/// fetched part by part, leaving its attachments on the server.
fn fetched_whole(size: u32, multipart: bool) -> bool {
    !multipart || size <= WHOLE_MESSAGE_LIMIT
}

/// The requests that fetch the bodies of many messages of one folder.
#[derive(Debug, PartialEq, Eq)]
pub enum BodyBatch {
    /// Whole messages, all in one request.
    Whole(Vec<u32>),
    /// A large message with parts, which has a request of its own (see `fetch_body`).
    Parts(u32),
}

impl BodyBatch {
    /// The messages the request fetches.
    pub fn count(&self) -> usize {
        match self {
            BodyBatch::Whole(uids) => uids.len(),
            BodyBatch::Parts(_) => 1,
        }
    }
}

/// Groups messages (`(uid, size, multipart)`) into as few requests as their sizes allow.
pub fn body_batches(messages: impl IntoIterator<Item = (u32, u32, bool)>) -> Vec<BodyBatch> {
    let mut batches = Vec::new();
    let mut whole = Vec::new();
    let mut bytes = 0u64;
    for (uid, size, multipart) in messages {
        if !fetched_whole(size, multipart) {
            batches.push(BodyBatch::Parts(uid));
            continue;
        }
        if !whole.is_empty() && (whole.len() >= BATCH_MESSAGES || bytes + u64::from(size) > BATCH_BYTES) {
            batches.push(BodyBatch::Whole(std::mem::take(&mut whole)));
            bytes = 0;
        }
        whole.push(uid);
        bytes += u64::from(size);
    }
    if !whole.is_empty() {
        batches.push(BodyBatch::Whole(whole));
    }
    batches
}

/// The bodies of whole messages, fetched in one request, by UID. A message no longer on the
/// server is left out.
pub async fn fetch_whole(conn: &mut Conn, path: &str, uids: &[u32]) -> anyhow::Result<HashMap<u32, Body>> {
    conn.ensure_selected(path).await?;
    let wanted: HashSet<u32> = uids.iter().copied().collect();
    let fetches = conn.fetch_many(&uid_set(uids), "(UID BODY.PEEK[])").await?;
    // The server may slip in news about other messages, without a body; only the answers count.
    Ok(fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid.filter(|u| wanted.contains(u))?;
            Some((uid, body::parse_body(f.body()?)))
        })
        .collect())
}

/// A message's body: the whole message when small, otherwise only its text and inline images.
/// None when the message is no longer on the server.
pub async fn fetch_body(conn: &mut Conn, path: &str, uid: u32, size: u32, plan: &Plan) -> anyhow::Result<Option<Body>> {
    conn.ensure_selected(path).await?;
    if fetched_whole(size, plan.multipart) {
        let Some(f) = conn.fetch_one(uid, "(UID BODY.PEEK[])").await? else {
            return Ok(None);
        };
        return Ok(Some(body::parse_body(f.body().unwrap_or_default())));
    }
    let mut sections: Vec<&str> = plan
        .html
        .iter()
        .chain(plan.text.iter())
        .take(1)
        .map(String::as_str)
        .collect();
    let mut budget = INLINE_BUDGET;
    for p in &plan.inline {
        if p.size <= budget {
            budget -= p.size;
            sections.push(&p.section);
        }
    }
    if sections.is_empty() {
        return Ok(Some(Body::default()));
    }
    let items: Vec<String> = sections
        .iter()
        .map(|s| format!("BODY.PEEK[{s}.MIME] BODY.PEEK[{s}]"))
        .collect();
    let Some(f) = conn.fetch_one(uid, &format!("(UID {})", items.join(" "))).await? else {
        return Ok(None);
    };
    let parts: Vec<(&[u8], &[u8])> = sections
        .iter()
        .filter_map(|s| {
            let nums = body::section_path(s)?;
            let mime = f.section(&SectionPath::Part(nums.clone(), Some(MessageSection::Mime)))?;
            let content = f.section(&SectionPath::Part(nums, None))?;
            Some((mime, content))
        })
        .collect();
    Ok(Some(body::parse_body(&body::synthetic(&parts))))
}

/// An attachment's decoded content. None when the message is no longer on the server.
pub async fn fetch_attachment(
    conn: &mut Conn,
    path: &str,
    uid: u32,
    plan: &Plan,
    section: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(attachment) = plan.attachments.iter().find(|a| a.section == section) else {
        anyhow::bail!("This message has no such attachment.");
    };
    conn.ensure_selected(path).await?;
    if !plan.multipart {
        let Some(f) = conn.fetch_one(uid, "(UID BODY.PEEK[])").await? else {
            return Ok(None);
        };
        return Ok(Some(body::part_contents(f.body().unwrap_or_default(), b"")));
    }
    let Some(nums) = body::section_path(section) else {
        anyhow::bail!("This message has no such attachment.");
    };
    if attachment.mime == "message/rfc822" {
        let Some(f) = conn.fetch_one(uid, &format!("(UID BODY.PEEK[{section}])")).await? else {
            return Ok(None);
        };
        return Ok(f.section(&SectionPath::Part(nums, None)).map(<[u8]>::to_vec));
    }
    let items = format!("(UID BODY.PEEK[{section}.MIME] BODY.PEEK[{section}])");
    let Some(f) = conn.fetch_one(uid, &items).await? else {
        return Ok(None);
    };
    let mime = f.section(&SectionPath::Part(nums.clone(), Some(MessageSection::Mime)));
    let content = f.section(&SectionPath::Part(nums, None));
    Ok(match (mime, content) {
        (Some(mime), Some(content)) => Some(body::part_contents(mime, content)),
        (None, Some(content)) => Some(content.to_vec()),
        _ => None,
    })
}

/// The sequence number the first look at a folder starts from: its newest `page` messages.
fn first_page_start(exists: u32, page: u32) -> u32 {
    exists.saturating_sub(page.saturating_sub(1)).max(1)
}

/// Whether the message count fell below what the additions explain, so some were removed.
fn removed_some(before: Option<u32>, added: u32, now: u32) -> bool {
    before.is_none_or(|b| b + added > now)
}

/// The sequence range of the next older page: the `older` messages before the cached ones end
/// at sequence `exists - cached`; the range reaches one further, to the oldest cached message,
/// so the overlap shows the counts were right.
fn older_range(exists: u32, cached: u32, page: u32) -> Option<(u32, u32)> {
    let older = exists.checked_sub(cached).filter(|n| *n > 0)?;
    let from = older.saturating_sub(page.saturating_sub(1)).max(1);
    Some((from, (older + 1).min(exists)))
}

fn gone_uids(cached: &[u32], present: &HashSet<u32>) -> Vec<u32> {
    cached.iter().copied().filter(|u| !present.contains(u)).collect()
}

/// A compact UID set: `3:5,9,12:13`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        out.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        i += 1;
    }
    out.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_look_takes_the_newest_page() {
        assert_eq!(first_page_start(1200, 50), 1151);
        assert_eq!(first_page_start(30, 50), 1);
        assert_eq!(first_page_start(50, 50), 1);
        assert_eq!(first_page_start(51, 50), 2);
    }

    #[test]
    fn removals_are_noticed_from_the_counts() {
        assert!(!removed_some(Some(100), 0, 100));
        assert!(!removed_some(Some(100), 3, 103));
        assert!(removed_some(Some(100), 3, 102));
        assert!(removed_some(Some(100), 0, 99));
        assert!(removed_some(None, 0, 10));
    }

    #[test]
    fn older_pages_overlap_the_oldest_cached_message() {
        // 1,000 on the server, the newest 50 cached: the page is 901..950, plus 951 to check.
        assert_eq!(older_range(1000, 50, 50), Some((901, 951)));
        assert_eq!(older_range(60, 50, 50), Some((1, 11)));
        assert_eq!(older_range(50, 50, 50), None);
        assert_eq!(older_range(40, 50, 50), None);
    }

    #[test]
    fn uid_sets_are_compact() {
        assert_eq!(uid_set(&[9, 3, 4, 5, 12, 13, 4]), "3:5,9,12:13");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
    }

    #[test]
    fn dates_are_searched_by_whole_days_around_the_range() {
        let ms = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis();
        let range = DateRange {
            since: Some(ms("2026-10-01T09:30:00Z")),
            until: Some(ms("2026-10-09T23:59:59.999Z")),
        };
        assert_eq!(date_criteria(&range), "SINCE 30-Sep-2026 BEFORE 11-Oct-2026");
        let open_end = DateRange {
            since: Some(ms("2026-01-01T00:00:00Z")),
            until: None,
        };
        assert_eq!(date_criteria(&open_end), "SINCE 31-Dec-2025");
        assert_eq!(date_criteria(&DateRange::default()), "");
        assert!(range.contains(ms("2026-10-05T12:00:00Z")));
        assert!(!range.contains(ms("2026-10-10T00:00:00Z")));
    }

    #[test]
    fn bodies_are_fetched_many_to_a_request() {
        use BodyBatch::{Parts, Whole};
        // 250 small messages: three requests, of at most 100.
        let small: Vec<_> = (1..=250).map(|uid| (uid, 20_000, true)).collect();
        let batches = body_batches(small);
        assert_eq!(batches.iter().map(BodyBatch::count).collect::<Vec<_>>(), [100, 100, 50]);
        // Requests stop growing at 8 MB; a large message with parts is fetched on its own.
        let mixed = [
            (1, 900_000, false),
            (2, 5_000_000, true),
            (3, 3_000_000, false),
            (4, 900_000, true),
        ];
        let mut sizes = mixed.to_vec();
        sizes.extend((5..=10).map(|uid| (uid, 1_000_000, false)));
        assert_eq!(
            body_batches(sizes),
            [Parts(2), Whole(vec![1, 3, 4, 5, 6, 7]), Whole(vec![8, 9, 10])]
        );
        // A message larger than a request still gets one.
        assert_eq!(body_batches([(1, 20_000_000, false)]), [Whole(vec![1])]);
        assert!(body_batches([]).is_empty());
    }

    #[test]
    fn gone_uids_are_the_cached_ones_the_server_lacks() {
        let present: HashSet<u32> = [1, 3].into_iter().collect();
        assert_eq!(gone_uids(&[1, 2, 3, 4], &present), [2, 4]);
    }
}
