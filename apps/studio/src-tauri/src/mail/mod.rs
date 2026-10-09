//! Mail: reading the accounts connected over IMAP (Gmail with an app password, or any IMAP
//! server over TLS), for the Mail window and the assistant's mail tools.
//!
//! Everything shown comes from a local cache ([`store`]) and the server is asked only for what
//! changed ([`sync`]). Each account has one connection for its requests, kept open while it is
//! used, and while the Mail window is open a second one waits on the inbox with IDLE, so new
//! mail shows up as it arrives and the inbox is never polled. Folders are opened read-only and
//! bodies fetched with BODY.PEEK: reading here never marks anything as read on the server.

pub mod accounts;
mod body;
mod imap;
mod store;
mod sync;
mod utf7;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use accounts::Accounts;
pub use accounts::{Account, Kind, NewAccount};
pub use body::{Addr, Attachment};
use imap::{Conn, IdleEnd, MailError};
use store::Store;

use crate::secrets::Secrets;

pub const ACCOUNTS_EVENT: &str = "mail://accounts";
/// A folder's cached messages changed: `{account, folder}`.
pub const CHANGED_EVENT: &str = "mail://changed";

/// A folder checked this recently is shown from the cache without asking the server.
const FRESH_MS: i64 = 60_000;
/// A connection unused this long is closed.
const CLOSE_UNUSED: Duration = Duration::from_secs(5 * 60);
/// IDLE is renewed this often; servers end it after 30 minutes.
const IDLE_RENEW: Duration = Duration::from_secs(20 * 60);
/// Without IDLE, the inbox is checked this often while the Mail window is open.
const POLL_EVERY: Duration = Duration::from_secs(3 * 60);
/// News is gathered this long before a sync, so a burst of changes costs one.
const DEBOUNCE: Duration = Duration::from_millis(1500);
/// The inbox stays watched this long after the Mail window closes.
const WATCH_GRACE: Duration = Duration::from_secs(120);
/// The longest wait between attempts to reconnect the watcher.
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub kind: Kind,
    pub email: String,
    pub host: String,
    /// Why the account cannot be read right now (a refused password).
    pub error: Option<String>,
    /// The password is missing and must be entered again.
    pub needs_password: bool,
    /// The credential store refused the password: it is kept only until the app closes.
    pub session_only: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub account: String,
    pub folder: String,
    pub messages: Vec<Summary>,
    /// Messages in the folder on the server, at the last check.
    pub total: Option<u32>,
    pub unseen: Option<u32>,
    /// The folder has been loaded at least once.
    pub loaded: bool,
    /// No older messages remain on the server.
    pub complete: bool,
    pub checked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedMessage {
    pub summary: Summary,
    pub html: Option<String>,
    pub text: String,
    pub attachment_list: Vec<Attachment>,
}

type Slot = Arc<tokio::sync::Mutex<Option<Conn>>>;

struct Watcher {
    stop: CancellationToken,
    /// The watcher is connected and the inbox synced: the server will say when it changes.
    live: Arc<AtomicBool>,
}

pub struct MailService {
    app: AppHandle,
    accounts: Accounts,
    store: Store,
    /// Each account's connection for requests.
    slots: Mutex<HashMap<String, Slot>>,
    /// Why an account cannot sign in, until it next does.
    errors: Mutex<HashMap<String, String>>,
    session_only: Mutex<HashSet<String>>,
    /// When each account's folder list was last read from the server.
    listed: Mutex<HashMap<String, i64>>,
    watchers: Mutex<HashMap<String, Watcher>>,
    window_open: AtomicBool,
    /// Bumped by every open or close of the Mail window, so a late stop does not undo a reopen.
    window_gen: AtomicU64,
    janitor: AtomicBool,
    closing: AtomicBool,
    me: Weak<MailService>,
}

