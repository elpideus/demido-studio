use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::St;
use crate::error::{AppError, CmdResult};
use crate::mail::{
    Account, AccountView, Changed, Compose, DateRange, Deleted, Draft, FLAGGED, FolderView, MailService, MessagePage,
    NewAccount, OpenedMessage, PageQuery, Prefill, SEEN, Start, Summary, read_files,
};

#[tauri::command]
pub fn mail_accounts(state: St<'_>) -> Vec<AccountView> {
    state.mail.accounts()
}

/// Signs in to check the account, then saves it (or its new password).
#[tauri::command]
pub async fn mail_add_account(state: St<'_>, account: NewAccount) -> CmdResult<AccountView> {
    Ok(state.mail.add_account(account).await?)
}

#[tauri::command]
pub async fn mail_remove_account(state: St<'_>, id: String) -> CmdResult<()> {
    Ok(state.mail.remove_account(&id).await?)
}

/// Names the account, or takes its name away when `nickname` is empty or missing.
#[tauri::command]
pub fn mail_rename_account(state: St<'_>, id: String, nickname: Option<String>) -> CmdResult<AccountView> {
    Ok(state.mail.rename_account(&id, nickname.as_deref().unwrap_or(""))?)
}

#[tauri::command]
pub async fn mail_folders(state: St<'_>, account: String, refresh: Option<bool>) -> CmdResult<Vec<FolderView>> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.folders(&account, refresh.unwrap_or(false)).await?)
}

/// A page of cached messages; the server is not asked (see `mail_sync`). `since` and `until`
/// (ms, both included) keep the mail that arrived between them.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn mail_messages(
    state: St<'_>,
    account: String,
    folder: String,
    limit: u32,
    filter: Option<String>,
    unread_only: Option<bool>,
    since: Option<i64>,
    until: Option<i64>,
) -> CmdResult<MessagePage> {
    let account = state.mail.account(Some(&account))?;
    let query = PageQuery {
        limit: limit.clamp(1, 5000),
        filter: filter.as_deref(),
        unread_only: unread_only.unwrap_or(false),
        dates: DateRange { since, until },
    };
    Ok(state.mail.messages(&account, &folder, &query)?)
}

/// Brings a folder up to date with the server when it may be behind. Changes are announced with
/// `mail://changed`.
#[tauri::command]
pub async fn mail_sync(state: St<'_>, account: String, folder: String, force: Option<bool>) -> CmdResult<bool> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.sync(&account, &folder, force.unwrap_or(false)).await?)
}

#[tauri::command]
pub async fn mail_load_older(state: St<'_>, account: String, folder: String) -> CmdResult<u32> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.load_older(&account, &folder).await?)
}

/// Searches a folder on the server for `query`, mail that arrived between `since` and `until`
/// (ms, both included), or both.
#[tauri::command]
pub async fn mail_search(
    state: St<'_>,
    account: String,
    folder: String,
    query: String,
    since: Option<i64>,
    until: Option<i64>,
    limit: Option<u32>,
) -> CmdResult<Vec<Summary>> {
    let account = state.mail.account(Some(&account))?;
    let limit = limit.unwrap_or(100).clamp(1, 500) as usize;
    let dates = DateRange { since, until };
    Ok(state.mail.search(&account, &folder, &query, &dates, limit).await?)
}

#[tauri::command]
pub async fn mail_open(state: St<'_>, id: i64) -> CmdResult<OpenedMessage> {
    Ok(state.mail.open(id).await?)
}

#[tauri::command]
pub async fn mail_save_attachment(state: St<'_>, id: i64, section: String, path: PathBuf) -> CmdResult<()> {
    Ok(state.mail.save_attachment(id, &section, &path).await?)
}

/// Marks messages as read on the server. Returns how many were unread.
#[tauri::command]
pub async fn mail_mark_read(state: St<'_>, ids: Vec<i64>) -> CmdResult<u32> {
    let report = state.mail.mark_read(&ids).await?;
    if let Some(why) = report.stopped {
        tracing::warn!("marking mail as read stopped partway: {why}");
    }
    Ok(report.changed as u32)
}

/// Sets or clears a flag on messages: `read` or `starred`.
#[tauri::command]
pub async fn mail_set_flag(state: St<'_>, ids: Vec<i64>, flag: String, on: bool) -> CmdResult<Changed> {
    let bit = match flag.as_str() {
        "read" => SEEN,
        "starred" => FLAGGED,
        other => return Err(AppError::msg(format!("There is no flag called {other}."))),
    };
    Ok(state.mail.set_flag(&ids, bit, on).await?)
}

/// Moves messages to a folder (its path, or `archive`), or with `copy` copies them there.
#[tauri::command]
pub async fn mail_move(state: St<'_>, ids: Vec<i64>, folder: String, copy: Option<bool>) -> CmdResult<Changed> {
    Ok(state.mail.move_to(&ids, &folder, copy.unwrap_or(false)).await?)
}

/// Moves messages to the trash, or deletes them for good with `permanent` (and those already
/// in the trash, or in an account without one).
#[tauri::command]
pub async fn mail_delete(state: St<'_>, ids: Vec<i64>, permanent: Option<bool>) -> CmdResult<Deleted> {
    Ok(state.mail.delete(&ids, permanent.unwrap_or(false)).await?)
}

