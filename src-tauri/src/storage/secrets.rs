//! Gemini API key in the Windows Credential Manager (via `keyring`).
//! The key is never logged, serialized, or written to any file.

use std::fmt;

use super::StorageError;

const SERVICE: &str = "NiteanStudio";
const USER: &str = "gemini_api_key";

/// The API key. `Debug` is redacted so it cannot leak through logs or error dumps.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// Validates a pasted key (trimmed, printable ASCII, sane length).
    pub fn parse(raw: &str) -> Result<Self, StorageError> {
        let key = raw.trim();
        let ok_len = (20..=200).contains(&key.len());
        let ok_chars = key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
        if ok_len && ok_chars {
            Ok(Self(key.to_owned()))
        } else {
            Err(StorageError::InvalidKeyFormat)
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

fn entry() -> Result<keyring::Entry, StorageError> {
    keyring::Entry::new(SERVICE, USER).map_err(keyring_err)
}

// keyring errors never contain the secret, only the platform failure.
fn keyring_err(e: keyring::Error) -> StorageError {
    StorageError::Keyring(e.to_string())
}

pub fn set_api_key(key: &ApiKey) -> Result<(), StorageError> {
    entry()?.set_password(key.expose()).map_err(keyring_err)
}

pub fn get_api_key() -> Result<Option<ApiKey>, StorageError> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(ApiKey(k))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keyring_err(e)),
    }
}

pub fn clear_api_key() -> Result<(), StorageError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(keyring_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let key = ApiKey::parse("AIzaSyTESTTESTTESTTESTTESTTEST123").unwrap();
        let shown = format!("{key:?}");
        assert!(!shown.contains("AIza"));
    }

    #[test]
    fn parse_trims_and_validates() {
        assert!(ApiKey::parse("  AIzaSyTESTTESTTESTTESTTESTTEST123\n").is_ok());
        assert!(ApiKey::parse("short").is_err());
        assert!(ApiKey::parse("AIzaSy TEST TEST TEST TEST TEST").is_err());
    }
}
