//! Secret storage (docs/06 §4): API keys live in the OS keychain, never in `getmasters.db` or
//! plaintext config. Settings reference a secret by name; the value is resolved at use time.
//!
//! Two backends behind one trait:
//! - [`KeyringStore`] — the OS keychain (Windows Credential Manager / macOS Keychain / Linux
//!   Secret Service), the default on a real desktop.
//! - [`MemoryStore`] — an in-process map used by tests and as a **graceful fallback** when no
//!   OS keychain is available (e.g. a headless server with no Secret Service). The daemon logs
//!   when it falls back so the degraded mode is visible.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The keychain service name Masters stores entries under.
const SERVICE: &str = "dev.shushuo.getmasters";

/// A named secret store.
pub trait SecretStore: Send + Sync {
    /// Fetch a secret by name, if present.
    fn get(&self, name: &str) -> Option<String>;
    /// Store (or replace) a secret.
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    /// Remove a secret (no error if absent).
    fn delete(&self, name: &str) -> Result<(), String>;
    /// Whether a secret is set (never returns the value).
    fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

/// OS-keychain-backed secret store.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Option<String> {
        Self::entry(name).ok()?.get_password().ok()
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        Self::entry(name)?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// In-process secret store (tests + headless fallback).
#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Option<String> {
        self.inner.lock().unwrap().get(name).cloned()
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        self.inner
            .lock()
            .unwrap()
            .insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        self.inner.lock().unwrap().remove(name);
        Ok(())
    }
}

/// Build the default secret store: the OS keychain if it works, otherwise an in-memory store
/// (logged). A round-trip probe avoids surprising failures later.
pub fn default_secret_store() -> Arc<dyn SecretStore> {
    let keyring = KeyringStore;
    match keyring
        .set("__getmasters_probe", "1")
        .and_then(|_| keyring.delete("__getmasters_probe"))
    {
        Ok(()) => Arc::new(KeyringStore),
        Err(e) => {
            tracing::warn!(error = %e, "OS keychain unavailable; using in-memory secret store (secrets will not persist)");
            Arc::new(MemoryStore::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::new();
        assert!(!s.has("k"));
        s.set("k", "v").unwrap();
        assert_eq!(s.get("k").as_deref(), Some("v"));
        assert!(s.has("k"));
        s.delete("k").unwrap();
        assert!(!s.has("k"));
    }
}
