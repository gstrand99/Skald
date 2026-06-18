use std::{
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Select};
use indicatif::{ProgressBar, ProgressStyle};
use skald_core::{
    config::Config,
    download::{download_model, verify_model_file},
    models::{ModelCandidate, recommended_candidates, record_managed_model},
    paths::{resolve_model_dir, scaffold_config_layout},
    protocol::{AsrBenchCandidate, Command},
    service::SERVICE_UNIT_NAME,
    setup::{SetupSelection, mark_setup_complete, needs_setup, setup_fixture_path},
    system_probe::{SystemProfile, dependency_report, probe_system},
};
use skald_platform::trigger_guidance;

use crate::{print_response, send, service};

const RECORD_SECONDS: u64 = 10;

#[allow(clippy::struct_excessive_bools)]
pub struct SetupOptions {
    pub if_missing: bool,
    pub force: bool,
    pub non_interactive: bool,
    pub json: bool,
}

#[allow(clippy::too_many_lines)]
pub async fn run(options: SetupOptions) -> Result<()> {
    if std::env::var("SKALD_SKIP_SETUP").as_deref() == Ok("1") {
        return Ok(());
    }

    let config = Config::load_or_default()?;
    let config_path_exists = Config::path()?.is_file();

    if config_path_exists && !options.force {
        if options.if_missing {
            if needs_setup(&config.paths) {
                println!("Config file exists; skipping setup (--if-missing).");
            }
            return Ok(());
        }
        if options.non_interactive {
            bail!(
                "config file already exists at {}; pass --force to reconfigure",
                Config::path()?.display()
            );
        }
        let reconfigure = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Skald is already configured. Re-run setup?")
            .default(false)
            .interact()?;
        if !reconfigure {
            return Ok(());
        }
    }

    let model_dir = resolve_model_dir(&config.paths);
    let mut profile = probe_system(&model_dir);
    let cuda_build = detect_cuda_build().await;
    profile.cuda_daemon_build = cuda_build;

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": profile,
                "cuda_build": cuda_build,
            }))?
        );
    } else {
        print_profile(&profile, cuda_build);
    }

    let deps = dependency_report(profile.distro_id.as_deref());
    if !options.non_interactive && !deps.missing.is_empty() {
        println!("\nMissing dependencies:");
        for check in deps.checks.iter().filter(|check| !check.available) {
            if let Some(hint) = &check.install_hint {
                println!("  {} — {hint}", check.name);
            }
        }
        let show_hints = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Continue setup anyway?")
            .default(true)
            .interact()?;
        if !show_hints {
            return Ok(());
        }
    }

    if profile.has_nvidia_gpu && cuda_build != Some(true) && !options.non_interactive {
        println!("\nNVIDIA GPU detected but skaldd was not built with CUDA support.");
        println!("Rebuild with: just release-cuda");
        let continue_cpu = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Continue with CPU-only benchmarks?")
            .default(true)
            .interact()?;
        if !continue_cpu {
            bail!("setup cancelled; rebuild skaldd with CUDA support first");
        }
    }

    let preview_enabled = if options.non_interactive {
        false
    } else {
        Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enable live preview overlay? (downloads a small preview model)")
            .default(false)
            .interact()?
    };

    let help_models = if options.non_interactive {
        true
    } else {
        Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Download and benchmark candidate Whisper models?")
            .default(true)
            .interact()?
    };

    let daemon_guard = ensure_daemon().await?;
    let daemon_was_running = daemon_guard.child.is_none();

    let fixture = Config::ensure_setup_fixture_dir(&config.paths)?;
    record_fixture(RECORD_SECONDS, &fixture, options.non_interactive).await?;

    let cuda_ok = cuda_build == Some(true);
    let mut candidates = recommended_candidates(&config.paths, &profile, cuda_ok, preview_enabled);

    if help_models && !options.non_interactive {
        download_candidates(&candidates, &model_dir).await?;
    } else {
        candidates.retain(|candidate| candidate.path.is_file());
    }

    if candidates.is_empty() {
        bail!("no model files are available; run setup again and confirm model downloads");
    }

    let bench_results = benchmark_candidates(&fixture, &candidates).await?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&bench_results)?);
    } else {
        print_bench_table(&bench_results);
    }

    let successful: Vec<_> = bench_results
        .iter()
        .filter(|result| result.error.is_none())
        .collect();
    if successful.is_empty() {
        bail!("every model benchmark failed");
    }

    let selected_id = select_model(&successful, options.non_interactive)?;
    let selected = candidates
        .iter()
        .find(|candidate| candidate.id == selected_id)
        .context("selected model not found in candidate list")?;

    let cleanup_enabled = if options.non_interactive {
        false
    } else {
        Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enable OpenRouter cleanup? (sends transcript text to a cloud provider)")
            .default(false)
            .interact()?
    };

    let selected_entry = selected
        .entry()
        .context("selected model catalog entry missing")?;
    let lifecycle_mode = if selected_entry.gpu {
        "keep_warm"
    } else {
        "on_demand"
    };

    let selection = SetupSelection {
        asr_model_id: selected.id.to_string(),
        asr_model_path: selected.path.clone(),
        asr_gpu: selected_entry.gpu && cuda_ok,
        asr_threads: u16::try_from(profile.cpu_logical_cores.min(16)).unwrap_or(4),
        lifecycle_mode: lifecycle_mode.into(),
        warm_on_daemon_start: lifecycle_mode == "keep_warm",
        idle_unload_seconds: if lifecycle_mode == "keep_warm" {
            900
        } else {
            0
        },
        preview_enabled,
        preview_model_path: preview_enabled.then(|| {
            candidates
                .iter()
                .find(|c| c.id == "small.en-q5" || c.id == "small.en")
                .map_or_else(|| selected.path.clone(), |c| c.path.clone())
        }),
        preview_gpu: preview_enabled && cuda_ok,
        cleanup_enabled,
    };

    if !Config::path()?.is_file() {
        Config::init(false)?;
    }
    let mut final_config = Config::load_or_default()?;
    final_config = Config::from_setup_selection(final_config, &selection)?;
    final_config.save()?;
    mark_setup_complete(&final_config.paths, &selection)?;

    let mut service_action_taken = false;
    if !options.non_interactive {
        let install_service = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Install the systemd user service for skaldd?")
            .default(true)
            .interact()?;
        if install_service {
            service::install(&final_config.daemon.log_level)?;
            let restart_now = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Start or restart the skaldd service now?")
                .default(true)
                .interact()?;
            if restart_now {
                service::restart()?;
                service_action_taken = true;
            }
        }
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let trigger = trigger_guidance(&session, &desktop);
        println!(
            "\nBind a compositor shortcut to `{}`:",
            trigger.recommended_command
        );
        for line in trigger.binding_examples {
            println!("  {line}");
        }
    }

    if daemon_was_running && !service_action_taken {
        print_daemon_restart_warning();
    }

    if options.json {
        println!("{}", serde_json::to_string_pretty(&selection)?);
    } else {
        println!("\nSetup complete.");
        println!("ASR model: {}", selection.asr_model_id);
        println!("Config: {}", Config::path()?.display());
        println!("Run `skald doctor` to verify the installation.");
    }

    Ok(())
}

