use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use crate::secrets::SecretsConfig;
use crate::{
    apps, commands,
    paths::{self, scaffold_config_layout},
    snippets, styles,
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

fn default_config_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Schema version for future migrations. Must be `1` in v1.
    #[serde(default = "default_config_version")]
    pub config_version: u32,
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
    pub voice_commands: VoiceCommandsConfig,
    pub preview: PreviewConfig,
    pub overlay: OverlayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceCommandsConfig {
    pub enabled: bool,
    pub prefix: String,
}

impl Default for VoiceCommandsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            prefix: "skald".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub enabled: bool,
    pub chunk_ms: u64,
    pub step_ms: u64,
    pub overlap_ms: u64,
    pub min_rms_energy: f32,
    pub ring_buffer_seconds: u64,
    pub gpu: bool,
    /// Defaults to `ggml-small.en.bin` under `paths.model_dir` when empty.
    pub model_path: String,
    /// Zero uses `asr.threads`.
    pub threads: u16,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            chunk_ms: 2_000,
            step_ms: 1_000,
            overlap_ms: 500,
            min_rms_energy: 0.003,
            ring_buffer_seconds: 30,
            gpu: false,
            model_path: "~/.local/share/skald/models/ggml-small.en.bin".into(),
            threads: 0,
        }
    }
}

impl PreviewConfig {
    #[must_use]
    pub fn effective_model_path(&self) -> String {
        let trimmed = self.model_path.trim();
        if trimmed.is_empty() {
            "~/.local/share/skald/models/ggml-small.en.bin".into()
        } else {
            trimmed.to_owned()
        }
    }

    #[must_use]
    pub fn effective_threads(&self) -> u16 {
        if self.threads == 0 { 4 } else { self.threads }
    }

