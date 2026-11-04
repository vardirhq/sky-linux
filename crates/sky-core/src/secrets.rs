//! Secret storage. Sky Linux keeps exactly one secret of its own: the
//! password that encrypts its private `rclone.conf` (which contains the OAuth
//! tokens). That password lives in the Linux Secret Service (GNOME Keyring,
//! KWallet, KeePassXC…), never on disk in plain text.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{CoreError, Result};

pub const SERVICE: &str = "sky-linux";
pub const RCLONE_CONFIG_PASSWORD: &str = "rclone-config-password";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;

    /// Return the secret, generating and storing a random one if missing.
    fn get_or_create(&self, key: &str) -> Result<String> {
        if let Some(v) = self.get(key)? {
            return Ok(v);
        }
        use rand::distr::{Alphanumeric, SampleString};
        let v = Alphanumeric.sample_string(&mut rand::rng(), 48);
        self.set(key, &v)?;
        Ok(v)
    }
}

/// Process-local store for tests and for systems without a Secret Service
/// (secrets are then lost on restart and rclone.conf stays unencrypted).
#[derive(Default)]
pub struct MemorySecretStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(feature = "keyring")]
pub struct KeyringSecretStore;

#[cfg(feature = "keyring")]
impl KeyringSecretStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(|e| CoreError::Secrets(e.to_string()))
    }

    /// Probe whether a Secret Service is reachable on the session bus.
    pub fn available() -> bool {
        Self::entry("probe")
            .map(|e| {
                !matches!(
                    e.get_password(),
                    Err(keyring::Error::PlatformFailure(_))
                        | Err(keyring::Error::NoStorageAccess(_))
                )
            })
            .unwrap_or(false)
    }
}

#[cfg(feature = "keyring")]
impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CoreError::Secrets(e.to_string())),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| CoreError::Secrets(e.to_string()))
    }
    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CoreError::Secrets(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_is_stable() {
        let s = MemorySecretStore::default();
        let a = s.get_or_create("k").unwrap();
        assert_eq!(a.len(), 48);
        assert_eq!(s.get_or_create("k").unwrap(), a);
        s.delete("k").unwrap();
        assert_eq!(s.get("k").unwrap(), None);
    }
}
