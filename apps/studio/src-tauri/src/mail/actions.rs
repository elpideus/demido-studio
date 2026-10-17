//! Changing mail: flags, moving and deleting messages, making, renaming and deleting folders,
//! sending, and saving drafts.
//!
//! Each change is made on the server first and then in the cache, so the cache never shows what
//! the server does not have; the folders it touched are announced with `mail://changed`.
//! Messages moved into a folder get new UIDs there, which only the server knows, so that folder
//! is marked to be asked again the next time it is shown. Many messages go to a request; a
//! request the server fails is tried again in smaller ones, and when those fail too, what was
//! done stays done and the report says where it stopped.
//!
//! What a move or a delete into the trash took away can be put back for a while: each one keeps
//! where its messages went ([`Trip`]), and [`MailService::undo`] finds them there and moves them
//! back.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::Duration;

use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};

use super::body::{Addr, Attachment};
use super::imap::Copied;
use super::send::{self, Draft, OutFile};
use super::store::{ANSWERED, FLAGGED, SEEN};
use super::{Account, Kind, MailService, Summary, sync, utf7, with_conn};

/// Messages changed in one request.
const BATCH: usize = 500;
/// A failed request of more messages is tried again in requests of this many: Gmail sometimes
/// fails a large one ("System Error") that goes through in parts.
const RETRY_BATCH: usize = 100;
/// The pause before those smaller requests.
const RETRY_PAUSE: Duration = Duration::from_secs(1);
/// The most a message carries of another's attachments (a forward's, a draft's).
const CARRIED_ATTACHMENTS: u64 = send::MAX_ATTACHMENTS;
/// How long after sending the Sent folder is checked for the server's own copy.
const SENT_SETTLE: Duration = Duration::from_secs(3);
/// The most recent actions that can still be undone.
const UNDOS: usize = 20;

/// What a change to many messages did.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    /// Messages changed.
    pub changed: usize,
    /// Messages that already were as asked: flagged already, or in the folder already.
    pub already: usize,
    /// Messages that left the cache since they were listed.
    pub missing: usize,
    /// Why it ended before every message was done, when it did.
    pub stopped: Option<String>,
    /// What [`MailService::undo`] takes to put moved messages back where they were.
    pub undo: Option<u64>,
}

/// What deleting messages did.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    /// Moved to the trash, from where they can be taken back.
    pub trashed: usize,
    /// Gone for good.
    pub destroyed: usize,
    pub missing: usize,
    pub stopped: Option<String>,
    /// What [`MailService::undo`] takes to bring the trashed messages back.
    pub undo: Option<u64>,
}

/// How to find a moved message again where it went.
#[derive(Clone, Debug, Default)]
struct Moved {
    /// Its UID there, when the server said.
    uid: Option<u32>,
    gmail_id: Option<u64>,
    message_id: Option<String>,
    /// Its Gmail labels, which the trash takes away.
    labels: Vec<String>,
    /// It was in the inbox too (on Gmail, where it is in a folder for each label).
    in_inbox: bool,
    /// It was there already (a Gmail label it had): it goes back with a copy, and stays.
    stays: bool,
}

/// Messages one action moved from a folder to another.
#[derive(Clone, Debug)]
pub(super) struct Trip {
    account: String,
    from: String,
    to: String,
    /// The UIDVALIDITY of `to` when the server gave the messages' UIDs there.
    validity: Option<u32>,
    messages: Vec<Moved>,
    /// They went where Gmail takes labels away (the trash, spam), so their labels go back on.
    relabel: bool,
}

/// What the latest actions moved, to be put back while they can be undone.
#[derive(Debug, Default)]
pub(super) struct Undos {
    last: u64,
    kept: VecDeque<(u64, Vec<Trip>)>,
}

impl Undos {
    /// Keeps what an action moved, the oldest giving way after [`UNDOS`]; returns its key.
    fn keep(&mut self, trips: Vec<Trip>) -> Option<u64> {
        let trips: Vec<Trip> = trips.into_iter().filter(|t| !t.messages.is_empty()).collect();
        if trips.is_empty() {
            return None;
        }
        self.last += 1;
        self.kept.push_back((self.last, trips));
        while self.kept.len() > UNDOS {
            self.kept.pop_front();
        }
        Some(self.last)
    }

    fn take(&mut self, key: u64) -> Option<Vec<Trip>> {
        let at = self.kept.iter().position(|(k, _)| *k == key)?;
        self.kept.remove(at).map(|(_, trips)| trips)
    }
}

/// A message to write: the person's text, and the message it answers or passes on.
#[derive(Clone, Debug, Default)]
pub struct Compose {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    /// Default: `Re:` or `Fwd:` and the original's subject.
    pub subject: Option<String>,
    pub text: String,
    pub attachments: Vec<OutFile>,
    pub reply_to: Option<i64>,
    /// A reply also goes to everyone the original went to.
    pub reply_all: bool,
    pub forward: Option<i64>,
    /// Written in the Mail window from a [`Prefill`]: the recipients, subject and text are sent
    /// as they are, with nothing added. The thread is `in_reply_to` and `references`, and
    /// another message's attachments come only as `carry` names them.
    pub as_written: bool,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Attachments of another message that go with this one: its id and their parts (a
    /// forward's, or those of the draft being finished).
    pub carry: Option<(i64, Vec<String>)>,
}

/// What the Mail window's editor starts from.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Start {
    Reply,
    ReplyAll,
    Forward,
    /// Going on with a draft.
    Draft,
}

/// A message for the person to finish in the Mail window: a reply, a forward or a draft, with
/// what it takes from the original. Sent back as a [`Compose`] with `as_written`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefill {
    pub account: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    /// Blank lines for the person's text, then the quote or the forwarded message; a draft's
    /// own text.
    pub text: String,
    /// The original's attachments that go with it (a forward's, a draft's), unless taken off.
    pub attachments: Vec<Attachment>,
    /// The message they come from.
    pub attachments_of: Option<i64>,
    pub reply_to: Option<i64>,
    pub forward: Option<i64>,
    /// The draft this goes on with, deleted once it is sent or saved again.
    pub draft: Option<i64>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

fn has(s: &Summary, bit: u8) -> bool {
    match bit {
        SEEN => !s.unread,
        FLAGGED => s.flagged,
        ANSWERED => s.answered,
        _ => false,
    }
}

fn flag_name(bit: u8) -> anyhow::Result<&'static str> {
    Ok(match bit {
        SEEN => r"\Seen",
        FLAGGED => r"\Flagged",
        ANSWERED => r"\Answered",
        _ => anyhow::bail!("unknown flag {bit}"),
    })
}

fn when(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%a, %-d %b %Y at %H:%M").to_string())
        .unwrap_or_default()
}

