use std::{ffi::OsStr, fs, path::PathBuf, process::Command};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{config::PathsConfig, paths};

const BUNDLED_TERMINAL_TOML: &str = include_str!("../assets/apps/terminal.toml");

#[derive(Debug, Error)]
pub enum AppError {
    #[error("app profile name is invalid")]
    InvalidName,
    #[error("app profile {0} was not found")]
    NotFound(String),
    #[error("app profile {0} already exists")]
    AlreadyExists(String),
    #[error("invalid app profile: {0}")]
    Validation(String),
    #[error("app profile at {path} is invalid: {source}")]
    InvalidProfile {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to read app profile at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write app profile at {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to launch editor {editor}: {source}")]
    EditorLaunch {
        editor: String,
        source: std::io::Error,
    },
    #[error("editor {editor} exited with status {status}")]
    EditorFailed { editor: String, status: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct AppProfileCleanup {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct AppProfileInjection {
    pub prefer_clipboard_only: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppProfile {
    pub name: String,
    pub default_style: Option<String>,
    pub match_process: Vec<String>,
    pub match_app_id: Vec<String>,
    pub prompt: Option<String>,
    pub cleanup: AppProfileCleanup,
    pub injection: AppProfileInjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProfileSummary {
    pub name: String,
    pub default_style: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppValidationIssue {
    pub app: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDetectionReport {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub backend: String,
    pub matched_profile: Option<String>,
}

pub fn ensure_default_app_profiles(paths: &PathsConfig) -> Result<(), AppError> {
    let apps_dir = paths::apps_dir(paths);
    fs::create_dir_all(&apps_dir).map_err(|source| AppError::Write {
        path: apps_dir.clone(),
        source,
    })?;
    write_if_missing(&apps_dir.join("terminal.toml"), BUNDLED_TERMINAL_TOML)?;
    Ok(())
}

pub fn list_app_profiles(paths: &PathsConfig) -> Result<Vec<AppProfileSummary>, AppError> {
    let apps_dir = paths::apps_dir(paths);
    if !apps_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for entry in fs::read_dir(&apps_dir).map_err(|source| AppError::Read {
        path: apps_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| AppError::Read {
            path: apps_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let profile = match read_profile(paths, stem) {
            Ok(profile) => profile,
            Err(error) => {
                tracing::warn!(
                    app = stem,
                    error = %error,
                    "skipping unreadable app profile file"
                );
                continue;
            }
        };
        profiles.push(AppProfileSummary {
            name: profile.name,
            default_style: profile.default_style,
        });
    }
    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(profiles)
}

pub fn load_app_profile(paths: &PathsConfig, name: &str) -> Result<AppProfile, AppError> {
    validate_profile_name(name)?;
    read_profile(paths, name)
}

pub fn edit_app_profile(paths: &PathsConfig, name: &str) -> Result<PathBuf, AppError> {
    let path = profile_path(paths, name)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|source| AppError::EditorLaunch {
            editor: editor.clone(),
            source,
        })?;
    if !status.success() {
        return Err(AppError::EditorFailed {
            editor,
            status: status.code().unwrap_or(1),
        });
    }
    validate_app_profile(paths, name)?;
    Ok(path)
}

pub fn validate_app_profile(paths: &PathsConfig, name: &str) -> Result<(), AppError> {
    validate_profile_name(name)?;
    let profile = read_profile(paths, name)?;
    validate_profile_contents(&profile)
}

#[must_use]
pub fn validate_installed_app_profiles(paths: &PathsConfig) -> Vec<AppValidationIssue> {
    let apps_dir = paths::apps_dir(paths);
    if !apps_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&apps_dir) else {
        return vec![AppValidationIssue {
            app: "*".into(),
            message: "apps directory is unreadable".into(),
        }];
    };
    let mut issues = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Err(error) = validate_app_profile(paths, stem) {
            issues.push(AppValidationIssue {
                app: stem.into(),
                message: error.to_string(),
            });
        }
    }
    issues
}

#[must_use]
pub fn match_app_profile(
    paths: &PathsConfig,
    app_id: Option<&str>,
    title: Option<&str>,
) -> Option<AppProfile> {
    let profiles = list_app_profiles(paths).ok()?;
    for summary in profiles {
        let profile = load_app_profile(paths, &summary.name).ok()?;
        if profile_matches(&profile, app_id, title) {
            return Some(profile);
        }
    }
    None
}

pub fn detect_app_profile(
    paths: &PathsConfig,
    backend: &str,
    app_id: Option<&str>,
    title: Option<&str>,
) -> AppDetectionReport {
    let matched = match_app_profile(paths, app_id, title);
    AppDetectionReport {
        app_id: app_id.map(ToOwned::to_owned),
        title: title.map(ToOwned::to_owned),
        backend: backend.into(),
        matched_profile: matched.map(|profile| profile.name),
    }
}

fn profile_matches(profile: &AppProfile, app_id: Option<&str>, title: Option<&str>) -> bool {
    let haystacks = [app_id, title]
        .into_iter()
        .flatten()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if haystacks.is_empty() {
        return false;
    }
    if profile.match_app_id.iter().any(|needle| {
        haystacks.iter().any(|haystack| {
            haystack == &needle.to_ascii_lowercase()
                || haystack.contains(&needle.to_ascii_lowercase())
        })
    }) {
        return true;
    }
    profile.match_process.iter().any(|needle| {
        let needle = needle.to_ascii_lowercase();
        haystacks.iter().any(|haystack| haystack.contains(&needle))
    })
}

fn validate_profile_contents(profile: &AppProfile) -> Result<(), AppError> {
    if profile.name.trim().is_empty() {
        return Err(AppError::Validation("name is required".into()));
    }
    if profile.match_process.is_empty() && profile.match_app_id.is_empty() {
        return Err(AppError::Validation(
            "at least one match_process or match_app_id entry is required".into(),
        ));
    }
    Ok(())
}

fn read_profile(paths: &PathsConfig, file_stem: &str) -> Result<AppProfile, AppError> {
    validate_profile_name(file_stem)?;
    let path = profile_path(paths, file_stem)?;
    let text = fs::read_to_string(&path).map_err(|source| AppError::Read {
        path: path.clone(),
        source,
    })?;
    let profile: AppProfile =
        toml::from_str(&text).map_err(|source| AppError::InvalidProfile { path, source })?;
    if profile.name != file_stem {
        return Err(AppError::NotFound(format!(
            "{file_stem} (profile name is {})",
            profile.name
        )));
    }
    validate_profile_contents(&profile)?;
    Ok(profile)
}

fn profile_path(paths: &PathsConfig, name: &str) -> Result<PathBuf, AppError> {
    validate_profile_name(name)?;
    let path = paths::apps_dir(paths).join(format!("{name}.toml"));
    if !path.is_file() {
        return Err(AppError::NotFound(name.into()));
    }
    Ok(path)
}

fn validate_profile_name(name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err(AppError::InvalidName);
    }
    Ok(())
}

fn write_if_missing(path: &PathBuf, contents: &str) -> Result<(), AppError> {
    if path.is_file() {
        return Ok(());
    }
    fs::write(path, contents).map_err(|source| AppError::Write {
        path: path.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn temp_paths(base: &Path) -> PathsConfig {
        PathsConfig {
            config_dir: base.join("config").display().to_string(),
            model_dir: base.join("models").display().to_string(),
            runtime_dir: base.join("runtime").display().to_string(),
        }
    }

    #[test]
    fn matches_terminal_process_names() {
        let base = std::env::temp_dir().join(format!("skald-apps-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        ensure_default_app_profiles(&paths).unwrap();
        let matched = match_app_profile(&paths, Some("kitty"), Some("shell"));
        assert_eq!(
            matched.as_ref().map(|profile| profile.name.as_str()),
            Some("terminal")
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn list_app_profiles_skips_corrupt_file_returns_valid() {
        let base = std::env::temp_dir().join(format!("skald-apps-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        ensure_default_app_profiles(&paths).unwrap();
        let corrupt_path = paths::apps_dir(&paths).join("bad.toml");
        fs::write(&corrupt_path, "not valid toml [[[").unwrap();
        let profiles = list_app_profiles(&paths).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "terminal");
        let _ = fs::remove_dir_all(&base);
    }
}
