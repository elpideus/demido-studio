//! The mail accounts: `mail.json` in the data folder lists them, their passwords live in the
//! system's credential store.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::imap::Login;
use crate::secrets::{Secrets, mail_key};

pub const GMAIL_HOST: &str = "imap.gmail.com";
/// IMAP over TLS.
pub const IMAPS_PORT: u16 = 993;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Gmail,
    /// Any other IMAP server.
    Imap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub kind: Kind,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub added_at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct AccountsFile {
    #[serde(default)]
    accounts: Vec<Account>,
}

/// An account as the connect form describes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub kind: Kind,
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
}

impl NewAccount {
    /// The account's server settings and password, checked and completed.
    pub fn resolve(&self) -> Result<(Account, String), String> {
        let email = self.email.trim().to_string();
        let valid = email
            .split_once('@')
            .is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.'))
            && !email.contains(char::is_whitespace);
        if !valid {
            return Err("Enter the whole email address, like name@gmail.com.".into());
        }
        let (host, port, username, password) = match self.kind {
            // Google shows app passwords in groups of four letters; the spaces are not part of it.
            Kind::Gmail => (
                GMAIL_HOST.to_string(),
                IMAPS_PORT,
                email.clone(),
                self.password.chars().filter(|c| !c.is_whitespace()).collect(),
            ),
            Kind::Imap => {
                let host = self
                    .host
                    .as_deref()
                    .map(str::trim)
                    .filter(|h| !h.is_empty())
                    .ok_or("Enter the IMAP server, like imap.example.com.")?;
                let username = self.username.as_deref().map(str::trim).filter(|u| !u.is_empty());
                (
                    host.to_lowercase(),
                    self.port.filter(|p| *p != 0).unwrap_or(IMAPS_PORT),
                    username.unwrap_or(&email).to_string(),
                    self.password.clone(),
                )
            }
        };
        if password.trim().is_empty() {
            return Err(match self.kind {
                Kind::Gmail => "Enter the app password Google gave you.".into(),
                Kind::Imap => "Enter the password.".into(),
            });
        }
        let account = Account {
            id: String::new(),
            kind: self.kind,
            email,
            host,
            port,
            username,
            added_at: 0,
        };
        Ok((account, password))
    }
}

pub struct Accounts {
    path: PathBuf,
    list: RwLock<Vec<Account>>,
    secrets: Arc<Secrets>,
}

impl Accounts {
    pub fn load(path: PathBuf, secrets: Arc<Secrets>) -> Self {
        let list = demido_core::fsx::read_json::<AccountsFile>(&path)
            .map(|f| f.accounts)
            .unwrap_or_default();
        Self {
            path,
            list: RwLock::new(list),
            secrets,
        }
    }

    pub fn list(&self) -> Vec<Account> {
        self.list.read().clone()
    }

    pub fn get(&self, id: &str) -> Option<Account> {
        self.list.read().iter().find(|a| a.id == id).cloned()
    }

    /// The account with this id or email address.
    pub fn find(&self, key: &str) -> Option<Account> {
        let key = key.trim();
        self.list
            .read()
            .iter()
            .find(|a| a.id == key || a.email.eq_ignore_ascii_case(key))
            .cloned()
    }

    /// Saves an account and its password. An account with the same address on the same server
    /// is updated in place, keeping its id and cache. Returns the account and whether the
    /// password is in the credential store (false: in memory only, until the app closes).
    pub fn save(&self, mut account: Account, password: &str) -> anyhow::Result<(Account, bool)> {
        let mut list = self.list.write();
        if let Some(existing) = list
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(&account.email) && a.host == account.host)
        {
            account.id = existing.id.clone();
            account.added_at = existing.added_at;
        } else {
            account.id = crate::db::new_id();
            account.added_at = crate::db::now_ms();
        }
        let remembered = self.secrets.set(&mail_key(&account.id), password);
        let mut next = list.clone();
        match next.iter_mut().find(|a| a.id == account.id) {
            Some(slot) => *slot = account.clone(),
            None => next.push(account.clone()),
        }
        demido_core::fsx::write_json(&self.path, &AccountsFile { accounts: next.clone() })?;
        *list = next;
        Ok((account, remembered))
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<Option<Account>> {
        let mut list = self.list.write();
        let Some(i) = list.iter().position(|a| a.id == id) else {
            return Ok(None);
        };
        let mut next = list.clone();
        let removed = next.remove(i);
        demido_core::fsx::write_json(&self.path, &AccountsFile { accounts: next.clone() })?;
        *list = next;
        self.secrets.delete(&mail_key(id));
        Ok(Some(removed))
    }

    /// What signing in to the account takes; None when its password is missing.
    pub fn login(&self, account: &Account) -> Option<Login> {
        let password = self.secrets.get(&mail_key(&account.id))?;
        Some(Login {
            host: account.host.clone(),
            port: account.port,
            username: account.username.clone(),
            password,
        })
    }

    pub fn has_password(&self, id: &str) -> bool {
        self.secrets.get(&mail_key(id)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(kind: Kind, email: &str, password: &str) -> NewAccount {
        NewAccount {
            kind,
            email: email.into(),
            password: password.into(),
            host: None,
            port: None,
            username: None,
        }
    }

    #[test]
    fn gmail_accounts_need_only_the_address_and_app_password() {
        let (a, password) = new(Kind::Gmail, " ada@gmail.com ", "abcd efgh ijkl mnop")
            .resolve()
            .unwrap();
        assert_eq!(
            (a.host.as_str(), a.port, a.username.as_str()),
            (GMAIL_HOST, 993, "ada@gmail.com")
        );
        assert_eq!(password, "abcdefghijklmnop");
        assert!(new(Kind::Gmail, "ada", "x").resolve().is_err());
        assert!(new(Kind::Gmail, "ada@gmail.com", "  ").resolve().is_err());
    }

    #[test]
    fn other_servers_need_a_host() {
        assert!(new(Kind::Imap, "ada@example.com", "pw").resolve().is_err());
        let mut n = new(Kind::Imap, "ada@example.com", "pw with spaces");
        n.host = Some(" IMAP.Example.com ".into());
        let (a, password) = n.resolve().unwrap();
        assert_eq!(
            (a.host.as_str(), a.port, a.username.as_str()),
            ("imap.example.com", 993, "ada@example.com")
        );
        assert_eq!(password, "pw with spaces");
    }
}