    #[must_use]
    pub fn to_asr_config(&self, asr: &AsrConfig) -> AsrConfig {
        AsrConfig {
            model_path: self.effective_model_path(),
            threads: self.effective_threads(),
            gpu: self.gpu,
            lifecycle: AsrLifecycleConfig {
                mode: "keep_warm".into(),
                warm_on_daemon_start: true,
                idle_unload_seconds: 900,
            },
            ..asr.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct OverlayConfig {
    /// text | visualizer
    pub mode: String,
    /// waveform | bars | pulse | dots
    pub visualizer_style: String,
    pub margin_px: u32,
    pub max_width_px: u32,
    /// top | bottom | auto (cursor-aware on supported compositors)
    pub anchor: String,
    pub use_layer_shell: bool,
    pub hide_when_idle: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            mode: "text".into(),
            visualizer_style: "waveform".into(),
            margin_px: 16,
            max_width_px: 720,
            anchor: "auto".into(),
            use_layer_shell: true,
            hide_when_idle: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonConfig {
    pub log_level: String,
    pub max_concurrent_jobs: u32,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    pub config_dir: String,
    pub model_dir: String,
    pub runtime_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub backend: String,
    pub device: String,
    pub target_sample_rate: u32,
    pub channels: u16,
    pub max_record_seconds: u64,
    pub gates: AudioGatesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AudioGatesConfig {
    pub min_record_ms: u64,
    pub min_rms_energy: f32,
    pub min_peak_energy: f32,
    pub notify_on_no_speech: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct AsrLifecycleConfig {
    pub mode: String,
    pub warm_on_daemon_start: bool,
    pub idle_unload_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct VocabularyConfig {
    pub enabled: bool,
    pub initial_prompt_enabled: bool,
    pub post_replace_enabled: bool,
    pub phrases: Vec<VocabularyPhrase>,
    pub replacements: Vec<VocabularyReplacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VocabularyPhrase {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VocabularyReplacement {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HallucinationFilterConfig {
    pub enabled: bool,
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct InjectionLinuxConfig {
    pub session: String,
    pub wayland_paste_command: String,
    pub x11_paste_command: String,
    pub gnome_wayland_mode: String,
    pub optional_paste_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationsConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct PrivacyConfig {
    pub store_history: bool,
    pub store_audio: bool,
    pub store_raw_transcript: bool,
    pub store_cleaned_transcript: bool,
    pub log_transcripts: bool,
    pub emit_transcript_in_events: bool,
}

impl PrivacyConfig {
    #[must_use]
    pub fn sensitive_storage_or_logging_enabled(&self) -> bool {
        self.store_audio || self.log_transcripts || self.emit_transcript_in_events
    }
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
            config_dir: "~/.config/skald".into(),
            model_dir: "~/.local/share/skald/models".into(),
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
            model_path: "~/.local/share/skald/models/ggml-large-v3-turbo-q5_0.bin".into(),
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
                    text: "Skald".into(),
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
                "subtitles by*".into(),
                "subtitle by*".into(),
                "captioned by*".into(),
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

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: default_config_version(),
            daemon: DaemonConfig::default(),
            paths: PathsConfig::default(),
            audio: AudioConfig::default(),
            asr: AsrConfig::default(),
            vocabulary: VocabularyConfig::default(),
            cleanup: CleanupConfig::default(),
            secrets: SecretsConfig::default(),
            injection: InjectionConfig::default(),
            notifications: NotificationsConfig::default(),
            privacy: PrivacyConfig::default(),
            voice_commands: VoiceCommandsConfig::default(),
            preview: PreviewConfig::default(),
            overlay: OverlayConfig::default(),
        }
    }
}

impl Config {
    #[must_use]
    pub fn preview_enabled_effective(&self) -> bool {
        self.preview.enabled && self.overlay.mode == "text"
    }

    /// Returns the fixed path to `config.toml`.
    ///
    /// This always resolves to `dirs::config_dir()/skald/config.toml` and does
    /// not read `paths.config_dir` from an on-disk config (that would be
    /// circular during bootstrap). Use `paths.config_dir` only for styles,
    /// apps, snippets, and other layout files.
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|path| path.join("skald/config.toml"))
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

    /// Loads the config file (or defaults when missing) and runs [`Self::validate`].
    pub fn load_validated() -> Result<Self, ConfigError> {
        let config = Self::load_or_default()?;
        config.validate()?;
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
        apps::ensure_default_app_profiles(&config.paths)
            .map_err(|error| ConfigError::Validation(error.to_string()))?;
        snippets::ensure_snippets_dir(&config.paths)
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
                self.asr.model_path = "~/.local/share/skald/models/ggml-small.en.bin".into();
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
        self.validate_all().into_iter().next().map_or(Ok(()), Err)
    }

    #[must_use]
    pub fn validate_all(&self) -> Vec<ConfigError> {
        let mut errors = Vec::new();
        collect_schema_errors(self, &mut errors);
        collect_audio_errors(self, &mut errors);
        collect_asr_errors(self, &mut errors);
        collect_secrets_and_cleanup_errors(self, &mut errors);
        collect_injection_and_paths_errors(self, &mut errors);
        collect_privacy_reserved_errors(self, &mut errors);
        collect_overlay_and_preview_errors(self, &mut errors);
        if let Err(error) = commands::validate_voice_commands(&self.voice_commands, &self.paths)
            .map_err(|error| ConfigError::Validation(format!("voice_commands: {error}")))
        {
            errors.push(error);
        }
        if self.cleanup.enabled {
            collect_cleanup_style_errors(self, &mut errors);
        }
        collect_layout_file_errors(self, &mut errors);
        errors
    }

    #[must_use]
    pub fn validation_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let model = paths::expand_home(&self.asr.model_path);
        if !model.is_file() {
            warnings.push(format!(
                "ASR model not found at {} (may not be downloaded yet)",
                model.display()
            ));
        }
        warnings
    }
}

fn push_validation(errors: &mut Vec<ConfigError>, message: String) {
    errors.push(ConfigError::Validation(message));
}

fn collect_schema_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.config_version != 1 {
        push_validation(
            errors,
            format!(
                "config_version must be 1 (found {}); see docs for upgrading",
                config.config_version
            ),
        );
    }
    if config.daemon.protocol_version != 1 {
        push_validation(errors, "protocol_version must be 1".into());
    }
    if config.daemon.max_concurrent_jobs != 1 {
        push_validation(errors, "max_concurrent_jobs must be 1 in v1".into());
    }
    if !matches!(
        config.daemon.log_level.as_str(),
        "error" | "warn" | "info" | "debug" | "trace"
    ) {
        push_validation(
            errors,
            "daemon.log_level must be error, warn, info, debug, or trace".into(),
        );
    }
}

fn collect_audio_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.audio.backend != "cpal" {
        push_validation(errors, "audio.backend must be cpal".into());
    }
    if config.audio.target_sample_rate != 16_000 || config.audio.channels != 1 {
        push_validation(errors, "v1 audio output must be 16 kHz mono".into());
    }
    let max_record_ms = config.audio.max_record_seconds.saturating_mul(1_000);
    if config.audio.gates.min_record_ms > max_record_ms {
        push_validation(
            errors,
            format!(
                "audio.gates.min_record_ms ({}) must not exceed max_record_seconds * 1000 ({max_record_ms})",
                config.audio.gates.min_record_ms
            ),
        );
    }
    if !(0.0..=1.0).contains(&config.audio.gates.min_rms_energy) {
        push_validation(
            errors,
            "audio.gates.min_rms_energy must be between 0.0 and 1.0".into(),
        );
    }
    if !(0.0..=1.0).contains(&config.audio.gates.min_peak_energy) {
        push_validation(
            errors,
            "audio.gates.min_peak_energy must be between 0.0 and 1.0".into(),
        );
    }
}

fn collect_asr_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.asr.backend != "whisper_rs" {
        push_validation(errors, "asr.backend must be whisper_rs".into());
    }
    if !matches!(config.asr.gpu_backend.as_str(), "cuda" | "vulkan" | "none") {
        push_validation(
            errors,
            "asr.gpu_backend must be cuda, vulkan, or none".into(),
        );
    }
    if config.asr.threads == 0 {
        push_validation(errors, "asr.threads must be at least 1".into());
    }
    if !matches!(
        config.asr.lifecycle.mode.as_str(),
        "on_demand" | "keep_warm"
    ) {
        push_validation(
            errors,
            "asr lifecycle mode must be on_demand or keep_warm".into(),
        );
    }
}

fn collect_secrets_and_cleanup_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.secrets.mode != "auto" {
        push_validation(errors, "secrets.mode must be auto".into());
    }
    if !matches!(config.cleanup.provider.as_str(), "openrouter" | "none") {
        push_validation(errors, "cleanup.provider must be openrouter or none".into());
    }
    if !(0.0..=2.0).contains(&config.cleanup.temperature) {
        push_validation(
            errors,
            "cleanup.temperature must be between 0.0 and 2.0".into(),
        );
    }
    if config.cleanup.timeout_ms == 0 {
        push_validation(
            errors,
            "cleanup.timeout_ms must be greater than zero".into(),
        );
    }
    if config.cleanup.enabled && config.cleanup.provider == "none" {
        push_validation(
            errors,
            "cleanup provider cannot be none when cleanup is enabled".into(),
        );
    }
    if config.cleanup.enabled
        && config.cleanup.provider == "openrouter"
        && config.cleanup.model.trim().is_empty()
    {
        push_validation(
            errors,
            "cleanup model is required when openrouter cleanup is enabled".into(),
        );
    }
    if config.cleanup.enabled && config.cleanup.default_style.trim().is_empty() {
        push_validation(
            errors,
            "cleanup.default_style is required when cleanup is enabled".into(),
        );
    }
}

fn collect_injection_and_paths_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.injection.auto_paste != AutoPasteMode::Off && !config.injection.copy_to_clipboard {
        push_validation(errors, "auto paste requires copy_to_clipboard".into());
    }
    if !matches!(
        config.injection.linux.gnome_wayland_mode.as_str(),
        "clipboard_only" | "custom"
    ) {
        push_validation(
            errors,
            "injection.linux.gnome_wayland_mode must be clipboard_only or custom".into(),
        );
    }
    if config.injection.linux.gnome_wayland_mode == "custom"
        && config
            .injection
            .linux
            .optional_paste_command
            .trim()
            .is_empty()
    {
        push_validation(
            errors,
            "injection.linux.optional_paste_command is required when gnome_wayland_mode is custom"
                .into(),
        );
    }
    if config.paths.runtime_dir.trim().is_empty() {
        push_validation(errors, "paths.runtime_dir cannot be empty".into());
    }
}

fn collect_privacy_reserved_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if config.privacy.store_history {
        push_validation(
            errors,
            "store_history is reserved and not yet implemented; remove it from config or set it to false".into(),
        );
    }
    if config.privacy.store_raw_transcript {
        push_validation(
            errors,
            "store_raw_transcript is reserved and not yet implemented; remove it from config or set it to false".into(),
        );
    }
    if config.privacy.store_cleaned_transcript {
        push_validation(
            errors,
            "store_cleaned_transcript is reserved and not yet implemented; remove it from config or set it to false".into(),
        );
    }
}

fn collect_overlay_and_preview_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if !matches!(config.overlay.mode.as_str(), "text" | "visualizer") {
        push_validation(errors, "overlay.mode must be text or visualizer".into());
    }
    if !matches!(
        config.overlay.visualizer_style.as_str(),
        "waveform" | "bars" | "pulse" | "dots"
    ) {
        push_validation(
            errors,
            "overlay.visualizer_style must be waveform, bars, pulse, or dots".into(),
        );
    }
    if !matches!(config.overlay.anchor.as_str(), "top" | "bottom" | "auto") {
        push_validation(errors, "overlay.anchor must be top, bottom, or auto".into());
    }
    if !config.preview_enabled_effective() {
        return;
    }
    if config.preview.chunk_ms == 0 || config.preview.step_ms == 0 {
        push_validation(
            errors,
            "preview chunk_ms and step_ms must be greater than zero".into(),
        );
    }
    if config.preview.overlap_ms >= config.preview.chunk_ms {
        push_validation(
            errors,
            "preview overlap_ms must be less than chunk_ms".into(),
        );
    }
    if config.preview.ring_buffer_seconds == 0 {
        push_validation(
            errors,
            "preview ring_buffer_seconds must be greater than zero".into(),
        );
    }
    if !(0.0..=1.0).contains(&config.preview.min_rms_energy) {
        push_validation(
            errors,
            "preview.min_rms_energy must be between 0.0 and 1.0".into(),
        );
    }
    let preview_model = paths::expand_home(&config.preview.effective_model_path());
    if !preview_model.is_file() {
        push_validation(
            errors,
            format!("preview model not found at {}", preview_model.display()),
        );
    }
}

fn collect_cleanup_style_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    if let Err(error) = styles::ensure_default_style_files(&config.paths)
        .map_err(|error| ConfigError::Validation(format!("default style files: {error}")))
    {
        errors.push(error);
        return;
    }
    if let Err(error) = styles::validate_style(&config.paths, &config.cleanup.default_style)
        .map_err(|error| {
            ConfigError::Validation(format!(
                "cleanup.default_style '{}': {error}",
                config.cleanup.default_style
            ))
        })
    {
        errors.push(error);
    }
    for issue in styles::validate_installed_styles(&config.paths) {
        push_validation(errors, format!("style {}: {}", issue.style, issue.message));
    }
}