/// Runs `$body` with the account's connection, connecting first when there is none. A broken
/// connection is dropped and the work tried once more on a new one.
macro_rules! with_conn {
    ($svc:expr, $account:expr, |$conn:ident| $body:expr) => {{
        let svc: &MailService = $svc;
        let account: &Account = $account;
        let slot = svc.slot(&account.id);
        let mut tries = 0;
        loop {
            let mut guard = slot.lock().await;
            if guard.is_none() {
                match svc.connect(account).await {
                    Ok(conn) => *guard = Some(conn),
                    Err(e) => break Err(e),
                }
            }
            let $conn = guard.as_mut().expect("connected above");
            let result: anyhow::Result<_> = $body.await;
            match result {
                Err(e) if is_broken(&e) => {
                    *guard = None;
                    tries += 1;
                    if tries < 2 {
                        continue;
                    }
                    break Err(e);
                }
                other => break other,
            }
        }
    }};
}

fn is_broken(e: &anyhow::Error) -> bool {
    matches!(e.downcast_ref::<MailError>(), Some(MailError::Network(_)))
}

impl MailService {
    pub fn new(
        app: AppHandle,
        accounts_file: PathBuf,
        cache_file: &Path,
        secrets: Arc<Secrets>,
    ) -> anyhow::Result<Arc<Self>> {
        let store = Store::open(cache_file)?;
        Ok(Arc::new_cyclic(|me| Self {
            app,
            accounts: Accounts::load(accounts_file, secrets),
            store,
            slots: Mutex::default(),
            errors: Mutex::default(),
            session_only: Mutex::default(),
            listed: Mutex::default(),
            watchers: Mutex::default(),
            window_open: AtomicBool::new(false),
            window_gen: AtomicU64::new(0),
            janitor: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            me: me.clone(),
        }))
    }

    pub fn has_accounts(&self) -> bool {
        !self.accounts.list().is_empty()
    }

    pub fn accounts(&self) -> Vec<AccountView> {
        let errors = self.errors.lock().clone();
        let session_only = self.session_only.lock().clone();
        self.accounts
            .list()
            .into_iter()
            .map(|a| AccountView {
                needs_password: !self.accounts.has_password(&a.id),
                error: errors.get(&a.id).cloned(),
                session_only: session_only.contains(&a.id),
                id: a.id,
                kind: a.kind,
                email: a.email,
                host: a.host,
            })
            .collect()
    }

    fn emit_accounts(&self) {
        let _ = self.app.emit(ACCOUNTS_EVENT, self.accounts());
    }

    fn emit_changed(&self, account: &str, folder: &str) {
        let _ = self
            .app
            .emit(CHANGED_EVENT, json!({"account": account, "folder": folder}));
    }

    fn set_error(&self, account: &str, error: Option<String>) {
        let changed = {
            let mut errors = self.errors.lock();
            match error {
                Some(e) => errors.insert(account.to_string(), e.clone()).as_ref() != Some(&e),
                None => errors.remove(account).is_some(),
            }
        };
        if changed {
            self.emit_accounts();
        }
    }

