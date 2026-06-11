use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use crate::secrets::SecretsConfig;
use crate::{
    paths::{self, scaffold_config_layout},
    styles,
};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the user configuration directory")]
    ConfigDirectoryUnavailable,
    #[error("configuration already exists at {0}")]
    AlreadyExists(PathBuf),
    #[error("failed to read configuration at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write configuration at {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub paths: PathsConfig,
    pub audio: AudioConfig,
    pub asr: AsrConfig,
    pub vocabulary: VocabularyConfig,
    pub cleanup: CleanupConfig,
    pub secrets: SecretsConfig,
    pub injection: InjectionConfig,
    pub notifications: NotificationsConfig,
    pub privacy: PrivacyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DaemonConfig {
    pub log_level: String,
    pub max_concurrent_jobs: u32,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PathsConfig {
    pub config_dir: String,
    pub model_dir: String,
    pub runtime_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioConfig {
    pub backend: String,
    pub device: String,
    pub target_sample_rate: u32,
    pub channels: u16,
    pub max_record_seconds: u64,
    pub gates: AudioGatesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioGatesConfig {
    pub min_record_ms: u64,
    pub min_rms_energy: f32,
    pub min_peak_energy: f32,
    pub notify_on_no_speech: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AsrConfig {
    pub backend: String,
    pub model_path: String,
    pub language: String,
    pub threads: u16,
    pub gpu: bool,
    pub gpu_backend: String,
    pub lifecycle: AsrLifecycleConfig,
    pub hallucination_filter: HallucinationFilterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AsrLifecycleConfig {
    pub mode: String,
    pub warm_on_daemon_start: bool,
    pub idle_unload_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VocabularyConfig {
    pub enabled: bool,
    pub initial_prompt_enabled: bool,
    pub post_replace_enabled: bool,
    pub phrases: Vec<VocabularyPhrase>,
    pub replacements: Vec<VocabularyReplacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocabularyPhrase {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocabularyReplacement {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HallucinationFilterConfig {
    pub enabled: bool,
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CleanupConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub default_style: String,
    pub temperature: f32,
    pub timeout_ms: u64,
    pub fallback_to_raw_on_error: bool,
    pub skip_if_word_count_below: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutoPasteMode {
    Off,
    Safe,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct InjectionConfig {
    pub copy_to_clipboard: bool,
    pub auto_paste: AutoPasteMode,
    pub max_paste_age_ms: u64,
    pub restore_clipboard: bool,
    pub paste_delay_ms: u64,
    pub fallback_to_clipboard_only: bool,
    pub notify_on_clipboard_only: bool,
    pub linux: InjectionLinuxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InjectionLinuxConfig {
    pub session: String,
    pub wayland_paste_command: String,
    pub x11_paste_command: String,
    pub gnome_wayland_mode: String,
    pub optional_paste_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NotificationsConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct PrivacyConfig {
    pub store_history: bool,
    pub store_audio: bool,
    pub store_raw_transcript: bool,
    pub store_cleaned_transcript: bool,
    pub log_transcripts: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            max_concurrent_jobs: 1,
            protocol_version: 1,
        }
    }
}
impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            config_dir: "~/.config/voxline".into(),
            model_dir: "~/.local/share/voxline/models".into(),
            runtime_dir: "auto".into(),
        }
    }
}
impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "cpal".into(),
            device: "default".into(),
            target_sample_rate: 16_000,
            channels: 1,
            max_record_seconds: 300,
            gates: AudioGatesConfig::default(),
        }
    }
}
impl Default for AudioGatesConfig {
    fn default() -> Self {
        Self {
            min_record_ms: 350,
            min_rms_energy: 0.003,
            min_peak_energy: 0.015,
            notify_on_no_speech: true,
        }
    }
}
impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            backend: "whisper_rs".into(),
            model_path: "~/.local/share/voxline/models/ggml-large-v3-turbo-q5_0.bin".into(),
            language: "en".into(),
            threads: 8,
            gpu: true,
            gpu_backend: "cuda".into(),
            lifecycle: AsrLifecycleConfig::default(),
            hallucination_filter: HallucinationFilterConfig::default(),
        }
    }
}
impl Default for AsrLifecycleConfig {
    fn default() -> Self {
        Self {
            mode: "keep_warm".into(),
            warm_on_daemon_start: true,
            idle_unload_seconds: 900,
        }
    }
}
impl Default for VocabularyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_prompt_enabled: true,
            post_replace_enabled: true,
            phrases: vec![
                VocabularyPhrase {
                    text: "OpenRouter".into(),
                },
                VocabularyPhrase {
                    text: "Hyprland".into(),
                },
                VocabularyPhrase {
                    text: "VoxLine".into(),
                },
            ],
            replacements: vec![
                VocabularyReplacement {
                    from: "hyper land".into(),
                    to: "Hyprland".into(),
                    case_sensitive: false,
                },
                VocabularyReplacement {
                    from: "open router".into(),
                    to: "OpenRouter".into(),
                    case_sensitive: false,
                },
            ],
        }
    }
}
impl Default for HallucinationFilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            phrases: vec![
                "thank you.".into(),
                "thanks for watching.".into(),
                "subtitles by".into(),
                "subtitle by".into(),
                "captioned by".into(),
            ],
        }
    }
}
impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "none".into(),
            model: String::new(),
            default_style: styles::DEFAULT_STYLE_NAME.into(),
            temperature: 0.2,
            timeout_ms: 10_000,
            fallback_to_raw_on_error: true,
            skip_if_word_count_below: 5,
        }
    }
}
impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            copy_to_clipboard: true,
            auto_paste: AutoPasteMode::Safe,
            max_paste_age_ms: 5_000,
            restore_clipboard: true,
            paste_delay_ms: 120,
            fallback_to_clipboard_only: true,
            notify_on_clipboard_only: true,
            linux: InjectionLinuxConfig::default(),
        }
    }
}
impl Default for InjectionLinuxConfig {
    fn default() -> Self {
        Self {
            session: "auto".into(),
            wayland_paste_command: "wtype -M ctrl -k v -m ctrl".into(),
            x11_paste_command: "xdotool key ctrl+v".into(),
            gnome_wayland_mode: "clipboard_only".into(),
            optional_paste_command: String::new(),
        }
    }
}
impl Default for NotificationsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
impl Config {
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|path| path.join("voxline/config.toml"))
            .ok_or(ConfigError::ConfigDirectoryUnavailable)
    }

    pub fn load_or_default() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let config = toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })?;
        Ok(config)
    }

    pub fn init(force: bool) -> Result<PathBuf, ConfigError> {
        let config = Self::default();
        let path = Self::path()?;
        if path.exists() && !force {
            return Err(ConfigError::AlreadyExists(path));
        }
        scaffold_config_layout(&config.paths).map_err(|source| ConfigError::Write {
            path: paths::resolve_config_dir(&config.paths),
            source,
        })?;
        styles::ensure_default_style_files(&config.paths)
            .map_err(|error| ConfigError::Validation(error.to_string()))?;
        let text = toml::to_string_pretty(&config)
            .map_err(|error| ConfigError::Validation(error.to_string()))?;
        fs::write(&path, text).map_err(|source| ConfigError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    pub fn apply_profile(&mut self, profile: &str) -> Result<(), ConfigError> {
        match profile {
            "power-user-nvidia" => {
                *self = Self {
                    secrets: self.secrets.clone(),
                    cleanup: self.cleanup.clone(),
                    ..Self::default()
                };
            }
            "cpu-safe" => {
                self.asr.model_path = "~/.local/share/voxline/models/ggml-small.en.bin".into();
                self.asr.threads = 4;
                self.asr.gpu = false;
                self.asr.lifecycle.mode = "on_demand".into();
                self.asr.lifecycle.warm_on_daemon_start = false;
                self.asr.lifecycle.idle_unload_seconds = 0;
                self.cleanup.enabled = false;
                self.cleanup.provider = "none".into();
            }
            other => {
                return Err(ConfigError::Validation(format!(
                    "unknown config profile: {other}"
                )));
            }
        }
        self.validate()
    }

    pub fn save(&self) -> Result<PathBuf, ConfigError> {
        let path = Self::path()?;
        let parent = path
            .parent()
            .ok_or(ConfigError::ConfigDirectoryUnavailable)?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
        let text = toml::to_string_pretty(self)
            .map_err(|error| ConfigError::Validation(error.to_string()))?;
        fs::write(&path, text).map_err(|source| ConfigError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.daemon.protocol_version != 1 {
            return Err(ConfigError::Validation("protocol_version must be 1".into()));
        }
        if self.daemon.max_concurrent_jobs != 1 {
            return Err(ConfigError::Validation(
                "max_concurrent_jobs must be 1 in v1".into(),
            ));
        }
        if self.audio.target_sample_rate != 16_000 || self.audio.channels != 1 {
            return Err(ConfigError::Validation(
                "v1 audio output must be 16 kHz mono".into(),
            ));
        }
        if self.cleanup.enabled && self.cleanup.provider == "none" {
            return Err(ConfigError::Validation(
                "cleanup provider cannot be none when cleanup is enabled".into(),
            ));
        }
        if self.cleanup.enabled
            && self.cleanup.provider == "openrouter"
            && self.cleanup.model.trim().is_empty()
        {
            return Err(ConfigError::Validation(
                "cleanup model is required when openrouter cleanup is enabled".into(),
            ));
        }
        if self.cleanup.enabled && self.cleanup.default_style.trim().is_empty() {
            return Err(ConfigError::Validation(
                "cleanup.default_style is required when cleanup is enabled".into(),
            ));
        }
        if !matches!(self.asr.lifecycle.mode.as_str(), "on_demand" | "keep_warm") {
            return Err(ConfigError::Validation(
                "asr lifecycle mode must be on_demand or keep_warm".into(),
            ));
        }
        if self.injection.auto_paste != AutoPasteMode::Off && !self.injection.copy_to_clipboard {
            return Err(ConfigError::Validation(
                "auto paste requires copy_to_clipboard".into(),
            ));
        }
        if !matches!(
            self.injection.linux.gnome_wayland_mode.as_str(),
            "clipboard_only" | "custom"
        ) {
            return Err(ConfigError::Validation(
                "injection.linux.gnome_wayland_mode must be clipboard_only or custom".into(),
            ));
        }
        if self.injection.linux.gnome_wayland_mode == "custom"
            && self
                .injection
                .linux
                .optional_paste_command
                .trim()
                .is_empty()
        {
            return Err(ConfigError::Validation(
                "injection.linux.optional_paste_command is required when gnome_wayland_mode is custom".into(),
            ));
        }
        if self.paths.runtime_dir.trim().is_empty() {
            return Err(ConfigError::Validation(
                "paths.runtime_dir cannot be empty".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_private_and_valid() {
        let config = Config::default();
        config.validate().unwrap();
        assert!(!config.cleanup.enabled);
        assert!(!config.privacy.store_audio);
        assert!(!config.privacy.log_transcripts);
    }

    #[test]
    fn rejects_cleanup_without_provider() {
        let mut config = Config::default();
        config.cleanup.enabled = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn cpu_safe_profile_matches_plan() {
        let mut config = Config::default();
        config.apply_profile("cpu-safe").unwrap();
        assert!(!config.asr.gpu);
        assert_eq!(config.asr.lifecycle.mode, "on_demand");
        assert!(!config.cleanup.enabled);
    }

    #[test]
    fn default_includes_injection_linux_section() {
        let config = Config::default();
        assert_eq!(config.injection.linux.session, "auto");
        assert_eq!(config.injection.linux.gnome_wayland_mode, "clipboard_only");
    }

    #[test]
    fn linux_example_config_is_valid() {
        let text = include_str!("../../../config-example/linux/config.toml");
        let config: Config = toml::from_str(text).expect("example config should parse");
        config.validate().expect("example config should validate");
    }
}