/// The roles of folders the mail system keeps, which are neither renamed nor deleted.
fn system_folder(role: Option<&str>, path: &str) -> bool {
    role.is_some() || path.eq_ignore_ascii_case("INBOX")
}

/// The account's own address, which its mail is sent from.
fn own_mailbox(account: &Account) -> anyhow::Result<Mailbox> {
    let email = account
        .email
        .parse()
        .map_err(|_| anyhow::anyhow!("{} is not an address mail can be sent from.", account.email))?;
    Ok(Mailbox::new(None, email))
}

/// The thread a reply to `s` joins: the message it answers, and the references so far.
fn thread(s: &Summary, original: &send::Original) -> (Option<String>, Vec<String>) {
    let parent = original.message_id.clone().or_else(|| {
        s.message_id
            .as_ref()
            .map(|m| format!("<{}>", m.trim_matches(['<', '>'])))
    });
    let mut references = original.references.clone();
    if let Some(parent) = &parent
        && !references.contains(parent)
    {
        references.push(parent.clone());
    }
    (parent, references)
}

/// Who a reply to `s` goes to: the original's Reply-To, else its sender, or the people it went
/// to when `me` sent it. With `all`, who it copies too: everyone else the original went to.
/// Never `me`.
fn reply_people(s: &Summary, original: &send::Original, all: bool, me: &Mailbox) -> (Vec<Mailbox>, Vec<Mailbox>) {
    let mine = |m: &Mailbox| send::same_address(m, me);
    let answer = if original.reply_to.is_empty() {
        std::slice::from_ref(&s.from)
    } else {
        &original.reply_to[..]
    };
    let mut to: Vec<Mailbox> = answer.iter().filter_map(send::mailbox).filter(|m| !mine(m)).collect();
    let sent_by_me = to.is_empty();
    if sent_by_me {
        to = s.to.iter().filter_map(send::mailbox).filter(|m| !mine(m)).collect();
    }
    let mut cc: Vec<Mailbox> = Vec::new();
    if all {
        let rest: Vec<&Addr> = if sent_by_me {
            s.cc.iter().collect()
        } else {
            s.to.iter().chain(&s.cc).collect()
        };
        for m in rest.into_iter().filter_map(send::mailbox) {
            if !mine(&m) && !to.iter().chain(&cc).any(|o| send::same_address(o, &m)) {
                cc.push(m);
            }
        }
    }
    (to, cc)
}

/// The original under a forward: its header fields and text.
fn forwarded_text(s: &Summary, text: &str) -> String {
    let to: Vec<String> = s.to.iter().map(Addr::display).collect();
    send::forwarded(&s.from.display(), &when(s.date), &s.subject, &to.join(", "), text)
}

/// Addresses from a message, as the editor shows them.
fn written(list: &[Addr]) -> Vec<String> {
    list.iter()
        .map(|a| {
            send::mailbox(a)
                .map(|m| send::written(&m))
                .unwrap_or_else(|| a.display())
        })
        .collect()
}

/// Cached messages by account and folder.
type Groups = BTreeMap<(String, String), Vec<Summary>>;

/// Gmail takes a message's labels away when it goes to these.
fn takes_labels(role: Option<&str>) -> bool {
    matches!(role, Some("trash" | "junk"))
}

/// `items` in requests of [`BATCH`].
fn batches<T>(items: &[T]) -> VecDeque<&[T]> {
    items.chunks(BATCH).collect()
}

/// After `failed` failed: whether to go on with it in smaller requests, which are put first in
/// `queue`. A request already that small is not tried again.
async fn split_after_failure<'a, T>(queue: &mut VecDeque<&'a [T]>, failed: &'a [T]) -> bool {
    if failed.len() <= RETRY_BATCH {
        return false;
    }
    for part in failed.chunks(RETRY_BATCH).rev() {
        queue.push_front(part);
    }
    tokio::time::sleep(RETRY_PAUSE).await;
    true
}

impl MailService {
    /// The cached messages of `ids`, by account and folder, and how many are not cached.
    fn by_folder(&self, ids: &[i64]) -> anyhow::Result<(Groups, usize)> {
        let mut groups = Groups::new();
        let mut missing = 0;
        let mut seen = HashSet::new();
        for id in ids.iter().filter(|id| seen.insert(**id)) {
            match self.store.message(*id)? {
                Some(s) => groups.entry((s.account.clone(), s.folder.clone())).or_default().push(s),
                None => missing += 1,
            }
        }
        Ok((groups, missing))
    }

