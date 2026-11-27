//! Where keys are kept: the vault.
//!
//! A provider's key is the person's money, so (DECISIONS U1–U3, research note 05 §4.6):
//!
//! - **Its value is kept in the system's own keyring** when this computer has one: the Keychain on macOS,
//!   the Credential Manager on Windows, the Secret Service on Linux (KWallet, GNOME Keyring). Where there is
//!   none, as on a server without a desktop, it is kept in a file only the person's user can read
//!   (`vault-keys.json`, mode 0600 on Linux and macOS), and the Providers screen says which. Each key
//!   remembers where it went, so a computer that has a keyring one day and not the next still finds it.
//! - **A value goes in and never comes out.** The API, the command line, the log and the events give a
//!   key's name, where it is kept and its last four characters. Only the core reads a value, at the moment
//!   it calls the provider.
//! - **A model never sees one.** What a tool gives back has every kept value taken out before a model or
//!   the person reads it (`hide_in`), and a tool asked for with a kept value in it is not run (`holds_one`).
//!   A program that reads the file, or a page that echoes a key, gives back `[a key Demido keeps]`.
//!
//! What is written down about the keys, without their values, is `vault.json`.
//!
//! A key is named for its provider and the person's account there, `<provider>.<account>`: a provider may
//! hold several (DECISIONS U32).
//!
//! Not built yet: a vault file locked with a passphrase or by systemd, for servers (note 05 §4.6). The Hugging
//! Face token for gated models is kept here too (`huggingface.main`, DECISIONS H8), and the person's keys for
//! search services (`brave.main`, `exa.main`, `tavily.main`, DECISIONS N17).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Error, Result, now_ms};

/// The keys there can be: a name, and what it is for.
pub const KNOWN: &[(&str, &str)] = &[
    ("anthropic", "Anthropic API key"),
    ("gemini", "Gemini API key (Google AI Studio)"),
    ("openrouter", "OpenRouter API key"),
    ("huggingface", "Hugging Face token, for gated models"),
    ("brave", "Brave Search API key"),
    ("exa", "Exa API key"),
    ("tavily", "Tavily API key"),
    ("email", "Email password, or a Gmail app password"),
    ("bot", "A Telegram or Discord bot's token"),
    ("linked", "The sign-in of your own WhatsApp, Telegram or Discord account: a token or a session"),
];

/// What a kept value is replaced with, wherever it would otherwise be shown.
pub const HIDDEN: &str = "[a key Demido keeps]";

/// A value shorter than this is not looked for in text: it would match ordinary words. No provider's key is.
const SHORTEST_LOOKED_FOR: usize = 8;

/// The name the keyring shows for Demido's keys.
const SERVICE: &str = "Demido Studio";

/// What is known about a kept key. Never its value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct KeyFacts {
    /// For example `anthropic`.
    pub name: String,
    /// What it is for, for a person.
    pub label: String,
    /// `keyring` (the system's own) or `file` (`vault-keys.json`, readable only by this user).
    pub kept_in: String,
    /// Its last four characters, so the person can tell which key it is.
    pub ends_with: String,
    /// Milliseconds since 1970.
    pub added_at: i64,
}

/// The vault as the person may see it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct VaultView {
    /// Where a key added now goes: `keyring` or `file`.
    pub new_keys_go_to: String,
    /// The same in words, with the reason when it is the file.
    pub where_words: String,
    pub keys: Vec<KeyFacts>,
    /// The keys there can be, by name, with what each is for.
    pub known: Vec<KnownKey>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct KnownKey {
    pub name: String,
    pub label: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    keys: Vec<KeyFacts>,
}