/// Puts back what a `mail_move` or `mail_delete` took away, by the `undo` it returned.
#[tauri::command]
pub async fn mail_undo(state: St<'_>, undo: u64) -> CmdResult<Changed> {
    Ok(state.mail.undo(undo).await?)
}

/// Whether `mail_delete` would delete any of these messages for good, so the window asks first.
#[tauri::command]
pub fn mail_deletes_for_good(state: St<'_>, ids: Vec<i64>, permanent: Option<bool>) -> CmdResult<bool> {
    Ok(state.mail.deletes_for_good(&ids, permanent.unwrap_or(false))?)
}

/// Makes a folder (a label on Gmail), inside `parent` when given. Returns its path.
#[tauri::command]
pub async fn mail_create_folder(
    state: St<'_>,
    account: String,
    name: String,
    parent: Option<String>,
) -> CmdResult<String> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.create_folder(&account, &name, parent.as_deref()).await?)
}

/// Renames a folder the person made. Returns its new path.
#[tauri::command]
pub async fn mail_rename_folder(state: St<'_>, account: String, folder: String, name: String) -> CmdResult<String> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.rename_folder(&account, &folder, &name).await?)
}

/// Deletes a folder the person made, with the mail in it (on Gmail, only the label).
#[tauri::command]
pub async fn mail_delete_folder(state: St<'_>, account: String, folder: String) -> CmdResult<String> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.delete_folder(&account, &folder).await?)
}

/// Deletes everything in the trash or the spam folder for good. Returns how many.
#[tauri::command]
pub async fn mail_empty_folder(state: St<'_>, account: String, folder: String) -> CmdResult<u32> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.empty_folder(&account, &folder).await? as u32)
}

/// Marks a whole folder read. Returns how many were unread.
#[tauri::command]
pub async fn mail_mark_folder_read(state: St<'_>, account: String, folder: String) -> CmdResult<u32> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.mark_folder_read(&account, &folder).await? as u32)
}

/// What the editor starts from to reply to, forward or finish message `id`.
#[tauri::command]
pub async fn mail_prefill(state: St<'_>, id: i64, start: Start) -> CmdResult<Prefill> {
    Ok(state.mail.prefill(id, start).await?)
}

/// Attachments of another message that go with the one written.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Carry {
    id: i64,
    sections: Vec<String>,
}

/// A message written in the Mail window, sent as it is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outgoing {
    account: String,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    text: String,
    /// Files on this computer to attach.
    #[serde(default)]
    files: Vec<PathBuf>,
    carry: Option<Carry>,
    reply_to: Option<i64>,
    forward: Option<i64>,
    in_reply_to: Option<String>,
    #[serde(default)]
    references: Vec<String>,
    /// The draft it was finished from.
    draft: Option<i64>,
}

/// The message put together. Attachments that cannot go with it fail it: the person sees them
/// in the editor and would not know they were left out.
async fn put_together(mail: &MailService, m: Outgoing) -> CmdResult<(Account, Draft, Option<i64>, Option<i64>)> {
    let account = mail.account(Some(&m.account))?;
    let c = Compose {
        to: m.to,
        cc: m.cc,
        bcc: m.bcc,
        subject: Some(m.subject),
        text: m.text,
        attachments: read_files(&m.files)?,
        reply_to: m.reply_to,
        forward: m.forward,
        as_written: true,
        in_reply_to: m.in_reply_to,
        references: m.references,
        carry: m.carry.map(|c| (c.id, c.sections)),
        ..Compose::default()
    };
    let draft = mail.compose(&account, c).await?;
    if let Some(left) = &draft.left_out {
        return Err(AppError::msg(left.clone()));
    }
    Ok((account, draft, m.reply_to, m.draft))
}

/// Sends a message written in the Mail window. Returns notes on what went wrong after it was
/// sent.
#[tauri::command]
pub async fn mail_send(state: St<'_>, message: Outgoing) -> CmdResult<Vec<String>> {
    let (account, draft, answers, from_draft) = put_together(&state.mail, message).await?;
    Ok(state.mail.send(&account, &draft, answers, from_draft).await?)
}

/// Saves a message written in the Mail window in Drafts, in place of the draft it was finished
/// from. Returns notes on what went wrong after it was saved.
#[tauri::command]
pub async fn mail_save_draft(state: St<'_>, message: Outgoing) -> CmdResult<Vec<String>> {
    let (account, draft, _, from_draft) = put_together(&state.mail, message).await?;
    Ok(state.mail.save_draft(&account, &draft, from_draft).await?)
}

/// A file picked to attach.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    path: PathBuf,
    name: String,
    size: u64,
}

/// The names and sizes of files picked to attach.
#[tauri::command]
pub fn mail_attach_files(paths: Vec<PathBuf>) -> CmdResult<Vec<PickedFile>> {
    let mut out = Vec::new();
    for path in paths {
        let meta = std::fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(AppError::msg(format!("{} is not a file.", path.display())));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        out.push(PickedFile {
            name,
            size: meta.len(),
            path,
        });
    }
    Ok(out)
}

/// The Mail window opened or closed.
#[tauri::command]
pub fn mail_window_open(state: St<'_>, open: bool) {
    state.mail.set_window_open(open);
}
