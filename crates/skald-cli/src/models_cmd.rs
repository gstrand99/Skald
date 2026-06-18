use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Subcommand, builder::PossibleValuesParser};
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use skald_core::{
    config::Config,
    download::{download_model, verify_model_file},
    models::{
        CATALOG, catalog_entry, load_managed_models, model_file_path, recommended_candidates,
        record_managed_model, save_managed_models,
    },
    paths::{expand_home, resolve_model_dir, to_tilde},
    protocol::{Command, ModelState},
    system_probe::probe_system,
};

use crate::send;

#[derive(Debug, Subcommand)]
pub enum ModelsCommands {
    List,
    Recommend,
    Install {
        #[arg(value_parser = model_ids())]
        model: String,
        #[arg(long, conflicts_with = "select_preview")]
        select: bool,
        #[arg(long, conflicts_with = "select")]
        select_preview: bool,
    },
    Verify {
        #[arg(value_parser = model_ids())]
        model: Option<String>,
    },
    Select {
        #[arg(value_parser = model_ids())]
        model: String,
    },
    SelectPreview {
        #[arg(value_parser = model_ids())]
        model: String,
    },
    Remove {
        #[arg(value_parser = model_ids())]
        model: String,
        #[arg(long)]
        yes: bool,
    },
    Prune {
        #[arg(long)]
        yes: bool,
    },
}

fn model_ids() -> PossibleValuesParser {
    PossibleValuesParser::new(CATALOG.iter().map(|entry| entry.id))
}

pub async fn run(command: &ModelsCommands, json: bool) -> Result<()> {
    match command {
        ModelsCommands::List => list(json).await,
        ModelsCommands::Recommend => recommend(json).await,
        ModelsCommands::Install {
            model,
            select: select_final,
            select_preview,
        } => install(model, *select_final, *select_preview, json).await,
        ModelsCommands::Verify { model } => verify(model.as_deref(), json).await,
        ModelsCommands::Select { model } => select(model, false, json).await,
        ModelsCommands::SelectPreview { model } => select(model, true, json).await,
        ModelsCommands::Remove { model, yes } => remove(model, *yes, json, true).await,
        ModelsCommands::Prune { yes } => prune(*yes, json).await,
    }
}

#[derive(Serialize)]
struct ModelListItem {
    id: String,
    state: String,
    integrity: String,
    size_mib: u64,
    intended_use: String,
    hardware_guidance: String,
    recommended: bool,
    selected_final: bool,
    selected_preview: bool,
}

fn configured_paths(config: &Config) -> [PathBuf; 2] {
    [
        expand_home(&config.asr.model_path),
        expand_home(&config.preview.effective_model_path()),
    ]
}

fn is_configured_model(config: &Config, path: &PathBuf) -> bool {
    configured_paths(config)
        .iter()
        .any(|configured| configured == path)
}

async fn cuda_build() -> Option<bool> {
    send(Command::Status)
        .await
        .ok()
        .and_then(|response| response.status)
        .map(|status| status.asr_gpu_build)
}

fn recommendations(config: &Config, cuda: bool) -> Vec<String> {
    let model_dir = resolve_model_dir(&config.paths);
    let profile = probe_system(&model_dir);
    recommended_candidates(&config.paths, &profile, cuda, true)
        .into_iter()
        .map(|candidate| candidate.id.to_owned())
        .collect()
}