/// The system's keyring, or why Demido does not use one.
enum Keyring {
    Usable(Arc<keyring_core::CredentialStore>, &'static str),
    None(String),
}

pub struct Vault {
    /// `vault.json`: what is known about each key, without its value.
    index: PathBuf,
    /// `vault-keys.json`: the values of the keys that are kept in a file.
    file: PathBuf,
    /// Tells these keys apart from those of another Demido on the same computer (another `DEMIDO_HOME`).
    tag: String,
    keyring: Keyring,
    /// Values already read while Demido runs, by name, with the moment the key was added (a key replaced
    /// by the command line has another moment, and is read again).
    read: Mutex<HashMap<String, (i64, String)>>,
    /// One change at a time.
    writing: Mutex<()>,
}

/// Runs `f` on a thread of its own. The keyrings' libraries may start their own async runtime, which must
/// not happen on one of the server's threads.
fn apart<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join()).unwrap_or_else(|_| panic!("the keyring's thread stopped unexpectedly"))
}

fn platform_keyring() -> Keyring {
    #[cfg(target_os = "linux")]
    {
        match apart(zbus_secret_service_keyring_store::Store::new) {
            Ok(store) => Keyring::Usable(store, "the Secret Service, your desktop's keyring (KWallet or GNOME Keyring)"),
            Err(e) => Keyring::None(format!("this session has no Secret Service ({e})")),
        }
    }
    #[cfg(target_os = "macos")]
    {
        match apart(apple_native_keyring_store::keychain::Store::new) {
            Ok(store) => Keyring::Usable(store, "the macOS Keychain"),
            Err(e) => Keyring::None(format!("the Keychain could not be opened ({e})")),
        }
    }
    #[cfg(windows)]
    {
        match apart(windows_native_keyring_store::Store::new) {
            Ok(store) => Keyring::Usable(store, "the Windows Credential Manager"),
            Err(e) => Keyring::None(format!("the Credential Manager could not be opened ({e})")),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Keyring::None("Demido does not know this system's keyring".into())
    }
}

/// Writes a file only this user can read, whole or not at all: to a new file first, then in its place.
fn write_private(path: &Path, text: &str) -> Result<()> {
    let new = path.with_extension(format!("new-{}", uuid::Uuid::new_v4()));
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.mode(0o600);
        }
        // On Windows the user's data folder is private to the user already.
        let mut f = open.open(&new)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&new, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    Ok(written?)
}

/// What a key is for. A key is named `<provider>.<account>`: which provider, and which of the person's
/// accounts there (DECISIONS U32).
fn label_of(name: &str) -> Option<&'static str> {
    let (provider, account) = name.split_once('.')?;
    if account.is_empty() || !account.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    KNOWN.iter().find(|(n, _)| *n == provider).map(|(_, label)| *label)
}

/// The last four characters of a value.
fn last_four(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    chars[chars.len().saturating_sub(4)..].iter().collect()
}

impl Vault {
    /// The vault of the Demido whose data folder is `home`. Uses the system's keyring when there is one,
    /// unless `DEMIDO_VAULT=file` says to keep keys in the file (a server, or a trial that must not touch
    /// the person's keyring). Tests never touch a keyring.
    pub fn open(home: &Path) -> Self {
        let keyring = if cfg!(test) {
            Keyring::None("tests keep keys in the file".into())
        } else if std::env::var("DEMIDO_VAULT").is_ok_and(|v| v.trim() == "file") {
            Keyring::None("DEMIDO_VAULT=file says to keep keys in the file".into())
        } else {
            platform_keyring()
        };
        Self::with(home, keyring)
    }

    /// The same, with keys only ever kept in the file. For tests.
    pub fn in_file(home: &Path) -> Self {
        Self::with(home, Keyring::None("tests keep keys in the file".into()))
    }

    fn with(home: &Path, keyring: Keyring) -> Self {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(home.to_string_lossy().as_bytes());
        let tag = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
        Self { index: home.join("vault.json"), file: home.join("vault-keys.json"), tag, keyring, read: Mutex::default(), writing: Mutex::default() }
    }

    /// How a key is named in the keyring: `anthropic (1a2b3c4d)`, the second part telling this Demido's
    /// data folder from another's.
    fn account(&self, name: &str) -> String {
        format!("{name} ({})", self.tag)
    }