fn print_daemon_restart_warning() {
    println!();
    println!("Warning: skaldd is still running with the previous configuration.");
    println!(
        "Restart it with `skald service restart` or `systemctl --user restart {SERVICE_UNIT_NAME}`.",
    );
}

fn print_profile(profile: &SystemProfile, cuda_build: Option<bool>) {
    println!("Skald setup — system profile");
    println!("  CPU cores: {}", profile.cpu_logical_cores);
    println!("  RAM: {} MiB", profile.ram_total_mib);
    if let Some(name) = &profile.gpu_name {
        println!("  GPU: {name}");
        if let Some(vram) = profile.gpu_vram_mib {
            println!("  VRAM: {vram} MiB");
        }
    } else {
        println!("  GPU: none detected");
    }
    if let Some(free) = profile.model_dir_free_mib {
        println!("  Model dir free space: {free} MiB");
    }
    println!(
        "  CUDA daemon build: {}",
        match cuda_build {
            Some(true) => "yes",
            Some(false) => "no",
            None => "daemon not running",
        }
    );
}

async fn detect_cuda_build() -> Option<bool> {
    let response = send(Command::Status).await.ok()?;
    response.status.map(|status| status.asr_gpu_build)
}

struct DaemonGuard {
    child: Option<Child>,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn ensure_daemon() -> Result<DaemonGuard> {
    if send(Command::Status).await.is_ok() {
        return Ok(DaemonGuard { child: None });
    }
    let skaldd = find_skaldd()?;
    let child = ProcessCommand::new(skaldd)
        .arg("--foreground")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start skaldd for setup")?;
    for _ in 0..40 {
        if send(Command::Status).await.is_ok() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            return Ok(DaemonGuard { child: Some(child) });
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    bail!("skaldd did not become ready");
}

fn find_skaldd() -> Result<PathBuf> {
    if let Ok(path) = std::env::current_exe() {
        let sibling = path.with_file_name("skaldd");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    which_skaldd()
}

fn which_skaldd() -> Result<PathBuf> {
    let output = ProcessCommand::new("sh")
        .args(["-c", "command -v skaldd"])
        .output()
        .context("failed to locate skaldd")?;
    if !output.status.success() {
        bail!("skaldd not found in PATH; run `just install` or `just build-cuda` first");
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

async fn record_fixture(seconds: u64, fixture: &Path, non_interactive: bool) -> Result<()> {
    if fixture.is_file() && non_interactive {
        return Ok(());
    }
    if fixture.is_file() && !non_interactive {
        let reuse = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!(
                "Reuse existing setup recording at {}?",
                fixture.display()
            ))
            .default(true)
            .interact()?;
        if reuse {
            return Ok(());
        }
    }
    if !non_interactive {
        println!("\nRecording a {seconds}-second speech sample. Speak clearly when prompted.");
    }
    let response = send(Command::SetupRecord {
        seconds,
        output_path: fixture.to_path_buf(),
    })
    .await?;
    if !response.ok {
        print_response(&response)?;
        bail!("setup recording failed");
    }
    if let Some(recording) = &response.recording {
        println!(
            "Saved setup fixture ({} ms, rms {:.4}) to {}",
            recording.duration_ms,
            recording.rms_energy,
            fixture.display()
        );
    }
    Ok(())
}

async fn download_candidates(candidates: &[ModelCandidate], model_dir: &Path) -> Result<()> {
    for candidate in candidates {
        if candidate.path.is_file() {
            if let Some(entry) = candidate.entry() {
                verify_model_file(&candidate.path, entry.expected_size, entry.sha256)
                    .await
                    .with_context(|| {
                        format!("existing model failed verification: {}", entry.file_name)
                    })?;
                record_managed_model(model_dir, entry)?;
                println!("Already have verified {}", entry.file_name);
            }
            continue;
        }
        let Some(entry) = candidate.entry() else {
            continue;
        };
        let download = Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!(
                "Download {} (~{} MiB)?",
                entry.file_name, entry.approx_size_mib
            ))
            .default(true)
            .interact()?;
        if !download {
            continue;
        }
        let bar = ProgressBar::new(entry.approx_size_mib.saturating_mul(1024 * 1024));
        bar.set_style(
            ProgressStyle::with_template("{msg} [{bar:40}] {bytes}/{total_bytes}")
                .unwrap()
                .progress_chars("=>-"),
        );
        bar.set_message(entry.file_name.to_string());
        let bar_cb = |downloaded: u64, total: Option<u64>| {
            if let Some(total) = total {
                bar.set_length(total);
            }
            bar.set_position(downloaded);
        };
        download_model(entry, &candidate.path, Some(&bar_cb)).await?;
        record_managed_model(model_dir, entry)?;
        bar.finish_with_message(format!("Downloaded {}", entry.file_name));
    }
    let _ = model_dir;
    Ok(())
}

async fn benchmark_candidates(
    fixture: &Path,
    candidates: &[ModelCandidate],
) -> Result<Vec<skald_core::protocol::ModelBenchResult>> {
    let protocol_candidates: Vec<AsrBenchCandidate> = candidates
        .iter()
        .map(|candidate| AsrBenchCandidate {
            model_id: candidate.id.to_string(),
            model_path: candidate.path.clone(),
            gpu: candidate.entry().is_some_and(|entry| entry.gpu),
        })
        .collect();
    let response = send(Command::BenchModelCompare {
        audio_path: fixture.to_path_buf(),
        candidates: protocol_candidates,
        include_cold_load: true,
    })
    .await?;
    if !response.ok {
        print_response(&response)?;
        bail!("model comparison benchmark failed");
    }
    response
        .model_bench_results
        .context("daemon did not return model benchmark results")
}

fn truncate_transcript_preview(text: &str) -> String {
    const LIMIT: usize = 48;
    if text.chars().count() <= LIMIT {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(LIMIT).collect::<String>())
    }
}

fn print_bench_table(results: &[skald_core::protocol::ModelBenchResult]) {
    println!("\nModel benchmark results:");
    println!(
        "{:<22} {:>10} {:>12} {:>8}  Preview",
        "Model", "Cold load", "Transcribe", "Audio"
    );
    for result in results {
        if let Some(error) = &result.error {
            println!("{:<22} ERROR: {error}", result.model_id);
            continue;
        }
        let preview = truncate_transcript_preview(&result.transcript_text);
        println!(
            "{:<22} {:>8} ms {:>8} ms {:>6} ms  {preview}",
            result.model_id,
            result.cold_load_ms,
            result.warm_transcribe_ms,
            result.audio_duration_ms,
        );
    }
}

fn select_model(
    results: &[&skald_core::protocol::ModelBenchResult],
    non_interactive: bool,
) -> Result<String> {
    if non_interactive {
        let best = results
            .iter()
            .min_by_key(|result| result.warm_transcribe_ms)
            .context("no successful benchmark results")?;
        return Ok(best.model_id.clone());
    }
    let labels: Vec<String> = results
        .iter()
        .map(|result| {
            format!(
                "{} — {} ms transcribe, {} ms cold load",
                result.model_id, result.warm_transcribe_ms, result.cold_load_ms
            )
        })
        .collect();
    let default = results
        .iter()
        .min_by_key(|result| result.warm_transcribe_ms)
        .map_or(results[0].model_id.as_str(), |result| {
            result.model_id.as_str()
        });
    let default_idx = results
        .iter()
        .position(|result| result.model_id == default)
        .unwrap_or(0);
    let selection = Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Select the ASR model to install")
        .default(default_idx)
        .items(&labels)
        .interact()?;
    Ok(results[selection].model_id.clone())
}

pub async fn run_record(seconds: u64) -> Result<()> {
    let config = Config::load_or_default()?;
    scaffold_config_layout(&config.paths)?;
    let fixture = setup_fixture_path(&config.paths);
    let _daemon = ensure_daemon().await?;
    record_fixture(seconds, &fixture, false).await
}
