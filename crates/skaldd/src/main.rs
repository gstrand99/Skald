mod asr;
mod audio;
mod bench;
mod cleanup;
mod delivery;
mod diagnostics;
mod dictation;
mod injection;
mod ipc;
mod jobs;
mod openrouter;
mod preview;
mod preview_asr;
mod template_extract;

use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    os::unix::fs::{FileTypeExt, OpenOptionsExt},
    path::Path,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use skald_core::{
    build_info,
    config::{AutoPasteMode, Config},
    protocol::{DaemonStatus, ModelState},
    runtime::{ensure_runtime_dir_for, secure_socket_permissions, socket_path_for},
};
use tokio::{
    net::UnixListener,
    signal,
    sync::{Mutex, RwLock, broadcast},
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::jobs::AppState;

#[derive(Debug, Parser)]
#[command(version, about = "Skald local dictation daemon")]
struct Args {
    #[arg(long)]
    foreground: bool,
    #[arg(long)]
    build_info_json: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.build_info_json {
        let acceleration = if cfg!(feature = "asr-whisper-rs-cuda") {
            "cuda"
        } else if cfg!(feature = "asr-whisper-rs-metal") {
            "metal"
        } else {
            "cpu"
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&build_info::build_info(acceleration))?
        );
        return Ok(());
    }
    let config = Config::load_validated()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&config.daemon.log_level)),
        )
        .init();

    ensure_runtime_dir_for(&config.paths)?;
    let socket = socket_path_for(&config.paths)?;
    let _socket_lock = lock_socket(&socket)?;
    remove_stale_socket(&socket).await?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("failed to bind {}", socket.display()))?;
    secure_socket_permissions(&socket).context("failed to secure daemon socket permissions")?;
    let (events, _) = broadcast::channel(32);
    let audio_gates = config.audio.gates.clone();
    let paste_available = skald_platform::paste_backend().is_some();
    let auto_paste_effective = match (&config.injection.auto_paste, paste_available) {
        (AutoPasteMode::Off, _) | (_, false) => "clipboard_only",
        (AutoPasteMode::Safe, true) => "safe",
        (AutoPasteMode::Always, true) => "always",
    };
    let preview_enabled = config.preview_enabled_effective();
    let preview_asr = preview_enabled
        .then(|| preview_asr::PreviewAsrManager::spawn(&config.preview, &config.asr));
    let mut preview_config = config.preview.clone();
    preview_config.enabled = preview_enabled;
    let state = Arc::new(AppState {
        status: RwLock::new(DaemonStatus {
            cleanup_enabled: config.cleanup.enabled,
            asr_gpu_build: cfg!(any(
                feature = "asr-whisper-rs-cuda",
                feature = "asr-whisper-rs-metal"
            )),
            auto_paste_effective: auto_paste_effective.into(),
            preview_model_state: preview_enabled.then_some(ModelState::Unloaded),
            ..DaemonStatus::default()
        }),
        events,
        preview: preview::PreviewCoordinator::new(preview_config),
        preview_asr,
        audio: audio::AudioRecorder::spawn(config.audio, config.paths.clone()),
        asr: asr::AsrManager::spawn(config.asr, config.vocabulary),
        audio_gates,
        injection: config.injection,
        notifications: config.notifications,
        privacy: config.privacy,
        diagnostics: Mutex::new(skald_core::diagnostics::DiagnosticsStore::new(
            config.diagnostics.enabled,
            config.diagnostics.max_records,
        )),
        target_at_start: Mutex::new(None),
        cleanup_override: Mutex::new(None),
        style_override: Mutex::new(None),
        active_app_profile: Mutex::new(None),
        job_config: Mutex::new(None),
    });

    info!(path = %socket.display(), "skaldd listening");
    loop {
        tokio::select! {
            incoming = listener.accept() => {
                let (stream, _) = incoming?;
                if ipc::reject_foreign_peer(&stream) {
                    continue;
                }
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(error) = ipc::handle_client(stream, state).await {
                        warn!(%error, "client connection failed");
                    }
                });
            }
            result = signal::ctrl_c() => {
                result?;
                info!("shutdown requested");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(socket);
    Ok(())
}
fn lock_socket(path: &Path) -> Result<File> {
    let lock_path = path.with_extension("lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .with_context(|| format!("failed to open daemon lock {}", lock_path.display()))?;
    lock.try_lock()
        .with_context(|| format!("daemon socket is in use: {}", path.display()))?;
    // Keep this file on disk: unlinking a lock permits locking a different inode.
    Ok(lock)
}

async fn remove_stale_socket(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("failed to inspect daemon socket"),
    };
    anyhow::ensure!(
        metadata.file_type().is_socket(),
        "refusing to remove non-socket path: {}",
        path.display()
    );
    // Also protect running daemons from older releases that do not hold a lock.
    match tokio::time::timeout(
        Duration::from_millis(250),
        tokio::net::UnixStream::connect(path),
    )
    .await
    {
        Ok(Err(error)) if error.kind() == ErrorKind::ConnectionRefused => {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to remove stale socket {}", path.display()))?;
        }
        Ok(Err(error)) if error.kind() == ErrorKind::NotFound => {}
        Ok(Err(error)) => {
            return Err(error).context("cannot determine whether daemon socket is stale");
        }
        Ok(Ok(_)) | Err(_) => anyhow::bail!("daemon socket is in use: {}", path.display()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::MetadataExt, net::UnixListener as StdUnixListener};

    fn test_socket_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "skald-startup-{}.sock",
            skald_core::protocol::JobId::new().0
        ))
    }

    #[tokio::test]
    async fn startup_preserves_a_live_socket() {
        let path = test_socket_path();
        let listener = StdUnixListener::bind(&path).expect("bind existing daemon");
        let inode = std::fs::metadata(&path).expect("socket metadata").ino();
        assert!(remove_stale_socket(&path).await.is_err());
        assert_eq!(
            std::fs::metadata(&path).expect("socket preserved").ino(),
            inode
        );
        tokio::net::UnixStream::connect(&path)
            .await
            .expect("existing daemon still reachable");
        drop(listener);
        std::fs::remove_file(path).expect("remove socket");
    }

    #[tokio::test]
    async fn startup_removes_a_stale_socket() {
        let path = test_socket_path();
        drop(StdUnixListener::bind(&path).expect("bind stale socket"));
        remove_stale_socket(&path)
            .await
            .expect("remove stale socket");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn startup_preserves_an_unexpected_file() {
        let path = test_socket_path();
        std::fs::write(&path, "keep this file").expect("write unexpected file");
        assert!(remove_stale_socket(&path).await.is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("file preserved"),
            "keep this file"
        );
        std::fs::remove_file(path).expect("remove test file");
    }

    #[test]
    fn startup_lock_excludes_a_second_daemon_until_released() {
        let path = test_socket_path();
        let first = lock_socket(&path).expect("first daemon lock");
        assert!(lock_socket(&path).is_err());
        drop(first);
        let next = lock_socket(&path).expect("lock released after daemon exits");
        drop(next);
        std::fs::remove_file(path.with_extension("lock")).expect("remove test lock");
    }
}
