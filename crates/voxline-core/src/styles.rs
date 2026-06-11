use std::{fs, path::PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::{config::PathsConfig, paths};

pub const DEFAULT_STYLE_NAME: &str = "default";

const BUNDLED_DEFAULT_TOML: &str = include_str!("../assets/styles/default.toml");
const BUNDLED_DEFAULT_PROMPT: &str = include_str!("../assets/styles/default.md");

#[derive(Debug, Error)]
pub enum StyleError {
    #[error("style {0} was not found")]
    NotFound(String),
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
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct StyleMetadata {
    name: String,
    description: String,
    prompt_file: String,
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

pub fn load_style_prompt(paths: &PathsConfig, style_name: &str) -> Result<String, StyleError> {
    if style_name.trim().is_empty() {
        return Err(StyleError::NotFound(style_name.into()));
    }
    let styles_dir = paths::styles_dir(paths);
    let metadata_path = styles_dir.join(format!("{style_name}.toml"));
    if !metadata_path.is_file() {
        if style_name == DEFAULT_STYLE_NAME {
            ensure_default_style_files(paths)?;
            return load_style_prompt(paths, style_name);
        }
        return Err(StyleError::NotFound(style_name.into()));
    }
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
    let prompt_path = styles_dir.join(&metadata.prompt_file);
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
    fn bundled_default_prompt_matches_asset_file() {
        assert!(BUNDLED_DEFAULT_PROMPT.contains("Return only the final text."));
    }
}