    fn role_of(&self, account: &str, path: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .store
            .folders(account)?
            .into_iter()
            .find(|f| f.path == path)
            .and_then(|f| f.role))
    }

    /// How to find `s` again once it has moved: as the server numbered it there (`copied`), or by
    /// Gmail's id for it and its Message-ID. `to` is where it went; a Gmail message that had
    /// that label already stays there when it is put back.
    fn moved(&self, s: &Summary, gmail: bool, to: &str, copied: Option<&Copied>) -> anyhow::Result<Moved> {
        let gmail_id = if gmail {
            self.store.gmail_ids(&[s.id])?.first().copied()
        } else {
            None
        };
        let in_inbox = match (gmail_id, self.store.find_folder(&s.account, "inbox")?) {
            (Some(id), Some(inbox)) => self.store.gmail_folders(&s.account, id)?.contains(&inbox),
            _ => false,
        };
        Ok(Moved {
            uid: copied.and_then(|c| c.uids.get(&s.uid).copied()),
            gmail_id,
            message_id: s.message_id.clone(),
            labels: s.labels.clone(),
            in_inbox,
            stays: gmail && s.labels.contains(&utf7::decode(to)),
        })
    }

    /// The account's trash folder, when it has one.
    pub fn trash(&self, account: &Account) -> anyhow::Result<Option<String>> {
        self.store.find_folder(&account.id, "trash")
    }

    /// Sets (`on`) or clears a flag ([`SEEN`], [`FLAGGED`], [`ANSWERED`]) on messages, on the
    /// server and in the cache, with their copies in other folders. Fails only when nothing
    /// could be changed.
    pub async fn set_flag(&self, ids: &[i64], bit: u8, on: bool) -> anyhow::Result<Changed> {
        let flag = flag_name(bit)?;
        let (groups, missing) = self.by_folder(ids)?;
        let mut report = Changed {
            missing,
            ..Changed::default()
        };
        let mut changed: HashSet<(String, String)> = HashSet::new();
        let mut stored = false;
        'folders: for ((account_id, folder), list) in groups {
            let Some(account) = self.accounts.get(&account_id) else {
                report.missing += list.len();
                continue;
            };
            report.already += list.iter().filter(|s| has(s, bit) == on).count();
            let uids: Vec<u32> = list.iter().map(|s| s.uid).collect();
            let mut queue = batches(&uids);
            while let Some(chunk) = queue.pop_front() {
                let set = sync::uid_set(chunk);
                let result = with_conn!(self, &account, |conn| async {
                    anyhow::Ok(conn.store_flag(&folder, &set, flag, on).await?)
                });
                if let Err(e) = result {
                    if split_after_failure(&mut queue, chunk).await {
                        continue;
                    }
                    if !stored {
                        return Err(self.unless_gone(&account, &folder, e).await);
                    }
                    report.stopped = Some(format!("{e:#}"));
                    break 'folders;
                }
                stored = true;
                for (path, n) in self.store.set_flag(&account_id, &folder, chunk, bit, on)? {
                    if path == folder {
                        report.changed += n as usize;
                    }
                    changed.insert((account_id.clone(), path));
                }
            }
            // Gmail shows starred mail in a folder of its own, which only the server can number.
            if bit == FLAGGED
                && account.kind == Kind::Gmail
                && let Some(starred) = self.store.find_folder(&account_id, "flagged")?
            {
                self.store.mark_stale(&account_id, &starred)?;
                changed.insert((account_id.clone(), starred));
            }
        }
        for (account, folder) in changed {
            self.emit_changed(&account, &folder);
        }
        Ok(report)
    }

    /// Marks messages as read; see [`MailService::set_flag`].
    pub async fn mark_read(&self, ids: &[i64]) -> anyhow::Result<Changed> {
        self.set_flag(ids, SEEN, true).await
    }

    /// The folder `folder` names: the one at that exact path when there is one (the Mail window
    /// names folders so), else as [`MailService::folder_path`] finds it, by name or role.
    fn resolve_folder(&self, account: &Account, folder: &str) -> anyhow::Result<String> {
        if self.store.folders(&account.id)?.iter().any(|f| f.path == folder) {
            return Ok(folder.to_string());
        }
        self.folder_path(account, Some(folder))
    }

    /// The folder `name` names for messages to go to: as [`MailService::resolve_folder`], with
    /// "archive" meaning All Mail where there is no archive folder (Gmail archives by taking
    /// mail out of the inbox).
    pub fn destination(&self, account: &Account, name: &str) -> anyhow::Result<String> {
        match self.resolve_folder(account, name) {
            Ok(path) => Ok(path),
            Err(e) => {
                if name.trim().eq_ignore_ascii_case("archive")
                    && let Some(all) = self.store.find_folder(&account.id, "all")?
                {
                    return Ok(all);
                }
                Err(e)
            }
        }
    }

    /// Moves messages to the folder `dest` names in their own account, or with `copy` copies
    /// them there (on Gmail: gives them that label too).
    pub async fn move_to(&self, ids: &[i64], dest: &str, copy: bool) -> anyhow::Result<Changed> {
        let (groups, missing) = self.by_folder(ids)?;
        let mut report = Changed {
            missing,
            ..Changed::default()
        };
        let mut changed: HashSet<(String, String)> = HashSet::new();
        let mut undo: Vec<Trip> = Vec::new();
        let mut done = false;
        'folders: for ((account_id, folder), list) in groups {
            let Some(account) = self.accounts.get(&account_id) else {
                report.missing += list.len();
                continue;
            };
            let to = self.destination(&account, dest)?;
            if to == folder {
                report.already += list.len();
                continue;
            }
            let gmail = account.kind == Kind::Gmail;
            // On Gmail, All Mail holds every message: moving out of it only adds a label.
            let stays = copy || (gmail && self.role_of(&account_id, &folder)?.as_deref() == Some("all"));
            let mut trip = Trip {
                account: account_id.clone(),
                from: folder.clone(),
                to: to.clone(),
                validity: None,
                messages: Vec::new(),
                relabel: gmail && takes_labels(self.role_of(&account_id, &to)?.as_deref()),
            };
            let by_uid: HashMap<u32, &Summary> = list.iter().map(|s| (s.uid, s)).collect();
            let uids: Vec<u32> = list.iter().map(|s| s.uid).collect();
            let mut queue = batches(&uids);
            while let Some(chunk) = queue.pop_front() {
                let set = sync::uid_set(chunk);
                let result = with_conn!(self, &account, |conn| async {
                    if copy {
                        conn.copy_messages(&folder, &set, &to).await?;
                        anyhow::Ok(None)
                    } else {
                        anyhow::Ok(conn.move_messages(&folder, &set, &to).await?)
                    }
                });
                let copied = match result {
                    Ok(copied) => copied,
                    Err(e) => {
                        if split_after_failure(&mut queue, chunk).await {
                            continue;
                        }
                        if !done {
                            return Err(self.unless_gone(&account, &folder, e).await);
                        }
                        report.stopped = Some(format!("{e:#}"));
                        undo.push(trip);
                        break 'folders;
                    }
                };
                done = true;
                report.changed += chunk.len();
                // A copy takes nothing away: there is nothing to undo.
                if !copy {
                    trip.validity = copied.as_ref().map(|c| c.validity).or(trip.validity);
                    for uid in chunk {
                        if let Some(s) = by_uid.get(uid) {
                            trip.messages.push(self.moved(s, gmail, &to, copied.as_ref())?);
                        }
                    }
                }
                if !stays {
                    for path in self.store.remove_messages(&account_id, &folder, chunk, false)? {
                        changed.insert((account_id.clone(), path));
                    }
                }
            }
            undo.push(trip);
            self.store.mark_stale(&account_id, &to)?;
            changed.insert((account_id.clone(), to));
        }
        for (account, folder) in changed {
            self.emit_changed(&account, &folder);
        }
        report.undo = self.undos.lock().keep(undo);
        Ok(report)
    }

    /// Deletes messages: into the trash, or with `permanent` (or already in the trash, or with
    /// no trash folder) for good.
    pub async fn delete(&self, ids: &[i64], permanent: bool) -> anyhow::Result<Deleted> {
        let (groups, missing) = self.by_folder(ids)?;
        let mut report = Deleted {
            missing,
            ..Deleted::default()
        };
        let mut changed: HashSet<(String, String)> = HashSet::new();
        let mut undo: Vec<Trip> = Vec::new();
        let mut done = false;
        'folders: for ((account_id, folder), list) in groups {
            let Some(account) = self.accounts.get(&account_id) else {
                report.missing += list.len();
                continue;
            };
            let gmail = account.kind == Kind::Gmail;
            let trash = self.trash(&account)?;
            let in_trash = trash.as_deref() == Some(folder.as_str());
            let mut trip = Trip {
                account: account_id.clone(),
                from: folder.clone(),
                to: trash.clone().unwrap_or_default(),
                validity: None,
                messages: Vec::new(),
                relabel: gmail,
            };
            let mut queue = batches(&list);
            while let Some(chunk) = queue.pop_front() {
                let uids: Vec<u32> = chunk.iter().map(|s| s.uid).collect();
                let set = sync::uid_set(&uids);
                let gm_ids = if gmail {
                    self.store.gmail_ids(&chunk.iter().map(|s| s.id).collect::<Vec<_>>())?
                } else {
                    Vec::new()
                };
                let to_trash = !in_trash && trash.is_some() && (!permanent || gmail);
                let result = with_conn!(self, &account, |conn| async {
                    match (&trash, to_trash) {
                        (Some(trash), true) => {
                            let copied = conn.move_messages(&folder, &set, trash).await?;
                            // Gmail deletes for good only from the trash: there, every label goes.
                            if permanent && !gm_ids.is_empty() {
                                let found = conn.find_gmail_ids(trash, &gm_ids).await?;
                                if !found.is_empty() {
                                    conn.expunge_messages(trash, &sync::uid_set(&found)).await?;
                                }
                            }
                            anyhow::Ok(copied)
                        }
                        _ => {
                            conn.expunge_messages(&folder, &set).await?;
                            anyhow::Ok(None)
                        }
                    }
                });
                let copied = match result {
                    Ok(copied) => copied,
                    Err(e) => {
                        if split_after_failure(&mut queue, chunk).await {
                            continue;
                        }
                        if !done {
                            return Err(self.unless_gone(&account, &folder, e).await);
                        }
                        report.stopped = Some(format!("{e:#}"));
                        undo.push(trip);
                        break 'folders;
                    }
                };
                done = true;
                if !to_trash || (permanent && !gm_ids.is_empty()) {
                    report.destroyed += chunk.len();
                } else {
                    report.trashed += chunk.len();
                    trip.validity = copied.as_ref().map(|c| c.validity).or(trip.validity);
                    for s in chunk {
                        trip.messages.push(self.moved(s, gmail, &trip.to, copied.as_ref())?);
                    }
                }
                // A Gmail message in the trash has left every label.
                for path in self.store.remove_messages(&account_id, &folder, &uids, gmail)? {
                    changed.insert((account_id.clone(), path));
                }
                if let (Some(trash), true) = (&trash, to_trash) {
                    self.store.mark_stale(&account_id, trash)?;
                    changed.insert((account_id.clone(), trash.clone()));
                }
            }
            undo.push(trip);
        }
        for (account, folder) in changed {
            self.emit_changed(&account, &folder);
        }
        report.undo = self.undos.lock().keep(undo);
        Ok(report)
    }

    /// Whether deleting `ids` (with `permanent` or not) would delete any of them for good.
    pub fn deletes_for_good(&self, ids: &[i64], permanent: bool) -> anyhow::Result<bool> {
        if permanent {
            return Ok(true);
        }
        let (groups, _) = self.by_folder(ids)?;
        for (account_id, folder) in groups.keys() {
            let Some(account) = self.accounts.get(account_id) else {
                continue;
            };
            match self.trash(&account)? {
                Some(trash) if trash != *folder => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    }

    /// Puts back what a move or a delete into the trash took away ([`Changed::undo`],
    /// [`Deleted::undo`]): the messages are found where they went and moved back. When none
    /// could be, it can be tried again.
    pub async fn undo(&self, key: u64) -> anyhow::Result<Changed> {
        let trips = self
            .undos
            .lock()
            .take(key)
            .ok_or_else(|| anyhow::anyhow!("It is too late to undo that."))?;
        let result = self.put_back(&trips).await;
        if result.is_err() {
            self.undos.lock().kept.push_back((key, trips));
        }
        result
    }

    /// The UIDs of the messages of `trip` where they went: those the server gave, if they are
    /// still there, else found by Gmail's id or Message-ID. Those to move back come first, then
    /// those to copy back ([`Moved::stays`]).
    async fn find_moved(&self, account: &Account, trip: &Trip) -> anyhow::Result<(Vec<u32>, Vec<u32>)> {
        with_conn!(self, account, |conn| async {
            let mailbox = conn.examine(&trip.to).await?;
            let known = trip.validity.is_some() && mailbox.uid_validity == trip.validity;
            let given: Vec<u32> = trip.messages.iter().filter_map(|m| m.uid).collect();
            let there: HashSet<u32> = if known && !given.is_empty() {
                let set = sync::uid_set(&given);
                conn.uid_search(&format!("UID {set}")).await?.into_iter().collect()
            } else {
                HashSet::new()
            };
            let (mut go, mut stay) = (Vec::new(), Vec::new());
            for m in &trip.messages {
                let uid = match m.uid.filter(|_| known) {
                    Some(uid) => there.contains(&uid).then_some(uid),
                    None => {
                        let found = match (m.gmail_id, &m.message_id) {
                            (Some(id), _) => conn.find_gmail_ids(&trip.to, &[id]).await?,
                            (None, Some(id)) => conn.find_message_id(&trip.to, id).await?,
                            (None, None) => Vec::new(),
                        };
                        // Of copies with the same Message-ID, the newest is the one that came.
                        found.into_iter().max()
                    }
                };
                if let Some(uid) = uid {
                    if m.stays { &mut stay } else { &mut go }.push(uid);
                }
            }
            for list in [&mut go, &mut stay] {
                list.sort_unstable();
                list.dedup();
            }
            anyhow::Ok((go, stay))
        })
    }

    /// Moves the messages of `trips` back where they came from. Fails only when nothing could be
    /// put back.
    async fn put_back(&self, trips: &[Trip]) -> anyhow::Result<Changed> {
        let mut report = Changed::default();
        let mut changed: HashSet<(String, String)> = HashSet::new();
        let mut back: Vec<(Account, String)> = Vec::new();
        'trips: for trip in trips {
            let Some(account) = self.accounts.get(&trip.account) else {
                report.missing += trip.messages.len();
                continue;
            };
            let gmail = account.kind == Kind::Gmail;
            let (go, stay) = match self.find_moved(&account, trip).await {
                Ok(found) => found,
                Err(e) if report.changed == 0 => return Err(self.unless_gone(&account, &trip.to, e).await),
                Err(e) => {
                    report.stopped = Some(format!("{e:#}"));
                    break;
                }
            };
            report.missing += trip.messages.len().saturating_sub(go.len() + stay.len());
            // On Gmail, All Mail keeps what moves out of it: only a label is added.
            let leaves = !(gmail && self.role_of(&account.id, &trip.to)?.as_deref() == Some("all"));
            for (uids, copy) in [(&go, false), (&stay, true)] {
                let mut queue = batches(uids);
                while let Some(chunk) = queue.pop_front() {
                    let set = sync::uid_set(chunk);
                    let result = with_conn!(self, &account, |conn| async {
                        if copy {
                            conn.copy_messages(&trip.to, &set, &trip.from).await?;
                        } else {
                            conn.move_messages(&trip.to, &set, &trip.from).await?;
                        }
                        anyhow::Ok(())
                    });
                    if let Err(e) = result {
                        if split_after_failure(&mut queue, chunk).await {
                            continue;
                        }
                        if report.changed == 0 {
                            return Err(self.unless_gone(&account, &trip.to, e).await);
                        }
                        report.stopped = Some(format!("{e:#}"));
                        break 'trips;
                    }
                    report.changed += chunk.len();
                    if !copy && leaves {
                        for path in self.store.remove_messages(&account.id, &trip.to, chunk, false)? {
                            changed.insert((account.id.clone(), path));
                        }
                    }
                }
            }
            if gmail && trip.relabel {
                for path in self.relabel(&account, trip).await {
                    self.store.mark_stale(&account.id, &path)?;
                    changed.insert((account.id.clone(), path));
                }
            }
            self.store.mark_stale(&account.id, &trip.from)?;
            changed.insert((account.id.clone(), trip.to.clone()));
            changed.insert((account.id.clone(), trip.from.clone()));
            if !back.iter().any(|(a, f)| a.id == account.id && *f == trip.from) {
                back.push((account, trip.from.clone()));
            }
        }
        for (account, folder) in changed {
            self.emit_changed(&account, &folder);
        }
        // They show where they are back at once, numbered as the server numbers them now.
        for (account, folder) in back {
            if let Err(e) = self.sync(&account, &folder, true).await {
                tracing::debug!("syncing {folder} after an undo failed: {e:#}");
            }
        }
        Ok(report)
    }

    /// Gives the messages of `trip`, back where they came from, the Gmail labels they had
    /// before the trash or spam took them, and puts those that were in the inbox back there too.
    /// Returns the folders of the labels given; one that fails is left.
    async fn relabel(&self, account: &Account, trip: &Trip) -> Vec<String> {
        let mut by_labels: BTreeMap<&[String], Vec<u64>> = BTreeMap::new();
        for m in &trip.messages {
            if let Some(id) = m.gmail_id
                && !m.labels.is_empty()
            {
                by_labels.entry(&m.labels).or_default().push(id);
            }
        }
        let mut given = Vec::new();
        for (labels, ids) in by_labels {
            let result = with_conn!(self, account, |conn| async {
                let uids = conn.find_gmail_ids(&trip.from, &ids).await?;
                if !uids.is_empty() {
                    conn.add_gmail_labels(&trip.from, &sync::uid_set(&uids), labels).await?;
                }
                anyhow::Ok(())
            });
            match result {
                Ok(()) => given.extend(labels.iter().map(|l| utf7::encode(l))),
                Err(e) => tracing::debug!("labelling messages put back in {} failed: {e:#}", trip.from),
            }
        }
        let inbox = self.store.find_folder(&account.id, "inbox").ok().flatten();
        if let Some(inbox) = inbox.filter(|i| *i != trip.from) {
            let ids: Vec<u64> = trip
                .messages
                .iter()
                .filter(|m| m.in_inbox)
                .filter_map(|m| m.gmail_id)
                .collect();
            if !ids.is_empty() {
                // A copy into Gmail's inbox gives the message the inbox's label.
                let result = with_conn!(self, account, |conn| async {
                    let uids = conn.find_gmail_ids(&trip.from, &ids).await?;
                    if !uids.is_empty() {
                        conn.copy_messages(&trip.from, &sync::uid_set(&uids), &inbox).await?;
                    }
                    anyhow::Ok(())
                });
                match result {
                    Ok(()) => given.push(inbox),
                    Err(e) => tracing::debug!("putting messages back in the inbox failed: {e:#}"),
                }
            }
        }
        given.sort();
        given.dedup();
        given
    }

    fn delimiter(&self, account: &Account) -> anyhow::Result<String> {
        Ok(self.store.delimiter(&account.id)?.unwrap_or_else(|| "/".into()))
    }

    /// `name` as the server spells it: `/` between a folder and its subfolders becomes the
    /// server's separator, and each part is encoded.
    fn server_name(name: &str, delimiter: &str) -> anyhow::Result<String> {
        let parts: Vec<&str> = name.split('/').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) {
            anyhow::bail!("A folder name cannot be empty, or start or end with /.");
        }
        Ok(parts.into_iter().map(utf7::encode).collect::<Vec<_>>().join(delimiter))
    }

    /// Makes a folder (a label on Gmail), inside `parent` when given. Returns its path on the
    /// server, which [`folder_name`](super::folder_name) shows.
    pub async fn create_folder(&self, account: &Account, name: &str, parent: Option<&str>) -> anyhow::Result<String> {
        self.folders(account, false).await?;
        let delimiter = self.delimiter(account)?;
        let encoded = Self::server_name(name, &delimiter)?;
        let path = match parent.map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => format!("{}{delimiter}{encoded}", self.resolve_folder(account, p)?),
            None => encoded.clone(),
        };
        let known = self.store.folders(&account.id)?;
        if known.iter().any(|f| f.path.eq_ignore_ascii_case(&path)) {
            anyhow::bail!("{} already has a folder {}.", account.email, utf7::decode(&path));
        }
        let made = with_conn!(self, account, |conn| async {
            anyhow::Ok(conn.create_folder(&path).await?)
        });
        let path = match made {
            Ok(()) => path,
            // Some servers keep every folder inside the inbox.
            Err(e) if parent.is_none() && !path.to_uppercase().starts_with("INBOX") => {
                let inside = format!("INBOX{delimiter}{encoded}");
                let retried = with_conn!(self, account, |conn| async {
                    anyhow::Ok(conn.create_folder(&inside).await?)
                });
                match retried {
                    Ok(()) => inside,
                    Err(_) => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };
        self.list_folders(account).await?;
        Ok(path)
    }

    /// The folder `folder` names, unless it is one the mail system keeps (the inbox, Sent,
    /// Trash…), which cannot be renamed or deleted.
    fn own_folder(&self, account: &Account, folder: &str) -> anyhow::Result<String> {
        let path = self.resolve_folder(account, folder)?;
        let role = self.role_of(&account.id, &path)?;
        if system_folder(role.as_deref(), &path) {
            anyhow::bail!(
                "{} is one of the folders the mail system keeps ({}); it cannot be renamed or deleted.",
                utf7::decode(&path),
                role.as_deref().unwrap_or("inbox")
            );
        }
        Ok(path)
    }

    /// Renames a folder. A new name without `/` keeps it where it is; with `/`, it is the
    /// whole path from the top. Returns its new path on the server.
    pub async fn rename_folder(&self, account: &Account, folder: &str, new_name: &str) -> anyhow::Result<String> {
        self.folders(account, false).await?;
        let from = self.own_folder(account, folder)?;
        let delimiter = self.delimiter(account)?;
        let encoded = Self::server_name(new_name, &delimiter)?;
        let to = match from.rsplit_once(delimiter.as_str()) {
            Some((parent, _)) if !new_name.contains('/') => format!("{parent}{delimiter}{encoded}"),
            _ => encoded,
        };
        if to == from {
            return Ok(to);
        }
        with_conn!(self, account, |conn| async {
            anyhow::Ok(conn.rename_folder(&from, &to).await?)
        })?;
        self.list_folders(account).await?;
        Ok(to)
    }

    /// Deletes a folder and, on most servers, the messages in it (on Gmail, the label only).
    pub async fn delete_folder(&self, account: &Account, folder: &str) -> anyhow::Result<String> {
        self.folders(account, false).await?;
        let path = self.own_folder(account, folder)?;
        with_conn!(self, account, |conn| async {
            anyhow::Ok(conn.delete_folder(&path).await?)
        })?;
        self.list_folders(account).await?;
        Ok(utf7::decode(&path))
    }

    /// The UIDs on the server of a folder's messages that match `query`.
    async fn search_uids(&self, account: &Account, path: &str, query: &str) -> anyhow::Result<Vec<u32>> {
        let found = with_conn!(self, account, |conn| async {
            conn.ensure_selected(path).await?;
            anyhow::Ok(conn.uid_search(query).await?)
        });
        match found {
            Ok(uids) => Ok(uids),
            Err(e) => Err(self.unless_gone(account, path, e).await),
        }
    }

    /// Deletes every message in the trash or the spam folder for good, cached or not. Returns
    /// how many.
    pub async fn empty_folder(&self, account: &Account, folder: &str) -> anyhow::Result<usize> {
        let path = self.resolve_folder(account, folder)?;
        if !matches!(self.role_of(&account.id, &path)?.as_deref(), Some("trash" | "junk")) {
            anyhow::bail!(
                "Only the trash and the spam folder can be emptied; {} is neither.",
                utf7::decode(&path)
            );
        }
        let uids = self.search_uids(account, &path, "ALL").await?;
        let mut done = 0;
        let mut stopped = None;
        let mut queue = batches(&uids);
        while let Some(chunk) = queue.pop_front() {
            let set = sync::uid_set(chunk);
            let result = with_conn!(self, account, |conn| async {
                anyhow::Ok(conn.expunge_messages(&path, &set).await?)
            });
            if let Err(e) = result {
                if split_after_failure(&mut queue, chunk).await {
                    continue;
                }
                stopped = Some(e);
                break;
            }
            done += chunk.len();
            self.store.remove_messages(&account.id, &path, chunk, false)?;
        }
        self.store.mark_stale(&account.id, &path)?;
        self.emit_changed(&account.id, &path);
        match stopped {
            Some(e) if done == 0 => Err(e),
            Some(e) => Err(e.context(format!("{done} were deleted, then it stopped"))),
            None => Ok(done),
        }
    }

    /// Marks every message of a folder read, cached or not. Returns how many were unread.
    pub async fn mark_folder_read(&self, account: &Account, folder: &str) -> anyhow::Result<usize> {
        let path = self.resolve_folder(account, folder)?;
        let uids = self.search_uids(account, &path, "UNSEEN").await?;
        let mut done = 0;
        let mut stopped = None;
        let mut changed: HashSet<String> = HashSet::new();
        let mut queue = batches(&uids);
        while let Some(chunk) = queue.pop_front() {
            let set = sync::uid_set(chunk);
            let result = with_conn!(self, account, |conn| async {
                anyhow::Ok(conn.store_flag(&path, &set, r"\Seen", true).await?)
            });
            if let Err(e) = result {
                if split_after_failure(&mut queue, chunk).await {
                    continue;
                }
                stopped = Some(e);
                break;
            }
            done += chunk.len();
            changed.extend(
                self.store
                    .set_flag(&account.id, &path, chunk, SEEN, true)?
                    .into_iter()
                    .map(|(p, _)| p),
            );
        }
        for folder in &changed {
            self.emit_changed(&account.id, folder);
        }
        // The unread count also counts mail older than the cache: the server has the new one.
        if done > 0
            && let Err(e) = self.sync(account, &path, true).await
        {
            tracing::debug!("syncing {path} after marking it read failed: {e:#}");
        }
        match stopped {
            Some(e) if done == 0 => Err(e),
            Some(e) => Err(e.context(format!("{done} were marked read, then it stopped"))),
            None => Ok(done),
        }
    }

    /// A message's header, read from the server, for what a reply takes from it.
    async fn original_of(&self, s: &Summary) -> anyhow::Result<send::Original> {
        let account = self.account_of(s)?;
        let header = with_conn!(self, &account, |conn| async {
            anyhow::Ok(conn.fetch_header(&s.folder, s.uid).await?)
        });
        Ok(header.ok().flatten().map(|h| send::original(&h)).unwrap_or_default())
    }

    /// Adds attachments of message `id` to the draft: those of `parts`, or all of them. Those
    /// that would take it past 25 MB together, or cannot be read, are left out, as the draft
    /// says.
    async fn carry(&self, draft: &mut Draft, id: i64, parts: Option<&[String]>) -> anyhow::Result<()> {
        let s = self.summary(id)?;
        let mut total: u64 = draft.attachments.iter().map(|f| f.bytes.len() as u64).sum();
        let mut left = Vec::new();
        for a in &s.plan.attachments {
            if parts.is_some_and(|p| !p.contains(&a.section)) {
                continue;
            }
            if total + a.size > CARRIED_ATTACHMENTS {
                left.push(a.name.clone());
                continue;
            }
            match self.attachment(id, &a.section).await {
                Ok((name, bytes)) => {
                    total += bytes.len() as u64;
                    draft.attachments.push(OutFile {
                        name,
                        mime: a.mime.clone(),
                        bytes,
                    });
                }
                Err(e) => left.push(format!("{} ({e:#})", a.name)),
            }
        }
        if !left.is_empty() {
            draft.left_out = Some(format!(
                "Attachments left out (over 25 MB together, or unreadable): {}.",
                left.join(", ")
            ));
        }
        Ok(())
    }

    /// What the Mail window's editor starts from to answer, forward or go on with message `id`.
    pub async fn prefill(&self, id: i64, start: Start) -> anyhow::Result<Prefill> {
        let s = self.summary(id)?;
        let account = self.account_of(&s)?;
        let opened = self.open(id).await?;
        let mut p = Prefill {
            account: account.id.clone(),
            ..Prefill::default()
        };
        match start {
            Start::Reply | Start::ReplyAll => {
                let original = self.original_of(&s).await?;
                let (to, cc) = reply_people(&s, &original, start == Start::ReplyAll, &own_mailbox(&account)?);
                p.to = to.iter().map(send::written).collect();
                p.cc = cc.iter().map(send::written).collect();
                p.subject = send::reply_subject(&s.subject);
                p.text = format!("\n\n{}", send::quote(&when(s.date), &s.from.display(), &opened.text));
                (p.in_reply_to, p.references) = thread(&s, &original);
                p.reply_to = Some(id);
            }
            Start::Forward => {
                p.subject = send::forward_subject(&s.subject);
                p.text = format!("\n\n{}", forwarded_text(&s, &opened.text));
                p.attachments = s.plan.attachments.clone();
                p.attachments_of = Some(id);
                p.forward = Some(id);
            }
            Start::Draft => {
                let original = self.original_of(&s).await?;
                p.to = written(&s.to);
                p.cc = written(&s.cc);
                p.bcc = written(&original.bcc);
                p.subject = s.subject.clone();
                p.text = opened.text;
                p.attachments = s.plan.attachments.clone();
                p.attachments_of = Some(id);
                p.in_reply_to = original.in_reply_to;
                p.references = original.references;
                p.draft = Some(id);
            }
        }
        Ok(p)
    }

    /// Puts a message together: the addresses checked, and for a reply or forward, the
    /// subject, recipients, thread and quote (and a forward's attachments) taken from the
    /// original, unless the person wrote it all ([`Compose::as_written`]).
    pub async fn compose(&self, account: &Account, c: Compose) -> anyhow::Result<Draft> {
        if c.reply_to.is_some() && c.forward.is_some() {
            anyhow::bail!("An email either replies to a message or forwards one, not both.");
        }
        let mut draft = Draft {
            to: send::mailboxes(&c.to)?,
            cc: send::mailboxes(&c.cc)?,
            bcc: send::mailboxes(&c.bcc)?,
            from: own_mailbox(account)?,
            subject: c.subject.clone().unwrap_or_default(),
            text: String::new(),
            attachments: c.attachments,
            in_reply_to: c.in_reply_to.clone(),
            references: c.references.clone(),
            left_out: None,
        };
        if let Some((id, parts)) = &c.carry {
            self.carry(&mut draft, *id, Some(parts)).await?;
        }
        if c.as_written {
            if draft.in_reply_to.is_none()
                && let Some(id) = c.reply_to
            {
                let s = self.summary(id)?;
                (draft.in_reply_to, draft.references) = thread(&s, &self.original_of(&s).await?);
            }
            draft.text = send::with_tail(&c.text, None);
            return Ok(draft);
        }
        let mut tail = None;
        if let Some(id) = c.reply_to {
            let s = self.summary(id)?;
            let original = self.original_of(&s).await?;
            (draft.in_reply_to, draft.references) = thread(&s, &original);
            let (to, cc) = reply_people(&s, &original, c.reply_all, &draft.from);
            if draft.to.is_empty() {
                draft.to = to;
            }
            for m in cc {
                let known = draft
                    .to
                    .iter()
                    .chain(&draft.cc)
                    .chain(&draft.bcc)
                    .any(|o| send::same_address(o, &m));
                if !known {
                    draft.cc.push(m);
                }
            }
            if c.subject.is_none() {
                draft.subject = send::reply_subject(&s.subject);
            }
            let opened = self.open(id).await?;
            tail = Some(send::quote(&when(s.date), &s.from.display(), &opened.text));
        }
        if let Some(id) = c.forward {
            let s = self.summary(id)?;
            let opened = self.open(id).await?;
            if c.subject.is_none() {
                draft.subject = send::forward_subject(&s.subject);
            }
            tail = Some(forwarded_text(&s, &opened.text));
            if c.carry.is_none() {
                self.carry(&mut draft, id, None).await?;
            }
        }
        draft.text = send::with_tail(&c.text, tail.as_deref());
        Ok(draft)
    }

    /// Sends a message over SMTP and keeps a copy in Sent (Gmail keeps its own). After that it
    /// marks the message it answers as answered and deletes the draft it came from. Once the
    /// message is sent the call no longer fails: what went wrong after comes back as notes.
    pub async fn send(
        &self,
        account: &Account,
        draft: &Draft,
        answers: Option<i64>,
        from_draft: Option<i64>,
    ) -> anyhow::Result<Vec<String>> {
        if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
            anyhow::bail!("The email has nobody to go to: give at least one address.");
        }
        let login = self.accounts.login(account).ok_or_else(|| {
            anyhow::anyhow!(
                "The password for {} is missing. Enter it again in the Mail window.",
                account.email
            )
        })?;
        let (host, mut port) = account.smtp();
        if port == 0 {
            port = send::probe(&host, &login).await.ok_or_else(|| {
                anyhow::anyhow!(
                    "The outgoing mail server {host} does not answer on port 465 or 587. Set the outgoing server \
                     of {} in the Mail window (connect the account again).",
                    account.email
                )
            })?;
            if let Err(e) = self.accounts.set_smtp_port(&account.id, port) {
                tracing::warn!("saving the SMTP port of {} failed: {e:#}", account.email);
            }
        }
        let built = draft.build()?;
        send::send(&host, port, &login, &built.send).await?;

        let mut notes = Vec::new();
        if let Some(id) = answers
            && let Err(e) = self.set_flag(&[id], ANSWERED, true).await
        {
            tracing::debug!("marking m{id} answered failed: {e:#}");
        }
        if account.kind == Kind::Gmail {
            if let Some(sent) = self.store.find_folder(&account.id, "sent")? {
                self.store.mark_stale(&account.id, &sent)?;
                self.emit_changed(&account.id, &sent);
            }
        } else if let Err(e) = self.keep_sent(account, &built).await {
            notes.push(format!("It was sent, but no copy could be saved in Sent: {e:#}"));
        }
        if let Some(id) = from_draft
            && let Err(e) = self.delete(&[id], true).await
        {
            notes.push(format!(
                "The draft it was sent from (m{id}) could not be deleted: {e:#}"
            ));
        }
        Ok(notes)
    }

    /// Puts the copy of a sent message in Sent, unless the server did (many do on sending).
    async fn keep_sent(&self, account: &Account, built: &send::Built) -> anyhow::Result<()> {
        let Some(sent) = self.store.find_folder(&account.id, "sent")? else {
            anyhow::bail!("the account has no Sent folder");
        };
        tokio::time::sleep(SENT_SETTLE).await;
        let found = with_conn!(self, account, |conn| async {
            anyhow::Ok(conn.find_message_id(&sent, &built.message_id).await?)
        })?;
        if found.is_empty() {
            with_conn!(self, account, |conn| async {
                anyhow::Ok(conn.append(&sent, r"(\Seen)", &built.copy).await?)
            })?;
        }
        self.store.mark_stale(&account.id, &sent)?;
        self.emit_changed(&account.id, &sent);
        Ok(())
    }

    /// Saves a message in Drafts, in place of the draft `replaces` when given. Returns notes on
    /// what went wrong after it was saved.
    pub async fn save_draft(
        &self,
        account: &Account,
        draft: &Draft,
        replaces: Option<i64>,
    ) -> anyhow::Result<Vec<String>> {
        self.folders(account, false).await?;
        let Some(drafts) = self.store.find_folder(&account.id, "drafts")? else {
            anyhow::bail!("{} has no Drafts folder.", account.email);
        };
        let built = draft.build()?;
        with_conn!(self, account, |conn| async {
            anyhow::Ok(conn.append(&drafts, r"(\Draft \Seen)", &built.copy).await?)
        })?;
        self.store.mark_stale(&account.id, &drafts)?;
        self.emit_changed(&account.id, &drafts);
        let mut notes = Vec::new();
        if let Some(id) = replaces
            && let Err(e) = self.delete(&[id], true).await
        {
            notes.push(format!("The draft it replaces (m{id}) could not be deleted: {e:#}"));
        }
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_failed_request_is_tried_again_in_parts() {
        let uids: Vec<u32> = (0..1200).collect();
        let mut queue = batches(&uids);
        assert_eq!(queue.iter().map(|b| b.len()).collect::<Vec<_>>(), [500, 500, 200]);
        let failed = queue.pop_front().unwrap();
        assert!(split_after_failure(&mut queue, failed).await);
        assert_eq!(queue.len(), 7);
        let part = queue.pop_front().unwrap();
        assert_eq!(part, &uids[..100]);
        // A small request that fails is the end of it.
        assert!(!split_after_failure(&mut queue, part).await);
        assert_eq!(queue.len(), 6);
    }

    fn trip(n: usize) -> Trip {
        Trip {
            account: "a".into(),
            from: "INBOX".into(),
            to: "Archive".into(),
            validity: None,
            messages: vec![Moved::default(); n],
            relabel: false,
        }
    }

    #[test]
    fn only_the_latest_actions_can_be_undone() {
        let mut undos = Undos::default();
        // Nothing moved, nothing to undo.
        assert_eq!(undos.keep(vec![trip(0)]), None);
        let first = undos.keep(vec![trip(1), trip(0)]).unwrap();
        assert_eq!(undos.take(first).unwrap().len(), 1);
        assert!(undos.take(first).is_none(), "an undo is used once");
        let keys: Vec<u64> = (0..UNDOS + 2).map(|_| undos.keep(vec![trip(1)]).unwrap()).collect();
        assert!(undos.take(keys[0]).is_none(), "the oldest gave way");
        assert!(undos.take(keys[UNDOS + 1]).is_some());
    }

    #[test]
    fn folder_names_follow_the_servers_separator() {
        assert_eq!(
            MailService::server_name("Work/Già fatto", ".").unwrap(),
            "Work.Gi&AOA- fatto"
        );
        assert_eq!(MailService::server_name(" Clients ", "/").unwrap(), "Clients");
        assert!(MailService::server_name("Work/", "/").is_err());
        assert!(MailService::server_name("", "/").is_err());
    }

    #[test]
    fn the_systems_folders_are_kept() {
        assert!(system_folder(None, "INBOX"));
        assert!(system_folder(Some("trash"), "[Gmail]/Trash"));
        assert!(!system_folder(None, "Clients"));
    }

    fn addr(name: &str, email: &str) -> Addr {
        Addr {
            name: name.into(),
            email: email.into(),
        }
    }

    fn message(from: Addr, to: Vec<Addr>, cc: Vec<Addr>) -> Summary {
        Summary {
            id: 1,
            account: "a".into(),
            folder: "INBOX".into(),
            uid: 1,
            date: 0,
            from,
            to,
            cc,
            subject: "Plans".into(),
            snippet: String::new(),
            unread: false,
            flagged: false,
            answered: false,
            draft: false,
            attachments: 0,
            labels: Vec::new(),
            size: 0,
            message_id: Some("abc@example.com".into()),
            plan: Default::default(),
        }
    }

    fn emails(list: &[Mailbox]) -> Vec<String> {
        list.iter().map(|m| m.email.to_string()).collect()
    }

    #[test]
    fn a_reply_goes_to_the_sender_and_a_reply_all_to_everyone_else_but_me() {
        let me: Mailbox = "me@example.com".parse().unwrap();
        let s = message(
            addr("Ada", "ada@example.com"),
            vec![addr("", "ME@example.com"), addr("Bo", "bo@example.com")],
            vec![addr("", "cy@example.com"), addr("Ada", "ada@example.com")],
        );
        let original = send::Original::default();
        let (to, cc) = reply_people(&s, &original, false, &me);
        assert_eq!(emails(&to), ["ada@example.com"]);
        assert!(cc.is_empty());
        let (to, cc) = reply_people(&s, &original, true, &me);
        assert_eq!(emails(&to), ["ada@example.com"]);
        assert_eq!(emails(&cc), ["bo@example.com", "cy@example.com"]);

        let to_list = send::Original {
            reply_to: vec![addr("List", "list@example.com")],
            ..Default::default()
        };
        let (to, _) = reply_people(&s, &to_list, false, &me);
        assert_eq!(emails(&to), ["list@example.com"]);
    }

    #[test]
    fn a_reply_to_my_own_message_goes_to_whom_it_went_to() {
        let me: Mailbox = "me@example.com".parse().unwrap();
        let s = message(
            addr("Me", "me@example.com"),
            vec![addr("Bo", "bo@example.com")],
            vec![addr("", "cy@example.com")],
        );
        let (to, cc) = reply_people(&s, &send::Original::default(), true, &me);
        assert_eq!(emails(&to), ["bo@example.com"]);
        assert_eq!(emails(&cc), ["cy@example.com"]);
    }

    #[test]
    fn a_reply_joins_the_thread() {
        let s = message(addr("", "ada@example.com"), Vec::new(), Vec::new());
        let (parent, references) = thread(&s, &send::Original::default());
        assert_eq!(parent.as_deref(), Some("<abc@example.com>"));
        assert_eq!(references, ["<abc@example.com>"]);
        let original = send::Original {
            message_id: Some("<abc@example.com>".into()),
            references: vec!["<root@example.com>".into()],
            ..Default::default()
        };
        let (_, references) = thread(&s, &original);
        assert_eq!(references, ["<root@example.com>", "<abc@example.com>"]);
    }

    #[test]
    fn addresses_are_written_as_they_are_read_back() {
        let list = written(&[addr("Doe, Jane", "jane@example.com"), addr("", "bo@example.com")]);
        let back = send::mailboxes(&list).unwrap();
        assert_eq!(emails(&back), ["jane@example.com", "bo@example.com"]);
        assert_eq!(back[0].name.as_deref(), Some("Doe, Jane"));
    }
}
