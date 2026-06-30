use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{config::PathsConfig, paths::resolve_model_dir, system_probe::SystemProfile};

pub const CATALOG_VERSION: u32 = 1;
pub const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const METADATA_FILE: &str = "managed-models.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub id: &'static str,
    pub file_name: &'static str,
    pub gpu: bool,
    pub expected_size: u64,
    pub approx_size_mib: u64,
    pub sha256: &'static str,
    pub language: &'static str,
    pub intended_use: &'static str,
    pub hardware_guidance: &'static str,
    pub description: &'static str,
}

pub const CATALOG: &[ModelCatalogEntry] = &[
    ModelCatalogEntry {
        id: "base.en",
        file_name: "ggml-base.en.bin",
        gpu: false,
        expected_size: 147_964_211,
        approx_size_mib: 150,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        language: "English",
        intended_use: "final",
        hardware_guidance: "CPU-safe baseline",
        description: "Fast CPU baseline",
    },
    ModelCatalogEntry {
        id: "small.en",
        file_name: "ggml-small.en.bin",
        gpu: false,
        expected_size: 487_614_201,
        approx_size_mib: 500,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        language: "English",
        intended_use: "final or preview",
        hardware_guidance: "CPU-safe quality default",
        description: "Quality CPU default",
    },
    ModelCatalogEntry {
        id: "small.en-q5",
        file_name: "ggml-small.en-q5_1.bin",
        gpu: true,
        expected_size: 190_098_681,
        approx_size_mib: 200,
        sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        language: "English",
        intended_use: "preview",
        hardware_guidance: "NVIDIA/CUDA preview",
        description: "Fast GPU model, good for preview",
    },
    ModelCatalogEntry {
        id: "large-v3-turbo-q5",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        gpu: true,
        expected_size: 574_041_195,
        approx_size_mib: 548,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        language: "Multilingual",
        intended_use: "final",
        hardware_guidance: "NVIDIA/CUDA power-user",
        description: "Highest quality CUDA model",
    },
];

#[must_use]
pub fn catalog_entry(id: &str) -> Option<&'static ModelCatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

#[must_use]
pub fn download_url(entry: &ModelCatalogEntry) -> String {
    format!("{HF_BASE}/{}", entry.file_name)
}

#[must_use]
pub fn model_file_path(model_dir: &Path, entry: &ModelCatalogEntry) -> PathBuf {
    model_dir.join(entry.file_name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManagedModelRecord {
    pub catalog_id: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManagedModels {
    pub catalog_version: u32,
    #[serde(default)]
    pub models: BTreeMap<String, ManagedModelRecord>,
}

impl Default for ManagedModels {
    fn default() -> Self {
        Self {
            catalog_version: CATALOG_VERSION,
            models: BTreeMap::new(),
        }
    }
}

#[must_use]
pub fn metadata_path(model_dir: &Path) -> PathBuf {
    model_dir.join(METADATA_FILE)
}

pub fn load_managed_models(model_dir: &Path) -> Result<ManagedModels, std::io::Error> {
    let path = metadata_path(model_dir);
    if !path.exists() {
        return Ok(ManagedModels::default());
    }
    let bytes = fs::read(&path)?;
    let metadata: ManagedModels = serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if metadata.catalog_version != CATALOG_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "unsupported managed-model metadata version {}",
                metadata.catalog_version
            ),
        ));
    }
    Ok(metadata)
}

pub fn save_managed_models(
    model_dir: &Path,
    metadata: &ManagedModels,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(model_dir)?;
    let path = metadata_path(model_dir);
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(metadata).map_err(std::io::Error::other)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

pub fn record_managed_model(
    model_dir: &Path,
    entry: &ModelCatalogEntry,
) -> Result<(), std::io::Error> {
    let mut metadata = load_managed_models(model_dir)?;
    metadata.models.insert(
        entry.id.to_owned(),
        ManagedModelRecord {
            catalog_id: entry.id.to_owned(),
            file_name: entry.file_name.to_owned(),
            size: entry.expected_size,
            sha256: entry.sha256.to_owned(),
        },
    );
    save_managed_models(model_dir, &metadata)
}

#[must_use]
pub fn catalog_entry_for_path(model_dir: &Path, path: &Path) -> Option<&'static ModelCatalogEntry> {
    CATALOG
        .iter()
        .find(|entry| model_file_path(model_dir, entry) == path)
}

