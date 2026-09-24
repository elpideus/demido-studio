//! API keys and session cookies, kept in the operating system's credential store (Windows
//! Credential Manager, macOS Keychain, the Secret Service on Linux). Nothing secret is written to
//! Demido's own files; if the credential store is unavailable, secrets live in memory for the
//! session only and the UI says so.

use std::collections::HashMap;

use parking_lot::Mutex;

const SERVICE: &str = "Demido Studio";

#[derive(Default)]
pub struct Secrets {
    /// Fallback when the OS store refuses an operation.
    memory: Mutex<HashMap<String, String>>,
}

impl Secrets {
    pub fn get(&self, key: &str) -> Option<String> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
            Ok(v) => Some(v),
            Err(keyring::Error::NoEntry) => self.memory.lock().get(key).cloned(),
            Err(err) => {
                tracing::warn!("credential store read failed for {key}: {err}");
                self.memory.lock().get(key).cloned()
            }
        }
    }

    /// Stores a secret. Returns false when it could only be kept in memory.
    pub fn set(&self, key: &str, value: &str) -> bool {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.set_password(value)) {
            Ok(()) => {
                self.memory.lock().remove(key);
                true
            }
            Err(err) => {
                tracing::warn!("credential store write failed for {key}: {err}");
                self.memory.lock().insert(key.to_string(), value.to_string());
                false
            }
        }
    }

    pub fn delete(&self, key: &str) {
        self.memory.lock().remove(key);
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(err) => tracing::warn!("credential store delete failed for {key}: {err}"),
        }
    }
}

pub fn provider_key(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

pub const HF_TOKEN: &str = "huggingface:token";
pub const TRADINGVIEW_SESSION: &str = "tradingview:session";
