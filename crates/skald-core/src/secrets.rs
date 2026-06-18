use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::paths;

pub const OPENROUTER_SECRET_NAME: &str = "openrouter";
const KEYRING_SERVICE: &str = "skald";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret store is unavailable")]
    StoreUnavailable,
    #[error("secret was not found")]
    NotFound,
    #[error("insecure file fallback is disabled")]
    InsecureFileDisabled,
    #[error("failed to read secrets file at {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write secrets file at {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("secrets file at {0} must have mode 0600")]
    InsecurePermissions(PathBuf),
    #[error("failed to access keyring: {0}")]
    Keyring(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SecretsConfig {
    pub mode: String,
    pub openrouter_env_var: String,
    pub allow_insecure_file_fallback: bool,
    pub insecure_file_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SecretStatus {
    pub keyring_available: bool,
    pub keyring_configured: bool,
    pub env_configured: bool,
    pub insecure_file_enabled: bool,
    pub insecure_file_configured: bool,
    pub openrouter_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretsFile {
    #[serde(default)]
    openrouter: Option<String>,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            openrouter_env_var: "OPENROUTER_API_KEY".into(),
            allow_insecure_file_fallback: false,
            insecure_file_path: "~/.config/skald/secrets.toml".into(),
        }
    }
}

#[must_use]
pub fn secret_status(config: &SecretsConfig) -> SecretStatus {
    let keyring_available = keyring_entry().is_ok();
    let keyring_configured = lookup_keyring().is_ok();
    let env_configured = lookup_env(config).is_some();
    let insecure_file_enabled = config.allow_insecure_file_fallback;
    let insecure_file_configured = insecure_file_enabled
        && read_secrets_file(&paths::expand_home(&config.insecure_file_path))
            .ok()
            .and_then(|file| file.openrouter)
            .is_some();
    SecretStatus {
        keyring_available,
        keyring_configured,
        env_configured,
        insecure_file_enabled,
        insecure_file_configured,
        openrouter_configured: lookup_openrouter_key(config).is_ok(),
    }
}

pub fn lookup_openrouter_key(config: &SecretsConfig) -> Result<String, SecretError> {
    if let Ok(key) = lookup_keyring() {
        return Ok(key);
    }
    if let Some(key) = lookup_env(config) {
        return Ok(key);
    }
    if config.allow_insecure_file_fallback {
        let path = paths::expand_home(&config.insecure_file_path);
        let file = read_secrets_file(&path)?;
        if let Some(key) = file.openrouter.filter(|value| !value.trim().is_empty()) {
            return Ok(key);
        }
    }
    Err(SecretError::NotFound)
}

pub fn set_openrouter_key(_config: &SecretsConfig, key: &str) -> Result<(), SecretError> {
    let entry = keyring_entry().map_err(|error| SecretError::Keyring(error.to_string()))?;
    entry
        .set_password(key)
        .map_err(|error| SecretError::Keyring(error.to_string()))
}

pub fn clear_openrouter_key() -> Result<(), SecretError> {
    let entry = keyring_entry().map_err(|error| SecretError::Keyring(error.to_string()))?;
    entry
        .delete_credential()
        .map_err(|error| SecretError::Keyring(error.to_string()))
}

fn lookup_keyring() -> Result<String, SecretError> {
    let entry = keyring_entry().map_err(|error| SecretError::Keyring(error.to_string()))?;
    entry.get_password().map_err(|error| match error {
        keyring::Error::NoEntry => SecretError::NotFound,
        other => SecretError::Keyring(other.to_string()),
    })
}

fn lookup_env(config: &SecretsConfig) -> Option<String> {
    std::env::var(&config.openrouter_env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn keyring_entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, OPENROUTER_SECRET_NAME)
}

fn read_secrets_file(path: &PathBuf) -> Result<SecretsFile, SecretError> {
    if !path.is_file() {
        return Ok(SecretsFile { openrouter: None });
    }
    verify_secret_file_mode(path)?;
    let text = fs::read_to_string(path).map_err(|source| SecretError::ReadFile {
        path: path.clone(),
        source,
    })?;
    toml::from_str(&text).map_err(|_| SecretError::ReadFile {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid secrets file"),
    })
}

fn verify_secret_file_mode(path: &PathBuf) -> Result<(), SecretError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map_err(|source| SecretError::ReadFile {
                path: path.clone(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            return Err(SecretError::InsecurePermissions(path.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_secrets_config_uses_env_var_name() {
        let config = SecretsConfig::default();
        assert_eq!(config.openrouter_env_var, "OPENROUTER_API_KEY");
        assert!(!config.allow_insecure_file_fallback);
    }

    #[test]
    fn rejects_unknown_secrets_config_keys() {
        let err = toml::from_str::<SecretsConfig>("unknown_key = true").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