#[must_use]
pub fn tilde_model_path(model_dir_tilde: &str, entry: &ModelCatalogEntry) -> String {
    format!(
        "{}/{}",
        model_dir_tilde.trim_end_matches('/'),
        entry.file_name
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCandidate {
    pub id: &'static str,
    pub path: PathBuf,
}

impl ModelCandidate {
    #[must_use]
    pub fn entry(&self) -> Option<&'static ModelCatalogEntry> {
        catalog_entry(self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRecommendation {
    pub hardware_profile: String,
    pub final_model_id: String,
    pub preview_model_id: Option<String>,
    pub asr_gpu: bool,
    pub lifecycle_mode: String,
    pub warm_on_daemon_start: bool,
    pub install_commands: Vec<String>,
    pub select_commands: Vec<String>,
    pub tradeoffs: Vec<String>,
    pub warnings: Vec<String>,
}

#[must_use]
pub fn recommend_model_profile(
    profile: &SystemProfile,
    cuda_build: bool,
    include_preview: bool,
) -> ModelRecommendation {
    if profile.has_nvidia_gpu && cuda_build && profile.gpu_vram_mib.unwrap_or(0) >= 2_048 {
        return ModelRecommendation {
            hardware_profile: "power-user-nvidia".into(),
            final_model_id: "large-v3-turbo-q5".into(),
            preview_model_id: include_preview.then(|| "small.en-q5".into()),
            asr_gpu: true,
            lifecycle_mode: "keep_warm".into(),
            warm_on_daemon_start: true,
            install_commands: install_commands(
                "large-v3-turbo-q5",
                include_preview.then_some("small.en-q5"),
            ),
            select_commands: select_commands(
                "large-v3-turbo-q5",
                include_preview.then_some("small.en-q5"),
            ),
            tradeoffs: vec![
                "Fastest stop-to-text path on CUDA with the best catalog accuracy.".into(),
                "Keeps the final model warm for low latency and uses more idle RAM/VRAM.".into(),
                "Text preview is supported with the smaller CUDA preview model.".into(),
            ],
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();
    if profile.has_nvidia_gpu && !cuda_build {
        warnings.push("NVIDIA hardware was detected, but the daemon is not CUDA-enabled.".into());
    } else if profile.has_nvidia_gpu && profile.gpu_vram_mib.unwrap_or(0) < 2_048 {
        warnings.push(
            "NVIDIA hardware was detected, but reported VRAM is below the CUDA profile threshold."
                .into(),
        );
    } else if profile.ram_total_mib == 0 {
        warnings.push("System RAM could not be detected; using CPU-safe defaults.".into());
    }

    ModelRecommendation {
        hardware_profile: "cpu-safe".into(),
        final_model_id: "small.en".into(),
        preview_model_id: include_preview.then(|| "small.en".into()),
        asr_gpu: false,
        lifecycle_mode: "on_demand".into(),
        warm_on_daemon_start: false,
        install_commands: install_commands("small.en", None),
        select_commands: select_commands("small.en", include_preview.then_some("small.en")),
        tradeoffs: vec![
            "Runs without CUDA and avoids GPU build requirements.".into(),
            "Lower idle memory with on-demand loading, with slower first transcription.".into(),
            "Good English accuracy; preview can reuse the same CPU-safe model.".into(),
        ],
        warnings,
    }
}

fn install_commands(final_model_id: &str, preview_model_id: Option<&str>) -> Vec<String> {
    let mut commands = vec![format!("skald models install {final_model_id}")];
    if let Some(preview_model_id) = preview_model_id.filter(|id| *id != final_model_id) {
        commands.push(format!("skald models install {preview_model_id}"));
    }
    commands
}

fn select_commands(final_model_id: &str, preview_model_id: Option<&str>) -> Vec<String> {
    let mut commands = vec![format!("skald models select {final_model_id}")];
    if let Some(preview_model_id) = preview_model_id {
        commands.push(format!("skald models select-preview {preview_model_id}"));
    }
    commands.push(format!(
        "skald config profile {}",
        if final_model_id == "large-v3-turbo-q5" {
            "power-user-nvidia"
        } else {
            "cpu-safe"
        }
    ));
    commands
}

#[must_use]
pub fn recommended_candidates(
    paths: &PathsConfig,
    profile: &SystemProfile,
    cuda_build: bool,
    include_preview: bool,
) -> Vec<ModelCandidate> {
    let model_dir = resolve_model_dir(paths);
    let mut ids = Vec::new();

    if profile.has_nvidia_gpu && cuda_build {
        if profile.gpu_vram_mib.unwrap_or(0) >= 2_048 {
            ids.push("small.en-q5");
            ids.push("large-v3-turbo-q5");
        } else {
            ids.push("small.en-q5");
        }
        ids.push("small.en");
    } else {
        ids.push("base.en");
        ids.push("small.en");
    }

    if include_preview && !ids.contains(&"small.en-q5") && cuda_build && profile.has_nvidia_gpu {
        ids.push("small.en-q5");
    } else if include_preview && !ids.contains(&"small.en") {
        ids.push("small.en");
    }

    ids.sort_unstable();
    ids.dedup();

    ids.into_iter()
        .filter_map(|id| {
            let entry = catalog_entry(id)?;
            Some(ModelCandidate {
                id: entry.id,
                path: model_file_path(&model_dir, entry),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_profile_recommends_cpu_models() {
        let profile = SystemProfile {
            cpu_logical_cores: 8,
            ram_total_mib: 16_384,
            has_nvidia_gpu: false,
            gpu_name: None,
            gpu_vram_mib: None,
            model_dir_free_mib: Some(10_000),
            distro_id: Some("arch".into()),
            audio_stack_available: true,
            cuda_daemon_build: Some(false),
        };
        let paths = PathsConfig::default();
        let candidates = recommended_candidates(&paths, &profile, false, false);
        let ids: Vec<_> = candidates.iter().map(|c| c.id).collect();
        assert!(ids.contains(&"base.en"));
        assert!(ids.contains(&"small.en"));
    }

    #[test]
    fn cpu_profile_recommends_concrete_cpu_safe_plan() {
        let profile = SystemProfile {
            cpu_logical_cores: 8,
            ram_total_mib: 16_384,
            has_nvidia_gpu: false,
            gpu_name: None,
            gpu_vram_mib: None,
            model_dir_free_mib: Some(10_000),
            distro_id: Some("arch".into()),
            audio_stack_available: true,
            cuda_daemon_build: Some(false),
        };
        let recommendation = recommend_model_profile(&profile, false, true);
        assert_eq!(recommendation.hardware_profile, "cpu-safe");
        assert_eq!(recommendation.final_model_id, "small.en");
        assert_eq!(recommendation.preview_model_id.as_deref(), Some("small.en"));
        assert!(!recommendation.asr_gpu);
        assert_eq!(recommendation.lifecycle_mode, "on_demand");
        assert!(
            recommendation
                .select_commands
                .contains(&"skald models select small.en".into())
        );
    }

    #[test]
    fn cuda_profile_recommends_power_user_plan() {
        let profile = SystemProfile {
            cpu_logical_cores: 24,
            ram_total_mib: 32_768,
            has_nvidia_gpu: true,
            gpu_name: Some("NVIDIA GeForce RTX 3070 Ti".into()),
            gpu_vram_mib: Some(8_192),
            model_dir_free_mib: Some(10_000),
            distro_id: Some("arch".into()),
            audio_stack_available: true,
            cuda_daemon_build: Some(true),
        };
        let recommendation = recommend_model_profile(&profile, true, true);
        assert_eq!(recommendation.hardware_profile, "power-user-nvidia");
        assert_eq!(recommendation.final_model_id, "large-v3-turbo-q5");
        assert_eq!(
            recommendation.preview_model_id.as_deref(),
            Some("small.en-q5")
        );
        assert!(recommendation.asr_gpu);
        assert_eq!(recommendation.lifecycle_mode, "keep_warm");
    }

    #[test]
    fn catalog_has_unique_valid_entries() {
        let mut ids = std::collections::BTreeSet::new();
        let mut files = std::collections::BTreeSet::new();
        for entry in CATALOG {
            assert!(ids.insert(entry.id));
            assert!(files.insert(entry.file_name));
            assert!(entry.expected_size > 0);
            assert_eq!(entry.sha256.len(), 64);
            assert!(entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(download_url(entry).starts_with("https://"));
        }
    }

    #[test]
    fn managed_metadata_round_trips() {
        let directory =
            std::env::temp_dir().join(format!("skald-model-metadata-{}", ulid::Ulid::new()));
        let entry = &CATALOG[0];
        record_managed_model(&directory, entry).unwrap();
        let metadata = load_managed_models(&directory).unwrap();
        assert_eq!(metadata.catalog_version, CATALOG_VERSION);
        assert_eq!(metadata.models[entry.id].file_name, entry.file_name);
        let _ = fs::remove_dir_all(directory);
    }
}