async fn list(json: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let metadata = load_managed_models(&model_dir)?;
    let recommended = recommendations(&config, cuda_build().await.unwrap_or(false));
    let final_path = expand_home(&config.asr.model_path);
    let preview_path = expand_home(&config.preview.effective_model_path());
    let mut items = Vec::new();
    for entry in CATALOG {
        let path = model_file_path(&model_dir, entry);
        let state = if metadata.models.contains_key(entry.id) && path.is_file() {
            "managed"
        } else if path.is_file() {
            "unverified"
        } else {
            "missing"
        };
        let integrity = if path.is_file() {
            match verify_model_file(&path, entry.expected_size, entry.sha256).await {
                Ok(()) => "verified",
                Err(_) => "invalid",
            }
        } else {
            "-"
        };
        items.push(ModelListItem {
            id: entry.id.into(),
            state: state.into(),
            integrity: integrity.into(),
            size_mib: entry.approx_size_mib,
            intended_use: entry.intended_use.into(),
            hardware_guidance: entry.hardware_guidance.into(),
            recommended: recommended.iter().any(|id| id == entry.id),
            selected_final: path == final_path,
            selected_preview: path == preview_path,
        });
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    println!(
        "{:<24} {:<12} {:<12} {:>9}  Labels",
        "Model", "State", "Checksum", "Size MiB"
    );
    for item in items {
        let mut labels = vec![item.intended_use];
        if item.recommended {
            labels.push("recommended".into());
        }
        if item.selected_final {
            labels.push("selected-final".into());
        }
        if item.selected_preview {
            labels.push("selected-preview".into());
        }
        println!(
            "{:<24} {:<12} {:<12} {:>9}  {}",
            item.id,
            item.state,
            item.integrity,
            item.size_mib,
            labels.join(", ")
        );
    }
    for path in configured_paths(&config) {
        if CATALOG
            .iter()
            .all(|entry| model_file_path(&model_dir, entry) != path)
        {
            println!(
                "{:<24} {:<12} {:<12} {:>10}  configured path: {}",
                "(custom)",
                "unverified",
                "unknown",
                "-",
                path.display()
            );
        }
    }
    Ok(())
}

async fn recommend(json: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let mut profile = probe_system(&model_dir);
    profile.cuda_daemon_build = cuda_build().await;
    let candidates = recommended_candidates(
        &config.paths,
        &profile,
        profile.cuda_daemon_build == Some(true),
        true,
    );
    let ids: Vec<_> = candidates.iter().map(|candidate| candidate.id).collect();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "system": profile,
                "recommended_models": ids,
                "final": ids.iter().find(|id| **id == "large-v3-turbo-q5").or_else(|| ids.iter().find(|id| **id == "small.en")),
                "preview": ids.iter().find(|id| **id == "small.en-q5").or_else(|| ids.iter().find(|id| **id == "small.en")),
            }))?
        );
    } else {
        println!("Detected recommendations:");
        for candidate in candidates {
            let entry = candidate.entry().context("catalog entry missing")?;
            println!(
                "  {} — {} ({})",
                entry.id, entry.description, entry.hardware_guidance
            );
        }
        if profile.has_nvidia_gpu && profile.cuda_daemon_build != Some(true) {
            println!(
                "Warning: NVIDIA hardware was detected, but the running daemon is not CUDA-enabled."
            );
        }
    }
    Ok(())
}

async fn install(id: &str, select_final: bool, select_preview: bool, json: bool) -> Result<()> {
    let entry = catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?;
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let destination = model_file_path(&model_dir, entry);
    if !json {
        println!(
            "{} requires {} MiB. {}",
            entry.id, entry.approx_size_mib, entry.hardware_guidance
        );
    }
    let bar = if json {
        ProgressBar::hidden()
    } else {
        ProgressBar::new(entry.expected_size)
    };
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {bytes}/{total_bytes}")?
            .progress_chars("=>-"),
    );
    bar.set_message(entry.file_name.to_owned());
    let progress = |downloaded, total| {
        if let Some(total) = total {
            bar.set_length(total);
        }
        bar.set_position(downloaded);
    };
    download_model(entry, &destination, Some(&progress)).await?;
    bar.finish_with_message(format!("Installed {}", entry.id));
    record_managed_model(&model_dir, entry)?;
    if select_final {
        select(id, false, json).await?;
    } else if select_preview {
        select(id, true, json).await?;
    } else if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"model": id, "installed": true}))?
        );
    }
    Ok(())
}

