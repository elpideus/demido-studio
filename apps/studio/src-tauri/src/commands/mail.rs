use std::path::PathBuf;

use super::St;
use crate::error::CmdResult;
use crate::mail::{AccountView, DateRange, FolderView, MessagePage, NewAccount, OpenedMessage, PageQuery, Summary};

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

#[tauri::command]
pub async fn mail_folders(state: St<'_>, account: String, refresh: Option<bool>) -> CmdResult<Vec<FolderView>> {
    let account = state.mail.account(Some(&account))?;
    Ok(state.mail.folders(&account, refresh.unwrap_or(false)).await?)
}

/// A page of cached messages; the server is not asked (see `mail_sync`).
#[tauri::command]
pub fn mail_messages(
    state: St<'_>,
    account: String,
    folder: String,
    limit: u32,
    filter: Option<String>,
    unread_only: Option<bool>,
) -> CmdResult<MessagePage> {
    let account = state.mail.account(Some(&account))?;
    let query = PageQuery {
        limit: limit.clamp(1, 5000),
        filter: filter.as_deref(),
        unread_only: unread_only.unwrap_or(false),
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

#[tauri::command]
pub async fn mail_search(
    state: St<'_>,
    account: String,
    folder: String,
    query: String,
    limit: Option<u32>,
) -> CmdResult<Vec<Summary>> {
    let account = state.mail.account(Some(&account))?;
    let limit = limit.unwrap_or(100).clamp(1, 500) as usize;
    Ok(state.mail.search(&account, &folder, &query, limit).await?)
}

#[tauri::command]
pub async fn mail_open(state: St<'_>, id: i64) -> CmdResult<OpenedMessage> {
    Ok(state.mail.open(id).await?)
}

#[tauri::command]
pub async fn mail_save_attachment(state: St<'_>, id: i64, section: String, path: PathBuf) -> CmdResult<()> {
    Ok(state.mail.save_attachment(id, &section, &path).await?)
}

/// The Mail window opened or closed: while it is open the inboxes are watched for new mail.
#[tauri::command]
pub fn mail_set_watching(state: St<'_>, open: bool) {
    state.mail.set_watching(open);
}
