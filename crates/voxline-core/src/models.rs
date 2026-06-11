use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{config::PathsConfig, paths::resolve_model_dir, system_probe::SystemProfile};

pub const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub id: &'static str,
    pub file_name: &'static str,
    pub gpu: bool,
    pub approx_size_mib: u64,
    pub description: &'static str,
}

pub const CATALOG: &[ModelCatalogEntry] = &[
    ModelCatalogEntry {
        id: "base.en",
        file_name: "ggml-base.en.bin",
        gpu: false,
        approx_size_mib: 150,
        description: "Fast CPU baseline",
    },
    ModelCatalogEntry {
        id: "small.en",
        file_name: "ggml-small.en.bin",
        gpu: false,
        approx_size_mib: 500,
        description: "Quality CPU default",
    },
    ModelCatalogEntry {
        id: "small.en-q5",
        file_name: "ggml-small.en-q5_1.bin",
        gpu: true,
        approx_size_mib: 200,
        description: "Fast GPU model, good for preview",
    },
    ModelCatalogEntry {
        id: "large-v3-turbo-q5",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        gpu: true,
        approx_size_mib: 1_500,
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
}
