use std::{ffi::OsStr, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{config::PathsConfig, paths};

pub const INSERT_SNIPPET_TYPE: &str = "insert";

#[derive(Deserialize)]
struct SnippetTypeProbe {
    #[serde(rename = "type")]
    snippet_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetKind {
    Insert,
    Template,
}

const NEW_SNIPPET_CONTENT_TEMPLATE: &str = "Replace this text with your snippet content.";

#[derive(Debug, Error)]
pub enum SnippetError {
    #[error("snippet name is invalid")]
    InvalidName,
    #[error("snippet {0} was not found")]
    NotFound(String),
    #[error("snippet {0} already exists")]
    AlreadyExists(String),
    #[error("invalid snippet: {0}")]
    Validation(String),
    #[error("snippet metadata at {path} is invalid: {source}")]
    InvalidMetadata {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("failed to read snippet file at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write snippet file at {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("snippet content at {0} is empty")]
    EmptyContent(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsertSnippetMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub snippet_type: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub content_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetSummary {
    pub name: String,
    pub kind: SnippetKind,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetValidationIssue {
    pub snippet: String,
    pub message: String,
}

pub fn ensure_snippets_dir(paths: &PathsConfig) -> Result<(), SnippetError> {
    let snippets_dir = paths::snippets_dir(paths);
    fs::create_dir_all(&snippets_dir).map_err(|source| SnippetError::Write {
        path: snippets_dir,
        source,
    })
}

pub fn list_snippets(paths: &PathsConfig) -> Result<Vec<SnippetSummary>, SnippetError> {
    let snippets_dir = paths::snippets_dir(paths);
    if !snippets_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut snippets = Vec::new();
    for entry in fs::read_dir(&snippets_dir).map_err(|source| SnippetError::Read {
        path: snippets_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| SnippetError::Read {
            path: snippets_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("toml")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let kind = snippet_kind(paths, stem)?;
        let aliases = match kind {
            SnippetKind::Insert => read_insert_metadata(paths, stem)?.aliases,
            SnippetKind::Template => {
                crate::snippet_templates::load_template_metadata(paths, stem)
                    .map_err(|error| SnippetError::Validation(error.to_string()))?
                    .aliases
            }
        };
        snippets.push(SnippetSummary {
            name: stem.to_owned(),
            kind,
            aliases,
        });
    }
    snippets.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(snippets)
}

pub fn create_snippet(paths: &PathsConfig, name: &str) -> Result<(), SnippetError> {
    validate_snippet_name(name)?;
    let snippets_dir = paths::snippets_dir(paths);
    fs::create_dir_all(&snippets_dir).map_err(|source| SnippetError::Write {
        path: snippets_dir.clone(),
        source,
    })?;
    let metadata_path = snippets_dir.join(format!("{name}.toml"));
    let content_path = snippets_dir.join(format!("{name}.md"));
    if metadata_path.exists() || content_path.exists() {
        return Err(SnippetError::AlreadyExists(name.into()));
    }
    let metadata = InsertSnippetMetadata {
        name: name.into(),
        snippet_type: INSERT_SNIPPET_TYPE.into(),
        aliases: vec![name.into()],
        content_file: format!("{name}.md"),
    };
    write_metadata(&metadata_path, &metadata)?;
    fs::write(&content_path, NEW_SNIPPET_CONTENT_TEMPLATE).map_err(|source| {
        SnippetError::Write {
            path: content_path,
            source,
        }
    })?;
    Ok(())
}

pub fn snippet_kind(paths: &PathsConfig, name: &str) -> Result<SnippetKind, SnippetError> {
    validate_snippet_name(name)?;
    let snippet_type = read_snippet_type(paths, name)?;
    match snippet_type.as_str() {
        INSERT_SNIPPET_TYPE => Ok(SnippetKind::Insert),
        crate::snippet_templates::TEMPLATE_SNIPPET_TYPE => Ok(SnippetKind::Template),
        other => Err(SnippetError::Validation(format!(
            "unsupported snippet type '{other}'"
        ))),
    }
}

pub fn validate_snippet(paths: &PathsConfig, name: &str) -> Result<(), SnippetError> {
    validate_snippet_name(name)?;
    match snippet_kind(paths, name)? {
        SnippetKind::Insert => {
            let _ = load_snippet_content(paths, name)?;
        }
        SnippetKind::Template => {
            crate::snippet_templates::validate_template_snippet(paths, name)
                .map_err(|error| SnippetError::Validation(error.to_string()))?;
        }
    }
    Ok(())
}

#[must_use]
pub fn validate_installed_snippets(paths: &PathsConfig) -> Vec<SnippetValidationIssue> {
    let Ok(snippets) = list_snippets(paths) else {
        return vec![SnippetValidationIssue {
            snippet: "*".into(),
            message: "snippets directory is unreadable".into(),
        }];
    };
    let mut issues = Vec::new();
    for snippet in snippets {
        if let Err(error) = validate_snippet(paths, &snippet.name) {
            issues.push(SnippetValidationIssue {
                snippet: snippet.name,
                message: error.to_string(),
            });
        }
    }
    issues
}

pub fn load_snippet_content(paths: &PathsConfig, name: &str) -> Result<String, SnippetError> {
    validate_snippet_name(name)?;
    if snippet_kind(paths, name)? != SnippetKind::Insert {
        return Err(SnippetError::Validation(
            "snippet is a template; use template preview or dictation routing".into(),
        ));
    }
    let metadata = read_insert_metadata(paths, name)?;
    validate_insert_metadata_contents(&metadata)?;
    let content_path = paths::snippets_dir(paths).join(&metadata.content_file);
    if !content_path.is_file() {
        return Err(SnippetError::Read {
            path: content_path,
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "content file missing"),
        });
    }
    let content = fs::read_to_string(&content_path).map_err(|source| SnippetError::Read {
        path: content_path.clone(),
        source,
    })?;
    if content.trim().is_empty() {
        return Err(SnippetError::EmptyContent(content_path));
    }
    Ok(content)
}

fn read_snippet_type(paths: &PathsConfig, name: &str) -> Result<String, SnippetError> {
    let metadata_path = paths::snippets_dir(paths).join(format!("{name}.toml"));
    if !metadata_path.is_file() {
        return Err(SnippetError::NotFound(name.into()));
    }
    let metadata_text =
        fs::read_to_string(&metadata_path).map_err(|source| SnippetError::Read {
            path: metadata_path.clone(),
            source,
        })?;
    let probe: SnippetTypeProbe =
        toml::from_str(&metadata_text).map_err(|source| SnippetError::InvalidMetadata {
            path: metadata_path,
            source,
        })?;
    Ok(probe.snippet_type)
}

fn read_insert_metadata(
    paths: &PathsConfig,
    name: &str,
) -> Result<InsertSnippetMetadata, SnippetError> {
    let metadata_path = paths::snippets_dir(paths).join(format!("{name}.toml"));
    if !metadata_path.is_file() {
        return Err(SnippetError::NotFound(name.into()));
    }
    let metadata_text =
        fs::read_to_string(&metadata_path).map_err(|source| SnippetError::Read {
            path: metadata_path.clone(),
            source,
        })?;
    let metadata: InsertSnippetMetadata =
        toml::from_str(&metadata_text).map_err(|source| SnippetError::InvalidMetadata {
            path: metadata_path,
            source,
        })?;
    if metadata.name != name {
        return Err(SnippetError::NotFound(format!(
            "{name} (metadata name is {})",
            metadata.name
        )));
    }
    validate_insert_metadata_contents(&metadata)?;
    Ok(metadata)
}

fn validate_insert_metadata_contents(metadata: &InsertSnippetMetadata) -> Result<(), SnippetError> {
    if metadata.snippet_type != INSERT_SNIPPET_TYPE {
        return Err(SnippetError::Validation(format!(
            "expected insert snippet, found '{}'",
            metadata.snippet_type
        )));
    }
    if metadata.content_file.trim().is_empty() {
        return Err(SnippetError::Validation(
            "content_file cannot be empty".into(),
        ));
    }
    if metadata.content_file.contains('/')
        || metadata.content_file.contains('\\')
        || metadata.content_file.contains("..")
    {
        return Err(SnippetError::Validation(
            "content_file must be a file name in the snippets directory".into(),
        ));
    }
    Ok(())
}

fn write_metadata(path: &PathBuf, metadata: &InsertSnippetMetadata) -> Result<(), SnippetError> {
    let text = toml::to_string_pretty(metadata).map_err(|error| SnippetError::Write {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
    })?;
    fs::write(path, text).map_err(|source| SnippetError::Write {
        path: path.clone(),
        source,
    })
}

pub(crate) fn validate_snippet_name(name: &str) -> Result<(), SnippetError> {
    let name = name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err(SnippetError::InvalidName);
    }
    Ok(())
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
    fn create_and_load_insert_snippet() {
        let base = std::env::temp_dir().join(format!("voxline-snippets-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        create_snippet(&paths, "signature").unwrap();
        let content = load_snippet_content(&paths, "signature").unwrap();
        assert!(content.contains("Replace this text"));
        validate_snippet(&paths, "signature").unwrap();
        let snippets = list_snippets(&paths).unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].name, "signature");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_duplicate_snippet_creation() {
        let base = std::env::temp_dir().join(format!("voxline-snippets-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        create_snippet(&paths, "greeting").unwrap();
        assert!(matches!(
            create_snippet(&paths, "greeting"),
            Err(SnippetError::AlreadyExists(_))
        ));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn validates_template_snippet_separately() {
        let base = std::env::temp_dir().join(format!("voxline-snippets-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        crate::snippet_templates::create_template_snippet(&paths, "standup").unwrap();
        assert_eq!(
            snippet_kind(&paths, "standup").unwrap(),
            SnippetKind::Template
        );
        validate_snippet(&paths, "standup").unwrap();
        let _ = fs::remove_dir_all(&base);
    }
}