fn collect_layout_file_errors(config: &Config, errors: &mut Vec<ConfigError>) {
    for issue in apps::validate_installed_app_profiles(&config.paths) {
        push_validation(errors, format!("app {}: {}", issue.app, issue.message));
    }
    for issue in snippets::validate_installed_snippets(&config.paths) {
        push_validation(
            errors,
            format!("snippet {}: {}", issue.snippet, issue.message),
        );
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
        assert!(!config.privacy.sensitive_storage_or_logging_enabled());
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

    #[test]
    fn preview_asr_config_uses_small_model_and_cpu_defaults() {
        let mut config = Config::default();
        config.preview.enabled = true;
        config.preview.model_path.clear();
        let preview_asr = config.preview.to_asr_config(&config.asr);
        assert_eq!(
            preview_asr.model_path,
            "~/.local/share/skald/models/ggml-small.en.bin"
        );
        assert!(!preview_asr.gpu);
        assert_eq!(preview_asr.lifecycle.mode, "keep_warm");
    }

    #[test]
    fn rejects_unknown_config_keys() {
        let err = toml::from_str::<Config>("enabeld = true").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("unknown field") && message.contains("enabeld"),
            "expected unknown field error naming enabeld, got: {message}"
        );
    }

    #[test]
    fn rejects_invalid_cleanup_provider() {
        let mut config = Config::default();
        config.cleanup.provider = "openai".into();
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("cleanup.provider must be openrouter or none")
        }));
    }

    #[test]
    fn rejects_invalid_log_level() {
        let mut config = Config::default();
        config.daemon.log_level = "verbose".into();
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("daemon.log_level must be error, warn, info, debug, or trace")
        }));
    }

    #[test]
    fn accepts_visualizer_overlay_without_preview() {
        let mut config = Config::default();
        config.overlay.mode = "visualizer".into();
        config.preview.enabled = false;
        config.validate().unwrap();
    }

    #[test]
    fn visualizer_mode_disables_preview_effectively() {
        let mut config = Config::default();
        config.preview.enabled = true;
        config.overlay.mode = "visualizer".into();
        assert!(!config.preview_enabled_effective());
    }

    #[test]
    fn rejects_invalid_overlay_mode() {
        let mut config = Config::default();
        config.overlay.mode = "both".into();
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("overlay.mode must be text or visualizer")
        }));
    }

    #[test]
    fn accepts_each_visualizer_style() {
        for style in ["waveform", "bars", "pulse", "dots"] {
            let mut config = Config::default();
            config.overlay.visualizer_style = style.into();
            config.validate().unwrap();
        }
    }

    #[test]
    fn rejects_invalid_visualizer_style() {
        let mut config = Config::default();
        config.overlay.visualizer_style = "spectrum".into();
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("overlay.visualizer_style must be waveform, bars, pulse, or dots")
        }));
    }

    #[test]
    fn rejects_zero_asr_threads() {
        let mut config = Config::default();
        config.asr.threads = 0;
        let errors = config.validate_all();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("asr.threads must be at least 1"))
        );
    }

    #[test]
    fn rejects_out_of_range_cleanup_temperature() {
        let mut config = Config::default();
        config.cleanup.temperature = 3.0;
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("cleanup.temperature must be between 0.0 and 2.0")
        }));
    }

    #[test]
    fn validate_all_collects_multiple_issues() {
        let mut config = Config::default();
        config.daemon.log_level = "verbose".into();
        config.asr.threads = 0;
        config.cleanup.provider = "openai".into();
        let errors = config.validate_all();
        assert!(errors.len() >= 3);
    }

    #[test]
    fn rejects_reserved_privacy_storage_flags() {
        let mut config = Config::default();
        config.privacy.store_history = true;
        let errors = config.validate_all();
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("store_history is reserved and not yet implemented")
        }));
    }

    #[test]
    fn validation_warnings_for_missing_asr_model() {
        let mut config = Config::default();
        config.asr.model_path = "/tmp/skald-nonexistent-model-for-test.bin".into();
        let warnings = config.validation_warnings();
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("ASR model not found"))
        );
    }
}
