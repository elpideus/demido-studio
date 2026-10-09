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
/// The longest name an account can be given.
const MAX_NICKNAME: usize = 40;

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
    /// A short name the person gave the account, like Work: the Mail window shows it, and the
    /// mail tools take it in place of the address. Empty when it has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nickname: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub added_at: i64,
}

impl Account {
    /// The address, and the name after it when there is one: `ada@example.com ("Work")`.
    pub fn label(&self) -> String {
        if self.nickname.is_empty() {
            self.email.clone()
        } else {
            format!("{} (\"{}\")", self.email, self.nickname)
        }
    }
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
    #[serde(default)]
    pub nickname: Option<String>,
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
            nickname: clean_nickname(self.nickname.as_deref().unwrap_or(""))?,
            host,
            port,
            username,
            added_at: 0,
        };
        Ok((account, password))
    }
}

/// A name for an account, tidied: no line breaks or runs of spaces. Empty means no name.
pub fn clean_nickname(raw: &str) -> Result<String, String> {
    let name = raw
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if name.chars().count() > MAX_NICKNAME {
        return Err(format!("Keep the name to {MAX_NICKNAME} characters."));
    }
    Ok(name)
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Why `account` cannot have its name: another account has the same name, or has it as its
/// address. None when it can.
fn nickname_clash(list: &[Account], account: &Account) -> Option<String> {
    let name = &account.nickname;
    if name.is_empty() {
        return None;
    }
    let itself = |a: &Account| {
        (!account.id.is_empty() && a.id == account.id)
            || (a.email.eq_ignore_ascii_case(&account.email) && a.host == account.host)
    };
    list.iter().filter(|a| !itself(a)).find_map(|a| {
        if same_name(&a.nickname, name) {
            Some(format!("{} is already named {}.", a.email, a.nickname))
        } else if a.email.eq_ignore_ascii_case(name) {
            Some(format!("{name} is the address of another account."))
        } else {
            None
        }
    })
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

    /// The account with this id, email address or name. A name may come in quotes, and an
    /// address with the name around it, as in `Work <ada@example.com>`.
    pub fn find(&self, key: &str) -> Option<Account> {
        let key = key.trim();
        let list = self.list.read();
        let by_address = |k: &str| list.iter().find(|a| a.id == k || a.email.eq_ignore_ascii_case(k));
        let name = key.trim_matches(['"', '\'', '“', '”']).trim();
        by_address(key)
            .or_else(|| {
                list.iter()
                    .find(|a| !a.nickname.is_empty() && same_name(&a.nickname, name))
            })
            .or_else(|| {
                key.split(|c: char| c.is_whitespace() || "<>()\"'“”,;".contains(c))
                    .filter(|word| word.contains('@'))
                    .find_map(by_address)
            })
            .cloned()
    }

    /// Refuses a name another account already has, before signing in to connect one.
    pub fn check_nickname(&self, account: &Account) -> Result<(), String> {
        nickname_clash(&self.list.read(), account).map_or(Ok(()), Err)
    }

    /// Saves an account and its password. An account with the same address on the same server
    /// is updated in place, keeping its id, cache and, unless a new one is given, its name.
    /// Returns the account and whether the password is in the credential store (false: in
    /// memory only, until the app closes).
    pub fn save(&self, mut account: Account, password: &str) -> anyhow::Result<(Account, bool)> {
        let mut list = self.list.write();
        if let Some(existing) = list
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(&account.email) && a.host == account.host)
        {
            account.id = existing.id.clone();
            account.added_at = existing.added_at;
            if account.nickname.is_empty() {
                account.nickname = existing.nickname.clone();
            }
        } else {
            account.id = crate::db::new_id();
            account.added_at = crate::db::now_ms();
        }
        if let Some(clash) = nickname_clash(&list, &account) {
            anyhow::bail!(clash);
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

    /// Gives the account a name, or takes its name away when `nickname` is empty.
    pub fn set_nickname(&self, id: &str, nickname: &str) -> anyhow::Result<Account> {
        let nickname = clean_nickname(nickname).map_err(anyhow::Error::msg)?;
        let mut list = self.list.write();
        let Some(i) = list.iter().position(|a| a.id == id) else {
            anyhow::bail!("That account is no longer connected.");
        };
        let mut next = list.clone();
        next[i].nickname = nickname;
        if let Some(clash) = nickname_clash(&next, &next[i]) {
            anyhow::bail!(clash);
        }
        demido_core::fsx::write_json(&self.path, &AccountsFile { accounts: next.clone() })?;
        *list = next;
        Ok(list[i].clone())
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
            nickname: None,
        }
    }

    fn account(id: &str, email: &str, nickname: &str) -> Account {
        Account {
            id: id.into(),
            kind: Kind::Imap,
            email: email.into(),
            nickname: nickname.into(),
            host: "imap.example.com".into(),
            port: IMAPS_PORT,
            username: email.into(),
            added_at: 0,
        }
    }

    /// Accounts kept in a temporary folder; its passwords are never asked for.
    fn accounts(list: Vec<Account>) -> (tempfile::TempDir, Accounts) {
        let dir = tempfile::tempdir().unwrap();
        let accounts = Accounts {
            path: dir.path().join("mail.json"),
            list: RwLock::new(list),
            secrets: Arc::new(Secrets::default()),
        };
        (dir, accounts)
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

    #[test]
    fn accounts_are_found_by_address_or_name() {
        let (_dir, a) = accounts(vec![
            account("1", "ada@gmail.com", "Personal"),
            account("2", "ada@work.com", "Work"),
        ]);
        let find = |key: &str| a.find(key).map(|a| a.id);
        assert_eq!(find("ADA@work.com").as_deref(), Some("2"));
        assert_eq!(find("2").as_deref(), Some("2"));
        assert_eq!(find("work").as_deref(), Some("2"));
        assert_eq!(find(" \"Personal\" ").as_deref(), Some("1"));
        // The way a listing or a mail header writes an account.
        assert_eq!(find("Work <ada@work.com>").as_deref(), Some("2"));
        assert_eq!(find(&a.find("1").unwrap().label()).as_deref(), Some("1"));
        assert_eq!(find("Home"), None);
        assert_eq!(find("ada@home.com"), None);
    }

    #[test]
    fn names_are_tidy_and_each_names_one_account() {
        assert_eq!(clean_nickname("  My \n work\tbox ").unwrap(), "My work box");
        assert_eq!(clean_nickname("").unwrap(), "");
        assert!(clean_nickname(&"x".repeat(41)).is_err());
        let mut n = new(Kind::Gmail, "ada@gmail.com", "pw");
        n.nickname = Some(" Personal ".into());
        assert_eq!(n.resolve().unwrap().0.nickname, "Personal");

        let (_dir, a) = accounts(vec![
            account("1", "ada@gmail.com", "Personal"),
            account("2", "ada@work.com", ""),
        ]);
        assert!(a.set_nickname("2", "personal").is_err(), "another account has it");
        assert!(
            a.set_nickname("2", "ADA@gmail.com").is_err(),
            "another account's address"
        );
        assert_eq!(a.set_nickname("2", " Work ").unwrap().nickname, "Work");
        assert_eq!(a.find("work").unwrap().id, "2");
        assert_eq!(a.set_nickname("1", "PERSONAL").unwrap().nickname, "PERSONAL");
        assert!(a.set_nickname("3", "Home").is_err());

        // Connecting an account again may keep its name, but not take another's.
        let mut again = account("", "ada@work.com", "Work");
        assert_eq!(a.check_nickname(&again), Ok(()));
        again.nickname = "personal".into();
        assert!(a.check_nickname(&again).is_err());

        assert_eq!(a.set_nickname("2", "").unwrap().nickname, "");
        assert!(a.find("Work").is_none());
        let saved = Accounts::load(a.path.clone(), Arc::new(Secrets::default()));
        assert_eq!(saved.get("1").unwrap().nickname, "PERSONAL");
        assert_eq!(saved.get("2").unwrap().nickname, "");
    }
}