async fn verify(id: Option<&str>, json: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let entries: Vec<_> = match id {
        Some(id) => vec![catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?],
        None => CATALOG.iter().collect(),
    };
    let mut failed = false;
    let mut results = Vec::new();
    for entry in entries {
        let path = model_file_path(&model_dir, entry);
        if !path.is_file() {
            results.push(serde_json::json!({"model": entry.id, "status": "missing"}));
            if !json {
                println!("{}: missing", entry.id);
            }
            failed = true;
            continue;
        }
        match verify_model_file(&path, entry.expected_size, entry.sha256).await {
            Ok(()) => {
                results.push(serde_json::json!({"model": entry.id, "status": "verified"}));
                if !json {
                    println!("{}: verified", entry.id);
                }
            }
            Err(error) => {
                results.push(serde_json::json!({"model": entry.id, "status": "invalid", "error": error.to_string()}));
                if !json {
                    println!("{}: {error}", entry.id);
                }
                failed = true;
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    if failed {
        bail!("one or more models failed verification");
    }
    Ok(())
}

async fn select(id: &str, preview: bool, json: bool) -> Result<()> {
    let entry = catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?;
    let mut config = Config::load_validated()?;
    let model_dir = resolve_model_dir(&config.paths);
    let path = model_file_path(&model_dir, entry);
    if !path.is_file() {
        bail!("model is not installed; run `skald models install {id}`");
    }
    let cuda = cuda_build().await;
    if entry.gpu && cuda != Some(true) {
        eprintln!(
            "Warning: {id} is intended for CUDA use, but the running daemon is not CUDA-enabled or unavailable."
        );
    }
    let configured = to_tilde(&path, &model_dir, &config.paths.model_dir);
    if preview {
        config.preview.model_path = configured;
    } else {
        config.asr.model_path = configured;
    }
    let path = config.save()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "model": id, "target": if preview { "preview" } else { "final" },
                "config_path": path, "restart_required": true
            }))?
        );
    } else {
        println!(
            "Selected {id} for {} in {}",
            if preview { "preview" } else { "final ASR" },
            path.display()
        );
        println!("Restart skaldd to load the selected model.");
    }
    Ok(())
}

async fn daemon_has_loaded_model() -> bool {
    send(Command::AsrStatus)
        .await
        .ok()
        .and_then(|response| response.status)
        .is_some_and(|status| status.final_model_state != ModelState::Unloaded)
}

async fn remove(id: &str, yes: bool, json: bool, emit: bool) -> Result<()> {
    let entry = catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?;
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let path = model_file_path(&model_dir, entry);
    if is_configured_model(&config, &path) {
        bail!("refusing to remove {id}: it is referenced by active configuration");
    }
    if daemon_has_loaded_model().await {
        bail!("refusing to remove {id}: skaldd has a model loaded; run `skald asr unload` first");
    }
    let mut metadata = load_managed_models(&model_dir)?;
    if !metadata.models.contains_key(id) {
        bail!("refusing to remove {id}: it is not recorded as managed by Skald");
    }
    if !yes
        && !Confirm::new()
            .with_prompt(format!("Remove {}?", path.display()))
            .default(false)
            .interact()?
    {
        return Ok(());
    }
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    metadata.models.remove(id);
    save_managed_models(&model_dir, &metadata)?;
    if json && emit {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"model": id, "removed": true}))?
        );
    } else if emit {
        println!("Removed {id}");
    }
    Ok(())
}

async fn prune(yes: bool, json: bool) -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let configured = configured_paths(&config);
    let metadata = load_managed_models(&model_dir)?;
    let unused: Vec<_> = metadata
        .models
        .keys()
        .filter(|id| {
            catalog_entry(id).is_some_and(|entry| {
                let path = model_file_path(&model_dir, entry);
                !configured.iter().any(|configured| configured == &path)
            })
        })
        .cloned()
        .collect();
    if unused.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No unused managed models.");
        }
        return Ok(());
    }
    if !json {
        println!("Unused managed models: {}", unused.join(", "));
    }
    let removed = unused.clone();
    for id in unused {
        remove(&id, yes, json, false).await?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&removed)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_paths_keep_final_and_preview_separate() {
        let mut config = Config::default();
        config.asr.model_path = "/tmp/final.bin".into();
        config.preview.model_path = "/tmp/preview.bin".into();
        let paths = configured_paths(&config);
        assert_eq!(paths[0], PathBuf::from("/tmp/final.bin"));
        assert_eq!(paths[1], PathBuf::from("/tmp/preview.bin"));
        assert!(is_configured_model(&config, &paths[0]));
        assert!(is_configured_model(&config, &paths[1]));
        assert!(!is_configured_model(
            &config,
            &PathBuf::from("/tmp/unused.bin")
        ));
    }
}
