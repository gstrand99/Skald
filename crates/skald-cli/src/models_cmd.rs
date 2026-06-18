use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use skald_core::{
    config::Config,
    download::{download_model, verify_model_file},
    models::{
        CATALOG, catalog_entry, load_managed_models, model_file_path, record_managed_model,
        save_managed_models,
    },
    paths::{expand_home, resolve_model_dir, to_tilde},
    protocol::{Command, ModelState},
};

use crate::send;

#[derive(Debug, Subcommand)]
pub enum ModelsCommands {
    List,
    Install {
        model: String,
    },
    Verify {
        model: Option<String>,
    },
    Select {
        model: String,
    },
    SelectPreview {
        model: String,
    },
    Remove {
        model: String,
        #[arg(long)]
        yes: bool,
    },
    Prune {
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(command: &ModelsCommands) -> Result<()> {
    match command {
        ModelsCommands::List => list().await,
        ModelsCommands::Install { model } => install(model).await,
        ModelsCommands::Verify { model } => verify(model.as_deref()).await,
        ModelsCommands::Select { model } => select(model, false),
        ModelsCommands::SelectPreview { model } => select(model, true),
        ModelsCommands::Remove { model, yes } => remove(model, *yes).await,
        ModelsCommands::Prune { yes } => prune(*yes).await,
    }
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

async fn list() -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let metadata = load_managed_models(&model_dir)?;
    println!(
        "{:<24} {:<12} {:<12} {:>10}  Use",
        "Model", "State", "Checksum", "Size MiB"
    );
    for entry in CATALOG {
        let path = model_file_path(&model_dir, entry);
        let state = if metadata.models.contains_key(entry.id) && path.is_file() {
            "managed"
        } else if path.is_file() {
            "unverified"
        } else {
            "missing"
        };
        let checksum = if path.is_file() {
            match verify_model_file(&path, entry.expected_size, entry.sha256).await {
                Ok(()) => "verified",
                Err(_) => "invalid",
            }
        } else {
            "-"
        };
        println!(
            "{:<24} {:<12} {:<12} {:>10}  {}",
            entry.id, state, checksum, entry.approx_size_mib, entry.intended_use
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

async fn install(id: &str) -> Result<()> {
    let entry = catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?;
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let destination = model_file_path(&model_dir, entry);
    let bar = ProgressBar::new(entry.expected_size);
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
    Ok(())
}

async fn verify(id: Option<&str>) -> Result<()> {
    let config = Config::load_or_default()?;
    let model_dir = resolve_model_dir(&config.paths);
    let entries: Vec<_> = match id {
        Some(id) => vec![catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?],
        None => CATALOG.iter().collect(),
    };
    let mut failed = false;
    for entry in entries {
        let path = model_file_path(&model_dir, entry);
        if !path.is_file() {
            println!("{}: missing", entry.id);
            failed = true;
            continue;
        }
        match verify_model_file(&path, entry.expected_size, entry.sha256).await {
            Ok(()) => println!("{}: verified", entry.id),
            Err(error) => {
                println!("{}: {error}", entry.id);
                failed = true;
            }
        }
    }
    if failed {
        bail!("one or more models failed verification");
    }
    Ok(())
}

fn select(id: &str, preview: bool) -> Result<()> {
    let entry = catalog_entry(id).with_context(|| format!("unknown model ID: {id}"))?;
    let mut config = Config::load_validated()?;
    let model_dir = resolve_model_dir(&config.paths);
    let path = model_file_path(&model_dir, entry);
    if !path.is_file() {
        bail!("model is not installed; run `skald models install {id}`");
    }
    let configured = to_tilde(&path, &model_dir, &config.paths.model_dir);
    if preview {
        config.preview.model_path = configured;
    } else {
        config.asr.model_path = configured;
    }
    let path = config.save()?;
    println!(
        "Selected {id} for {} in {}",
        if preview { "preview" } else { "final ASR" },
        path.display()
    );
    println!("Restart skaldd to load the selected model.");
    Ok(())
}

async fn daemon_has_loaded_model() -> bool {
    send(Command::AsrStatus)
        .await
        .ok()
        .and_then(|response| response.status)
        .is_some_and(|status| status.final_model_state != ModelState::Unloaded)
}

async fn remove(id: &str, yes: bool) -> Result<()> {
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
    println!("Removed {id}");
    Ok(())
}

async fn prune(yes: bool) -> Result<()> {
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
        println!("No unused managed models.");
        return Ok(());
    }
    println!("Unused managed models: {}", unused.join(", "));
    for id in unused {
        remove(&id, yes).await?;
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
