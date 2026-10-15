//! Secrets in the Windows Credential Manager (via `keyring`), service `NiteanStudio`:
//! the Gemini API key (`gemini_api_key`) and the Cloudflare API token (`cloudflare_api_token`).
//! They are never logged, serialized, or written to any file.

use std::fmt;

use super::StorageError;

const SERVICE: &str = "NiteanStudio";
const USER: &str = "gemini_api_key";
const CLOUDFLARE_USER: &str = "cloudflare_api_token";

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

/// Cloudflare API token (Workers AI). `Debug` is redacted.
#[derive(Clone)]
pub struct CloudflareToken(String);

impl CloudflareToken {
    /// Cloudflare tokens are 40 URL-safe characters today; accept a sane range.
    pub fn parse(raw: &str) -> Result<Self, StorageError> {
        let token = raw.trim();
        let ok_len = (20..=200).contains(&token.len());
        let ok_chars = token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok_len && ok_chars {
            Ok(Self(token.to_owned()))
        } else {
            Err(StorageError::InvalidSetting(
                "the Cloudflare API token looks invalid — paste it exactly as Cloudflare shows it"
                    .into(),
            ))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CloudflareToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CloudflareToken(<redacted>)")
    }
}

fn cloudflare_entry() -> Result<keyring::Entry, StorageError> {
    keyring::Entry::new(SERVICE, CLOUDFLARE_USER).map_err(keyring_err)
}

pub fn set_cloudflare_token(token: &CloudflareToken) -> Result<(), StorageError> {
    cloudflare_entry()?
        .set_password(token.expose())
        .map_err(keyring_err)
}

pub fn get_cloudflare_token() -> Result<Option<CloudflareToken>, StorageError> {
    match cloudflare_entry()?.get_password() {
        Ok(t) => Ok(Some(CloudflareToken(t))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keyring_err(e)),
    }
}

pub fn clear_cloudflare_token() -> Result<(), StorageError> {
    match cloudflare_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(keyring_err(e)),
    }
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
    fn cloudflare_token_is_validated_and_redacted() {
        let t = CloudflareToken::parse(" abcdefghijklmnopqrstuvwxyzABCDEFGHIJ_-12 ").unwrap();
        assert!(!format!("{t:?}").contains("abcdef"));
        assert!(CloudflareToken::parse("short").is_err());
        assert!(CloudflareToken::parse("has spaces in the middle of it ok").is_err());
    }

    #[test]
    fn parse_trims_and_validates() {
        assert!(ApiKey::parse("  AIzaSyTESTTESTTESTTESTTESTTEST123\n").is_ok());
        assert!(ApiKey::parse("short").is_err());
        assert!(ApiKey::parse("AIzaSy TEST TEST TEST TEST TEST").is_err());
    }
}
