//! macOS Keychain storage for Resend API keys.
//!
//! API keys are stored as generic passwords under service
//! `ai.paperfoot.email-cli` with the profile name as the account.
//!
//! A SQLite row with `api_key = KEYCHAIN_SENTINEL` means the real key
//! is in the Keychain. Any other string means legacy SQLite storage;
//! `client_for_profile` migrates those on first use.

/// Sentinel written to `profiles.api_key` when the real key lives in
/// the Keychain. Kept ASCII and distinctive so it never collides with
/// a real Resend key (which starts with `re_`).
pub const KEYCHAIN_SENTINEL: &str = "__keychain__";

#[cfg(target_os = "macos")]
const SERVICE: &str = "ai.paperfoot.email-cli";

#[cfg(target_os = "macos")]
mod mac {
    use anyhow::{Context, Result};
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    use super::SERVICE;

    pub fn store(profile_name: &str, api_key: &str) -> Result<()> {
        set_generic_password(SERVICE, profile_name, api_key.as_bytes())
            .with_context(|| format!("keychain: store key for profile {profile_name}"))
    }

    pub fn load(profile_name: &str) -> Result<String> {
        let bytes = get_generic_password(SERVICE, profile_name)
            .with_context(|| format!("keychain: load key for profile {profile_name}"))?;
        String::from_utf8(bytes).context("keychain: stored key is not valid UTF-8")
    }

    #[allow(dead_code)]
    pub fn delete(profile_name: &str) -> Result<()> {
        match delete_generic_password(SERVICE, profile_name) {
            Ok(()) => Ok(()),
            // Not-found is fine; nothing to clean up.
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(e).with_context(|| {
                format!("keychain: delete key for profile {profile_name}")
            }),
        }
    }

    pub fn is_available() -> bool {
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use anyhow::{Result, bail};

    pub fn store(_profile_name: &str, _api_key: &str) -> Result<()> {
        bail!("Keychain storage is macOS only; use --api-key-env or --api-key-file instead")
    }

    pub fn load(_profile_name: &str) -> Result<String> {
        bail!("Keychain storage is macOS only")
    }

    pub fn delete(_profile_name: &str) -> Result<()> {
        Ok(())
    }

    pub fn is_available() -> bool {
        false
    }
}

#[allow(unused_imports)]
pub use mac::{delete, is_available, load, store};