    fn index(&self) -> Index {
        std::fs::read_to_string(&self.index).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn file_values(&self) -> HashMap<String, String> {
        std::fs::read_to_string(&self.file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn reading(&self) -> std::sync::MutexGuard<'_, HashMap<String, (i64, String)>> {
        self.read.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The keys, without their values, and where a new one would go.
    pub fn view(&self) -> VaultView {
        let (new_keys_go_to, where_words) = match &self.keyring {
            Keyring::Usable(_, words) => ("keyring", format!("Keys are kept in {words}.")),
            Keyring::None(why) => ("file", format!("Keys are kept in {}, which only your user can read: {why}.", self.file.display())),
        };
        VaultView {
            new_keys_go_to: new_keys_go_to.into(),
            where_words,
            keys: self.index().keys,
            known: KNOWN.iter().map(|(name, label)| KnownKey { name: name.to_string(), label: label.to_string() }).collect(),
        }
    }

    /// What is known about one key, if it is kept.
    pub fn facts(&self, name: &str) -> Option<KeyFacts> {
        self.index().keys.into_iter().find(|k| k.name == name)
    }

    /// Keeps a key, in place of the one of that name if there was one. Returns what is known about it now.
    pub fn put(&self, name: &str, value: &str) -> Result<KeyFacts> {
        let Some(label) = label_of(name) else {
            return Err(Error::NotFound(format!(
                "Demido keeps no key called \"{name}\". A key is named after its provider and account, as <provider>.<account>, for one of: {}.",
                KNOWN.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            )));
        };
        let value = value.trim();
        if value.chars().count() < SHORTEST_LOOKED_FOR {
            return Err(Error::Invalid(format!("That is too short to be a key: a key is at least {SHORTEST_LOOKED_FOR} characters.")));
        }
        if value.len() > 4096 || !value.chars().all(|c| c.is_ascii_graphic()) {
            return Err(Error::Invalid("That is not a key: a key is one word of letters, digits and signs, with no spaces.".into()));
        }
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = self.index();
        let before = index.keys.iter().find(|k| k.name == name).cloned();
        // Into the keyring when there is one. When it says no, into the file, and the person is told where it went.
        let kept_in = match &self.keyring {
            Keyring::Usable(store, _) => {
                let (store, account, secret) = (store.clone(), self.account(name), value.to_string());
                match apart(move || keyring_core::Entry::new_in_store(&store, SERVICE, &account).and_then(|entry| entry.set_password(&secret))) {
                    Ok(()) => "keyring",
                    Err(e) => {
                        tracing::warn!("The keyring did not take the {name} key, so it goes to the file: {e}");
                        "file"
                    }
                }
            }
            Keyring::None(_) => "file",
        };
        let mut in_file = self.file_values();
        if kept_in == "file" {
            in_file.insert(name.to_string(), value.to_string());
            write_private(&self.file, &serde_json::to_string_pretty(&in_file)?)?;
        } else if in_file.remove(name).is_some() {
            // It was in the file before: it is not there any more.
            write_private(&self.file, &serde_json::to_string_pretty(&in_file)?)?;
        }
        // It was in the keyring before and is in the file now: the old one goes.
        if kept_in == "file" && before.as_ref().is_some_and(|b| b.kept_in == "keyring") {
            self.forget_in_keyring(name);
        }
        let facts = KeyFacts { name: name.into(), label: label.into(), kept_in: kept_in.into(), ends_with: last_four(value), added_at: now_ms() };
        index.keys.retain(|k| k.name != name);
        index.keys.push(facts.clone());
        index.keys.sort_by(|a, b| a.name.cmp(&b.name));
        write_private(&self.index, &serde_json::to_string_pretty(&index)?)?;
        self.reading().insert(name.to_string(), (facts.added_at, value.to_string()));
        Ok(facts)
    }

    fn forget_in_keyring(&self, name: &str) {
        if let Keyring::Usable(store, _) = &self.keyring {
            let (store, account) = (store.clone(), self.account(name));
            let gone = apart(move || keyring_core::Entry::new_in_store(&store, SERVICE, &account).and_then(|entry| entry.delete_credential()));
            if let Err(e) = gone
                && !matches!(e, keyring_core::Error::NoEntry)
            {
                tracing::warn!("The {name} key could not be taken out of the keyring: {e}");
            }
        }
    }

    /// Lets a key go: it is deleted from wherever it was kept.
    pub fn remove(&self, name: &str) -> Result<KeyFacts> {
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = self.index();
        let Some(gone) = index.keys.iter().find(|k| k.name == name).cloned() else {
            return Err(Error::NotFound(format!("No \"{name}\" key is kept.")));
        };
        if gone.kept_in == "keyring" {
            self.forget_in_keyring(name);
        }
        let mut in_file = self.file_values();
        if in_file.remove(name).is_some() {
            write_private(&self.file, &serde_json::to_string_pretty(&in_file)?)?;
        }
        index.keys.retain(|k| k.name != name);
        write_private(&self.index, &serde_json::to_string_pretty(&index)?)?;
        self.reading().remove(name);
        Ok(gone)
    }

    /// The value of a key, for the core to send to its provider. `None` when no such key is kept.
    pub(crate) fn get(&self, name: &str) -> Result<Option<String>> {
        let Some(facts) = self.facts(name) else { return Ok(None) };
        if let Some((added, value)) = self.reading().get(name)
            && *added == facts.added_at
        {
            return Ok(Some(value.clone()));
        }
        let value = match facts.kept_in.as_str() {
            "keyring" => {
                let Keyring::Usable(store, words) = &self.keyring else {
                    return Err(Error::Invalid(format!(
                        "The {} is kept in the system's keyring, and Demido cannot reach the keyring now. Start Demido from your desktop session, or add the key again.",
                        facts.label
                    )));
                };
                let (store, account) = (store.clone(), self.account(name));
                match apart(move || keyring_core::Entry::new_in_store(&store, SERVICE, &account).and_then(|entry| entry.get_password())) {
                    Ok(value) => value,
                    Err(keyring_core::Error::NoEntry) => return Err(Error::Invalid(format!("The {} is no longer in {words}: it was deleted there. Add it again.", facts.label))),
                    Err(e) => return Err(Error::Invalid(format!("{words} did not give the {}: {e}. If the keyring is locked, unlock it.", facts.label))),
                }
            }
            _ => match self.file_values().remove(name) {
                Some(value) => value,
                None => return Err(Error::Invalid(format!("The {} should be in {}, and it is not there. Add it again.", facts.label, self.file.display()))),
            },
        };
        self.reading().insert(name.to_string(), (facts.added_at, value.clone()));
        Ok(Some(value))
    }

    /// Every kept value that can be read now.
    fn values(&self) -> Vec<String> {
        let names: Vec<String> = self.index().keys.into_iter().map(|k| k.name).collect();
        names.iter().filter_map(|name| self.get(name).ok().flatten()).filter(|v| v.chars().count() >= SHORTEST_LOOKED_FOR).collect()
    }

    /// The text with every kept value taken out.
    pub fn hide_in(&self, text: &str) -> String {
        let mut out = text.to_string();
        for value in self.values() {
            if out.contains(&value) {
                out = out.replace(&value, HIDDEN);
            }
        }
        out
    }

    /// Whether the text holds a kept value.
    pub fn holds_one(&self, text: &str) -> bool {
        self.values().iter().any(|value| text.contains(value.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> crate::testing::TempDir {
        crate::testing::TempDir::new("demido-vault")
    }

    #[test]
    fn a_key_goes_in_and_only_its_last_four_characters_come_out() {
        let dir = home();
        let vault = Vault::in_file(&dir);
        let facts = vault.put("anthropic.a1", "  sk-ant-test-0123456789abcdWXYZ \n").unwrap();
        assert_eq!((facts.name.as_str(), facts.kept_in.as_str(), facts.ends_with.as_str()), ("anthropic.a1", "file", "WXYZ"));
        // What the person and the API see holds no value.
        let shown = serde_json::to_string(&vault.view()).unwrap();
        assert!(!shown.contains("0123456789") && shown.contains("WXYZ"), "{shown}");
        assert_eq!(vault.view().new_keys_go_to, "file");
        // The index file holds no value either; the file of values does, readable only by this user.
        assert!(!std::fs::read_to_string(dir.join("vault.json")).unwrap().contains("0123456789"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in ["vault.json", "vault-keys.json"] {
                assert_eq!(std::fs::metadata(dir.join(file)).unwrap().permissions().mode() & 0o777, 0o600, "{file}");
            }
        }
        // Another Demido on the same folder (the command line) reads the same key.
        assert_eq!(Vault::in_file(&dir).get("anthropic.a1").unwrap().as_deref(), Some("sk-ant-test-0123456789abcdWXYZ"));
        assert_eq!(vault.get("gemini.a1").unwrap(), None);
    }

    #[test]
    fn only_known_keys_are_kept_and_only_what_looks_like_a_key() {
        let home = home();
        let vault = Vault::in_file(&home);
        assert!(matches!(vault.put("my-bank.a1", "0123456789abcdef"), Err(Error::NotFound(_))));
        // A key is named for its provider and its account.
        assert!(matches!(vault.put("gemini", "0123456789abcdef"), Err(Error::NotFound(_))));
        assert!(matches!(vault.put("gemini.a b", "0123456789abcdef"), Err(Error::NotFound(_))));
        assert!(matches!(vault.put("gemini.a1", "short"), Err(Error::Invalid(_))));
        assert!(matches!(vault.put("gemini.a1", "two words here, not a key"), Err(Error::Invalid(_))));
        assert!(vault.view().keys.is_empty());
    }

    #[test]
    fn a_key_replaced_or_removed_is_gone() {
        let dir = home();
        let vault = Vault::in_file(&dir);
        vault.put("openrouter.a1", "sk-or-v1-first-key-aaaa").unwrap();
        vault.put("openrouter.a1", "sk-or-v1-second-key-bbbb").unwrap();
        // Another account's key is another key.
        vault.put("openrouter.b2", "sk-or-v1-other-key-cccc").unwrap();
        assert_eq!(vault.view().keys.len(), 2);
        assert_eq!(vault.get("openrouter.a1").unwrap().as_deref(), Some("sk-or-v1-second-key-bbbb"));
        let gone = vault.remove("openrouter.a1").unwrap();
        assert_eq!(gone.ends_with, "bbbb");
        assert_eq!(vault.get("openrouter.a1").unwrap(), None);
        assert_eq!(vault.get("openrouter.b2").unwrap().as_deref(), Some("sk-or-v1-other-key-cccc"));
        assert!(!std::fs::read_to_string(dir.join("vault-keys.json")).unwrap().contains("second-key"));
        assert!(matches!(vault.remove("openrouter.a1"), Err(Error::NotFound(_))));
    }

    #[test]
    fn a_kept_value_is_taken_out_of_text_and_found_in_it() {
        let home = home();
        let vault = Vault::in_file(&home);
        vault.put("gemini.a1", "AIzaSyTEST-not-a-real-key-42").unwrap();
        let printed = "export GEMINI_API_KEY=AIzaSyTEST-not-a-real-key-42\nok";
        assert_eq!(vault.hide_in(printed), format!("export GEMINI_API_KEY={HIDDEN}\nok"));
        assert!(vault.holds_one("https://example.com/?k=AIzaSyTEST-not-a-real-key-42"));
        assert!(!vault.holds_one("https://example.com/?k=AIzaSy"));
        assert_eq!(vault.hide_in("nothing here"), "nothing here");
    }
}
