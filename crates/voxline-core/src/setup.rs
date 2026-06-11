use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    config::{Config, PathsConfig},
    paths::{expand_home, resolve_model_dir, scaffold_config_layout},
};

const SETUP_MARKER: &str = "setup-complete.json";
const SETUP_FIXTURE: &str = "samples/setup.wav";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SetupCompleteMarker {
    pub completed_at: String,
    pub asr_model_id: String,
    pub preview_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct SetupSelection {
    pub asr_model_id: String,
    pub asr_model_path: PathBuf,
    pub asr_gpu: bool,
    pub asr_threads: u16,
    pub lifecycle_mode: String,
    pub warm_on_daemon_start: bool,
    pub idle_unload_seconds: u64,
    pub preview_enabled: bool,
    pub preview_model_path: Option<PathBuf>,
    pub preview_gpu: bool,
    pub cleanup_enabled: bool,
}

#[must_use]
pub fn setup_marker_path(paths: &PathsConfig) -> PathBuf {
    expand_home(&paths.model_dir).join(SETUP_MARKER)
}

#[must_use]
pub fn setup_fixture_path(paths: &PathsConfig) -> PathBuf {
    expand_home(&paths.model_dir).join(SETUP_FIXTURE)
}

#[must_use]
pub fn is_setup_complete(paths: &PathsConfig) -> bool {
    let marker = setup_marker_path(paths);
    marker.is_file() && Config::path().is_ok_and(|path| path.is_file())
}

pub fn mark_setup_complete(
    paths: &PathsConfig,
    selection: &SetupSelection,
) -> Result<(), std::io::Error> {
    let marker_path = setup_marker_path(paths);
    if let Some(parent) = marker_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let marker = SetupCompleteMarker {
        completed_at: chrono_lite_now(),
        asr_model_id: selection.asr_model_id.clone(),
        preview_enabled: selection.preview_enabled,
    };
    let json = serde_json::to_string_pretty(&marker).map_err(std::io::Error::other)?;
    std::fs::write(marker_path, json)
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

fn path_to_tilde(path: &Path, model_dir: &Path, model_dir_tilde: &str) -> String {
    if let Ok(relative) = path.strip_prefix(model_dir)
        && let Some(file_name) = relative.file_name()
    {
        return format!(
            "{}/{}",
            model_dir_tilde.trim_end_matches('/'),
            file_name.to_string_lossy()
        );
    }
    if let Some(home) = dirs::home_dir()
        && let Ok(relative) = path.strip_prefix(home)
    {
        return format!("~/{}", relative.display()).replace('\\', "/");
    }
    path.display().to_string()
}

impl Config {
    pub fn from_setup_selection(
        mut config: Config,
        selection: &SetupSelection,
    ) -> Result<Self, crate::config::ConfigError> {
        let model_dir = resolve_model_dir(&config.paths);
        config.asr.model_path = path_to_tilde(
            &selection.asr_model_path,
            &model_dir,
            &config.paths.model_dir,
        );
        config.asr.gpu = selection.asr_gpu;
        config.asr.threads = selection.asr_threads;
        config
            .asr
            .lifecycle
            .mode
            .clone_from(&selection.lifecycle_mode);
        config.asr.lifecycle.warm_on_daemon_start = selection.warm_on_daemon_start;
        config.asr.lifecycle.idle_unload_seconds = selection.idle_unload_seconds;
        config.preview.enabled = selection.preview_enabled;
        if let Some(path) = &selection.preview_model_path {
            config.preview.model_path = path_to_tilde(path, &model_dir, &config.paths.model_dir);
        }
        config.preview.gpu = selection.preview_gpu;
        config.cleanup.enabled = selection.cleanup_enabled;
        if !selection.cleanup_enabled {
            config.cleanup.provider = "none".into();
        }
        config.validate()?;
        Ok(config)
    }

    pub fn ensure_setup_fixture_dir(paths: &PathsConfig) -> Result<PathBuf, std::io::Error> {
        scaffold_config_layout(paths)?;
        let fixture = setup_fixture_path(paths);
        if let Some(parent) = fixture.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(fixture)
    }
}

#[must_use]
pub fn config_file_exists() -> bool {
    Config::path().is_ok_and(|path| path.is_file())
}

#[must_use]
pub fn needs_setup(paths: &PathsConfig) -> bool {
    !config_file_exists() || !is_setup_complete(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn needs_setup_when_no_config_file() {
        let config = Config::default();
        assert!(!is_setup_complete(&config.paths));
    }

    #[test]
    fn from_setup_selection_applies_asr_fields() {
        let config = Config::default();
        let model_dir = std::env::temp_dir().join(format!("voxline-setup-{}", ulid::Ulid::new()));
        let selection = SetupSelection {
            asr_model_id: "small.en".into(),
            asr_model_path: model_dir.join("ggml-small.en.bin"),
            asr_gpu: false,
            asr_threads: 4,
            lifecycle_mode: "on_demand".into(),
            warm_on_daemon_start: false,
            idle_unload_seconds: 0,
            preview_enabled: false,
            preview_model_path: None,
            preview_gpu: false,
            cleanup_enabled: false,
        };
        let updated = Config::from_setup_selection(config, &selection).unwrap();
        assert!(!updated.asr.gpu);
        assert_eq!(updated.asr.lifecycle.mode, "on_demand");
        let _ = std::fs::remove_dir_all(model_dir);
    }
}
