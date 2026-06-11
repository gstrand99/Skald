use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{config::PathsConfig, paths};

pub const DEFAULT_STYLE_NAME: &str = "default";

const BUNDLED_DEFAULT_TOML: &str = include_str!("../assets/styles/default.toml");
const BUNDLED_DEFAULT_PROMPT: &str = include_str!("../assets/styles/default.md");

const NEW_STYLE_PROMPT_TEMPLATE: &str = "\
Rewrite dictated speech into clean text ready to paste.
Preserve the user's meaning.
Do not add facts.
Return only the final text.";

#[derive(Debug, Error)]
pub enum StyleError {
    #[error("style name is invalid")]
    InvalidName,
    #[error("style {0} was not found")]
    NotFound(String),
    #[error("style {0} already exists")]
    AlreadyExists(String),
    #[error("style metadata at {path} is invalid: {source}")]
    InvalidMetadata {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to read style file at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write style file at {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("style prompt at {0} is empty")]
    EmptyPrompt(PathBuf),
    #[error("invalid style {style}: {message}")]
    InvalidPromptFile { style: String, message: String },
    #[error("failed to launch editor {editor}: {source}")]
    EditorLaunch {
        editor: String,
        source: std::io::Error,
    },
    #[error("editor {editor} exited with status {status}")]
    EditorFailed { editor: String, status: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StyleMetadata {
    pub name: String,
    pub description: String,
    pub prompt_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleSummary {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleValidationIssue {
    pub style: String,
    pub message: String,
}

pub fn ensure_default_style_files(paths: &PathsConfig) -> Result<(), StyleError> {
    let styles_dir = paths::styles_dir(paths);
    fs::create_dir_all(&styles_dir).map_err(|source| StyleError::Write {
        path: styles_dir.clone(),
        source,
    })?;
    write_if_missing(&styles_dir.join("default.toml"), BUNDLED_DEFAULT_TOML)?;
    write_if_missing(&styles_dir.join("default.md"), BUNDLED_DEFAULT_PROMPT)?;
    Ok(())
}

#[must_use]
pub fn resolve_style_name(style_override: Option<&str>, default_style: &str) -> String {
    match style_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => name.to_owned(),
        None => default_style.to_owned(),
    }
}

pub fn list_styles(paths: &PathsConfig) -> Result<Vec<StyleSummary>, StyleError> {
    let styles_dir = paths::styles_dir(paths);
    if !styles_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut styles = Vec::new();
    for entry in fs::read_dir(&styles_dir).map_err(|source| StyleError::Read {
        path: styles_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StyleError::Read {
            path: styles_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        let Some(file_stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let metadata = match read_metadata(paths, file_stem) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    style = file_stem,
                    error = %error,
                    "skipping unreadable style metadata file"
                );
                continue;
            }
        };
        styles.push(StyleSummary {
            name: metadata.name,
            description: metadata.description,
        });
    }
    styles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(styles)
}

pub fn create_style(
    paths: &PathsConfig,
    name: &str,
    description: Option<&str>,
) -> Result<(), StyleError> {
    validate_style_name(name)?;
    let styles_dir = paths::styles_dir(paths);
    fs::create_dir_all(&styles_dir).map_err(|source| StyleError::Write {
        path: styles_dir.clone(),
        source,
    })?;
    let metadata_path = styles_dir.join(format!("{name}.toml"));
    let prompt_path = styles_dir.join(format!("{name}.md"));
    if metadata_path.exists() || prompt_path.exists() {
        return Err(StyleError::AlreadyExists(name.into()));
    }
    let metadata = StyleMetadata {
        name: name.into(),
        description: description
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Custom cleanup style.")
            .into(),
        prompt_file: format!("{name}.md"),
    };
    write_metadata(&metadata_path, &metadata)?;
    fs::write(&prompt_path, NEW_STYLE_PROMPT_TEMPLATE).map_err(|source| StyleError::Write {
        path: prompt_path,
        source,
    })?;
    Ok(())
}

pub fn edit_style(paths: &PathsConfig, name: &str) -> Result<PathBuf, StyleError> {
    let prompt_path = prompt_path_for_style(paths, name)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let status = Command::new(&editor)
        .arg(&prompt_path)
        .status()
        .map_err(|source| StyleError::EditorLaunch {
            editor: editor.clone(),
            source,
        })?;
    if !status.success() {
        return Err(StyleError::EditorFailed {
            editor,
            status: status.code().unwrap_or(1),
        });
    }
    validate_style(paths, name)?;
    Ok(prompt_path)
}

pub fn validate_style(paths: &PathsConfig, name: &str) -> Result<(), StyleError> {
    validate_style_name(name)?;
    let _ = load_style_prompt(paths, name)?;
    Ok(())
}

#[must_use]
pub fn validate_installed_styles(paths: &PathsConfig) -> Vec<StyleValidationIssue> {
    let styles_dir = paths::styles_dir(paths);
    if !styles_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&styles_dir) else {
        return vec![StyleValidationIssue {
            style: "*".into(),
            message: "styles directory is unreadable".into(),
        }];
    };
    let mut issues = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        let Some(file_stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Err(error) = validate_style(paths, file_stem) {
            issues.push(StyleValidationIssue {
                style: file_stem.into(),
                message: error.to_string(),
            });
        }
    }
    issues
}

pub fn load_style_prompt(paths: &PathsConfig, style_name: &str) -> Result<String, StyleError> {
    validate_style_name(style_name)?;
    let styles_dir = paths::styles_dir(paths);
    let metadata_path = styles_dir.join(format!("{style_name}.toml"));
    if !metadata_path.is_file() {
        if style_name == DEFAULT_STYLE_NAME {
            ensure_default_style_files(paths)?;
            return load_style_prompt(paths, style_name);
        }
        return Err(StyleError::NotFound(style_name.into()));
    }
    let metadata = read_metadata(paths, style_name)?;
    let prompt_path = styles_dir.join(&metadata.prompt_file);
    if !prompt_path.is_file() {
        return Err(StyleError::Read {
            path: prompt_path,
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "prompt file missing"),
        });
    }
    let prompt = fs::read_to_string(&prompt_path).map_err(|source| StyleError::Read {
        path: prompt_path.clone(),
        source,
    })?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(StyleError::EmptyPrompt(prompt_path));
    }
    Ok(prompt.to_owned())
}

fn read_metadata(paths: &PathsConfig, style_name: &str) -> Result<StyleMetadata, StyleError> {
    let metadata_path = paths::styles_dir(paths).join(format!("{style_name}.toml"));
    let metadata_text = fs::read_to_string(&metadata_path).map_err(|source| StyleError::Read {
        path: metadata_path.clone(),
        source,
    })?;
    let metadata: StyleMetadata =
        toml::from_str(&metadata_text).map_err(|source| StyleError::InvalidMetadata {
            path: metadata_path,
            source,
        })?;
    if metadata.name != style_name {
        return Err(StyleError::NotFound(format!(
            "{style_name} (metadata name is {})",
            metadata.name
        )));
    }
    validate_metadata_contents(style_name, &metadata)?;
    Ok(metadata)
}

fn validate_metadata_contents(
    style_name: &str,
    metadata: &StyleMetadata,
) -> Result<(), StyleError> {
    if metadata.prompt_file.trim().is_empty() {
        return Err(StyleError::InvalidPromptFile {
            style: style_name.into(),
            message: "prompt_file cannot be empty".into(),
        });
    }
    if metadata.prompt_file.contains('/')
        || metadata.prompt_file.contains('\\')
        || metadata.prompt_file.contains("..")
    {
        return Err(StyleError::InvalidPromptFile {
            style: style_name.into(),
            message: "prompt_file must be a file name in the styles directory".into(),
        });
    }
    Ok(())
}

fn prompt_path_for_style(paths: &PathsConfig, style_name: &str) -> Result<PathBuf, StyleError> {
    let metadata = read_metadata(paths, style_name)?;
    Ok(paths::styles_dir(paths).join(metadata.prompt_file))
}

fn write_metadata(path: &Path, metadata: &StyleMetadata) -> Result<(), StyleError> {
    let text = toml::to_string_pretty(metadata).map_err(|error| StyleError::Write {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
    })?;
    fs::write(path, text).map_err(|source| StyleError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_style_name(name: &str) -> Result<(), StyleError> {
    let name = name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err(StyleError::InvalidName);
    }
    Ok(())
}

fn write_if_missing(path: &PathBuf, contents: &str) -> Result<(), StyleError> {
    if path.is_file() {
        return Ok(());
    }
    fs::write(path, contents).map_err(|source| StyleError::Write {
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
    fn loads_default_style_prompt_from_scaffolded_files() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        ensure_default_style_files(&paths).unwrap();
        let prompt = load_style_prompt(&paths, DEFAULT_STYLE_NAME).unwrap();
        assert!(prompt.contains("cleanup engine for VoxLine"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_style_prefers_cli_override() {
        assert_eq!(
            resolve_style_name(Some("professional"), "default"),
            "professional"
        );
        assert_eq!(resolve_style_name(None, "default"), "default");
    }

    #[test]
    fn create_and_list_styles() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        create_style(&paths, "professional", Some("Professional prose.")).unwrap();
        let styles = list_styles(&paths).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "professional");
        validate_style(&paths, "professional").unwrap();
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_duplicate_style_creation() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        create_style(&paths, "notes", None).unwrap();
        assert!(matches!(
            create_style(&paths, "notes", None),
            Err(StyleError::AlreadyExists(_))
        ));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn list_styles_skips_corrupt_file_returns_valid() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        create_style(&paths, "good", Some("Valid style.")).unwrap();
        let corrupt_path = paths::styles_dir(&paths).join("bad.toml");
        fs::write(&corrupt_path, "not valid toml [[[").unwrap();
        let styles = list_styles(&paths).unwrap();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "good");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_invalid_prompt_file_in_metadata() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        fs::create_dir_all(paths::styles_dir(&paths)).unwrap();
        let metadata_path = paths::styles_dir(&paths).join("escape.toml");
        fs::write(
            &metadata_path,
            r#"
name = "escape"
description = "Escape attempt."
prompt_file = "../escape.md"
"#,
        )
        .unwrap();
        let issues = validate_installed_styles(&paths);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].style, "escape");
        assert!(issues[0].message.contains("prompt_file"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn load_style_prompt_rejects_escape_prompt_file() {
        let base = std::env::temp_dir().join(format!("voxline-styles-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        fs::create_dir_all(paths::styles_dir(&paths)).unwrap();
        let metadata_path = paths::styles_dir(&paths).join("escape.toml");
        fs::write(
            &metadata_path,
            r#"
name = "escape"
description = "Escape attempt."
prompt_file = "../escape.md"
"#,
        )
        .unwrap();
        assert!(matches!(
            load_style_prompt(&paths, "escape"),
            Err(StyleError::InvalidPromptFile { .. })
        ));
        let _ = fs::remove_dir_all(&base);
    }
}