    /// The account with this id or email address, or the only one when `key` is None.
    pub fn account(&self, key: Option<&str>) -> anyhow::Result<Account> {
        let list = self.accounts.list();
        match key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => self
                .accounts
                .find(k)
                .ok_or_else(|| anyhow::anyhow!("No mail account {k}. Connected: {}.", emails(&list))),
            None => list
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No mail account is connected. Connect one in the Mail window.")),
        }
    }

    /// Checks the account by signing in, then saves it. Entering the password of an account
    /// already connected updates it.
    pub async fn add_account(&self, new: NewAccount) -> anyhow::Result<AccountView> {
        let (account, password) = new.resolve().map_err(anyhow::Error::msg)?;
        let login = imap::Login {
            host: account.host.clone(),
            port: account.port,
            username: account.username.clone(),
            password: password.clone(),
        };
        let mut conn = imap::connect(&login).await?;
        let folders = conn.list_folders().await;
        let (account, remembered) = self.accounts.save(account, &password)?;
        if let Ok(folders) = folders {
            self.store.set_folders(&account.id, &folders)?;
            self.listed.lock().insert(account.id.clone());
        }
        {
            let mut session_only = self.session_only.lock();
            if remembered {
                session_only.remove(&account.id);
            } else {
                session_only.insert(account.id.clone());
            }
        }
        self.errors.lock().remove(&account.id);
        // The connection that just signed in serves the first requests.
        let slot = self.slot(&account.id);
        let old = slot.lock().await.replace(conn);
        if let Some(old) = old {
            old.logout().await;
        }
        self.start_janitor();
        if self.window_open.load(Ordering::SeqCst) {
            self.start_watcher(&account);
        }
        self.emit_accounts();
        let id = account.id.clone();
        Ok(self
            .accounts()
            .into_iter()
            .find(|a| a.id == id)
            .expect("the account was just saved"))
    }

    pub async fn remove_account(&self, id: &str) -> anyhow::Result<()> {
        self.stop_watcher(id);
        let slot = self.slots.lock().remove(id);
        if let Some(slot) = slot {
            let conn = slot.lock().await.take();
            if let Some(conn) = conn {
                conn.logout().await;
            }
        }
        self.accounts.remove(id)?;
        self.store.remove_account(id)?;
        self.errors.lock().remove(id);
        self.session_only.lock().remove(id);
        self.listed.lock().remove(id);
        self.emit_accounts();
        Ok(())
    }

    fn slot(&self, account: &str) -> Slot {
        self.slots.lock().entry(account.to_string()).or_default().clone()
    }

    async fn connect(&self, account: &Account) -> anyhow::Result<Conn> {
        if self.closing.load(Ordering::SeqCst) {
            anyhow::bail!("Demido Studio is closing.");
        }
        let Some(login) = self.accounts.login(account) else {
            let message = format!(
                "The password for {} is missing. Enter it again in the Mail window.",
                account.email
            );
            self.set_error(&account.id, Some(message.clone()));
            anyhow::bail!(message);
        };
        match imap::connect(&login).await {
            Ok(conn) => {
                self.set_error(&account.id, None);
                self.start_janitor();
                Ok(conn)
            }
            Err(e) => {
                if let MailError::Auth(text) = &e {
                    self.set_error(&account.id, Some(text.clone()));
                }
                Err(e.into())
            }
        }
    }

    /// The account's folders. The list is read from the server once a session (or when
    /// `refresh`), from the cache otherwise.
    pub async fn folders(&self, account: &Account, refresh: bool) -> anyhow::Result<Vec<FolderView>> {
        let listed = self.listed.lock().contains(&account.id);
        if refresh || !listed {
            match with_conn!(self, account, |conn| async { Ok(conn.list_folders().await?) }) {
                Ok(folders) => {
                    self.store.set_folders(&account.id, &folders)?;
                    self.listed.lock().insert(account.id.clone());
                }
                Err(e) => {
                    let cached = self.store.folders(&account.id)?;
                    if cached.is_empty() {
                        return Err(e);
                    }
                    tracing::warn!("listing the folders of {} failed: {e:#}", account.email);
                    return Ok(cached);
                }
            }
        }
        self.store.folders(&account.id)
    }

    /// The folder `name` names (a path, a folder name, or a role: inbox, sent, drafts, all,
    /// junk, trash, flagged, important); the inbox when None.
    pub fn folder_path(&self, account: &Account, name: Option<&str>) -> anyhow::Result<String> {
        let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or("inbox");
        if let Some(path) = self.store.find_folder(&account.id, name)? {
            return Ok(path);
        }
        if name.eq_ignore_ascii_case("inbox") {
            return Ok("INBOX".into());
        }
        let known: Vec<String> = self.store.folders(&account.id)?.into_iter().map(|f| f.name).collect();
        if known.is_empty() {
            Ok(name.to_string())
        } else {
            anyhow::bail!("No folder {name} in {}. Folders: {}.", account.email, known.join(", "))
        }
    }

    /// A page of a folder's cached messages, newest first. Nothing is asked of the server.
    pub fn messages(&self, account: &Account, path: &str, q: &PageQuery<'_>) -> anyhow::Result<MessagePage> {
        let state = self.store.folder_state(&account.id, path)?;
        Ok(MessagePage {
            account: account.id.clone(),
            folder: path.to_string(),
            messages: self.store.page(&account.id, path, q)?,
            total: state.exists,
            unseen: state.unseen,
            loaded: state.low_uid.is_some(),
            complete: state.complete,
            checked_at: state.checked_at,
        })
    }

    /// Brings a folder's cache up to date, unless it is already (it was checked in the last
    /// minute, or it is the inbox and IDLE is watching it). Returns whether anything changed;
    /// a change is also announced with `mail://changed`.
    pub async fn sync(&self, account: &Account, path: &str, force: bool) -> anyhow::Result<bool> {
        let asked = crate::db::now_ms();
        if !force && self.is_fresh(account, path, asked)? {
            return Ok(false);
        }
        let changed = with_conn!(self, account, |conn| async {
            // Someone else synced the folder while this waited for the connection.
            let state = self.store.folder_state(&account.id, path)?;
            if state.low_uid.is_some() && state.checked_at.is_some_and(|t| t >= asked) {
                return anyhow::Ok(false);
            }
            sync::sync_folder(conn, &self.store, &account.id, path).await
        })?;
        if changed {
            self.emit_changed(&account.id, path);
        }
        Ok(changed)
    }

    fn is_fresh(&self, account: &Account, path: &str, now: i64) -> anyhow::Result<bool> {
        let state = self.store.folder_state(&account.id, path)?;
        if state.low_uid.is_none() {
            return Ok(false);
        }
        if state.checked_at.is_some_and(|t| now - t < FRESH_MS) {
            return Ok(true);
        }
        let live = self
            .watchers
            .lock()
            .get(&account.id)
            .is_some_and(|w| w.live.load(Ordering::SeqCst));
        Ok(live && path == self.folder_path(account, None)?)
    }

    /// Fetches the next page of older messages. Returns how many arrived.
    pub async fn load_older(&self, account: &Account, path: &str) -> anyhow::Result<u32> {
        let added = with_conn!(self, account, |conn| sync::load_older(conn, &self.store, &account.id, path))?;
        if added > 0 {
            self.emit_changed(&account.id, path);
        }
        Ok(added)
    }

    /// Searches a folder on the server (with Gmail's search syntax on Gmail): the newest
    /// `limit` matches.
    pub async fn search(&self, account: &Account, path: &str, text: &str, limit: usize) -> anyhow::Result<Vec<Summary>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
    }

    /// A cached message's summary.
    pub fn summary(&self, id: i64) -> anyhow::Result<Summary> {
        self.store
            .message(id)?
            .ok_or_else(|| anyhow::anyhow!("That message is not in the mail cache any more; list the folder again."))
    }

    /// A message with its body, from the cache when it was opened before.
    pub async fn open(&self, id: i64) -> anyhow::Result<OpenedMessage> {
        let summary = self.summary(id)?;
        let opened = |summary: Summary, body: body::Body| OpenedMessage {
            attachment_list: summary.plan.attachments.clone(),
            html: body.html,
            text: body.text,
            summary,
        };
        if let Some(body) = self.store.body(id)? {
            return Ok(opened(summary, body));
        }
        let account = self.account_of(&summary)?;
        let fetched = with_conn!(self, &account, |conn| sync::fetch_body(
            conn,
            &summary.folder,
            summary.uid,
            summary.size,
            &summary.plan
        ))?;
        let Some(body) = fetched else {
            return Err(self.gone(&summary));
        };
        self.store.save_body(id, &body)?;
        Ok(opened(self.summary(id).unwrap_or(summary), body))
    }

    /// An attachment's file name and contents.
    pub async fn attachment(&self, id: i64, section: &str) -> anyhow::Result<(String, Vec<u8>)> {
        let summary = self.summary(id)?;
        let Some(name) = summary
            .plan
            .attachments
            .iter()
            .find(|a| a.section == section)
            .map(|a| a.name.clone())
        else {
            anyhow::bail!("That message has no such attachment.");
        };
        let account = self.account_of(&summary)?;
        let fetched = with_conn!(self, &account, |conn| sync::fetch_attachment(
            conn,
            &summary.folder,
            summary.uid,
            &summary.plan,
            section
        ))?;
        let Some(bytes) = fetched else {
            return Err(self.gone(&summary));
        };
        Ok((name, bytes))
    }

    pub async fn save_attachment(&self, id: i64, section: &str, dest: &Path) -> anyhow::Result<()> {
        let (_, bytes) = self.attachment(id, section).await?;
        demido_core::fsx::write_atomic(dest, &bytes)
    }

    fn account_of(&self, summary: &Summary) -> anyhow::Result<Account> {
        self.accounts
            .get(&summary.account)
            .ok_or_else(|| anyhow::anyhow!("The account of that message was removed."))
    }

    /// The message was deleted or moved on the server: it leaves the cache too.
    fn gone(&self, summary: &Summary) -> anyhow::Error {
        if self.store.delete_message(summary.id).is_ok() {
            self.emit_changed(&summary.account, &summary.folder);
        }
        anyhow::anyhow!("This message is no longer on the server; it was deleted or moved.")
    }

    /// The Mail window opened or closed. While it is open each account's inbox is watched.
    pub fn set_watching(&self, open: bool) {
        self.window_open.store(open, Ordering::SeqCst);
        let generation = self.window_gen.fetch_add(1, Ordering::SeqCst) + 1;
        if open {
            for account in self.accounts.list() {
                self.start_watcher(&account);
            }
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(WATCH_GRACE).await;
            if me.window_gen.load(Ordering::SeqCst) == generation && !me.window_open.load(Ordering::SeqCst) {
                let ids: Vec<String> = me.watchers.lock().keys().cloned().collect();
                for id in ids {
                    me.stop_watcher(&id);
                }
            }
        });
    }

    fn start_watcher(&self, account: &Account) {
        if self.closing.load(Ordering::SeqCst) {
            return;
        }
        let mut watchers = self.watchers.lock();
        if watchers.contains_key(&account.id) {
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        let stop = CancellationToken::new();
        let live = Arc::new(AtomicBool::new(false));
        watchers.insert(
            account.id.clone(),
            Watcher {
                stop: stop.clone(),
                live: live.clone(),
            },
        );
        let account = account.clone();
        tauri::async_runtime::spawn(async move { me.watch(account, stop, live).await });
    }

    fn stop_watcher(&self, account: &str) {
        if let Some(w) = self.watchers.lock().remove(account) {
            w.live.store(false, Ordering::SeqCst);
            w.stop.cancel();
        }
    }

    /// Keeps a connection waiting on the inbox, reconnecting with growing pauses when it breaks.
    async fn watch(&self, account: Account, stop: CancellationToken, live: Arc<AtomicBool>) {
        let mut backoff = Duration::from_secs(5);
        while !stop.is_cancelled() {
            let result = self.watch_once(&account, &stop, &live, &mut backoff).await;
            live.store(false, Ordering::SeqCst);
            match result {
                Ok(()) => break,
                Err(e) => {
                    if matches!(e.downcast_ref::<MailError>(), Some(MailError::Auth(_))) {
                        // The password was refused: retrying will not help until it changes.
                        break;
                    }
                    tracing::debug!("mail watcher for {} stopped: {e:#}", account.email);
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = stop.cancelled() => break,
            }
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
        let mut watchers = self.watchers.lock();
        if watchers.get(&account.id).is_some_and(|w| Arc::ptr_eq(&w.live, &live)) {
            watchers.remove(&account.id);
        }
    }

    async fn watch_once(
        &self,
        account: &Account,
        stop: &CancellationToken,
        live: &AtomicBool,
        backoff: &mut Duration,
    ) -> anyhow::Result<()> {
        let inbox = self.folder_path(account, None)?;
        let mut conn = self.connect(account).await?;
        if !conn.caps.idle {
            // Without IDLE the inbox is checked now and then, over the account's connection. It
            // does not count as watched, so the window still checks it when it comes to the front.
            conn.logout().await;
            loop {
                if let Err(e) = self.sync(account, &inbox, true).await {
                    tracing::debug!("checking the inbox of {} failed: {e:#}", account.email);
                }
                *backoff = Duration::from_secs(5);
                tokio::select! {
                    _ = tokio::time::sleep(POLL_EVERY) => {}
                    _ = stop.cancelled() => return Ok(()),
                }
            }
        }
        let result = async {
            conn.examine(&inbox).await?;
            // Catch up on what changed before the watch began; from here the server says.
            self.watch_sync(account, &inbox, live).await;
            *backoff = Duration::from_secs(5);
            loop {
                match conn.idle(IDLE_RENEW, stop).await? {
                    IdleEnd::Stopped => return Ok(()),
                    IdleEnd::Quiet => {}
                    IdleEnd::News => {
                        tokio::select! {
                            _ = tokio::time::sleep(DEBOUNCE) => {}
                            _ = stop.cancelled() => return Ok(()),
                        }
                        self.watch_sync(account, &inbox, live).await;
                    }
                }
            }
        }
        .await;
        conn.logout().await;
        result
    }

    /// A sync for the watcher. The inbox counts as watched only while its syncs succeed.
    async fn watch_sync(&self, account: &Account, inbox: &str, live: &AtomicBool) {
        match self.sync(account, inbox, true).await {
            Ok(_) => live.store(true, Ordering::SeqCst),
            Err(e) => {
                live.store(false, Ordering::SeqCst);
                tracing::debug!("syncing the inbox of {} failed: {e:#}", account.email);
            }
        }
    }

    /// Closes connections unused for a while; the one serving a watched account stays open.
    fn start_janitor(&self) {
        if self.janitor.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(me) = me.upgrade() else { return };
                if me.closing.load(Ordering::SeqCst) {
                    return;
                }
                let slots: Vec<(String, Slot)> = me.slots.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                for (account, slot) in slots {
                    if me.watchers.lock().contains_key(&account) {
                        continue;
                    }
                    let Ok(mut guard) = slot.try_lock() else { continue };
                    if guard.as_ref().is_some_and(|c| c.last_used.elapsed() >= CLOSE_UNUSED)
                        && let Some(conn) = guard.take()
                    {
                        drop(guard);
                        conn.logout().await;
                    }
                }
            }
        });
    }

    /// Stops the watchers and signs out of the servers, briefly: the app is exiting.
    pub async fn shutdown(&self, grace: Duration) {
        self.closing.store(true, Ordering::SeqCst);
        for (_, w) in self.watchers.lock().drain() {
            w.stop.cancel();
        }
        let slots: Vec<Slot> = self.slots.lock().values().cloned().collect();
        let logouts = slots.into_iter().map(|slot| async move {
            let conn = match slot.try_lock() {
                Ok(mut guard) => guard.take(),
                Err(_) => None,
            };
            if let Some(conn) = conn {
                conn.logout().await;
            }
        });
        let _ = tokio::time::timeout(grace, futures_util::future::join_all(logouts)).await;
    }
}

fn emails(list: &[Account]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    list.iter().map(|a| a.email.as_str()).collect::<Vec<_>>().join(", ")
}
