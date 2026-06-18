use std::path::{Path, PathBuf};

use thiserror::Error;

pub const SERVICE_UNIT_NAME: &str = "skaldd.service";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("systemd user config directory is unavailable")]
    ConfigDirectoryUnavailable,
    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("service unit is not installed at {0}")]
    NotInstalled(PathBuf),
}

#[must_use]
pub fn service_unit_path() -> Option<PathBuf> {
    dirs::config_dir().map(|config| config.join("systemd/user").join(SERVICE_UNIT_NAME))
}

#[must_use]
pub fn render_service_unit(exec_start: &str, log_level: &str) -> String {
    format!(
        "[Unit]
Description=Skald local dictation daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart={exec_start}
Restart=on-failure
RestartSec=2
Environment=RUST_LOG={log_level}

[Install]
WantedBy=graphical-session.target
"
    )
}

pub fn write_service_unit(
    path: &Path,
    exec_start: &str,
    log_level: &str,
) -> Result<(), ServiceError> {
    let parent = path
        .parent()
        .ok_or(ServiceError::ConfigDirectoryUnavailable)?;
    std::fs::create_dir_all(parent).map_err(|source| ServiceError::Io {
        action: "create",
        path: parent.to_path_buf(),
        source,
    })?;
    std::fs::write(path, render_service_unit(exec_start, log_level)).map_err(|source| {
        ServiceError::Io {
            action: "write",
            path: path.to_path_buf(),
            source,
        }
    })
}

pub fn remove_service_unit(path: &Path) -> Result<(), ServiceError> {
    if !path.is_file() {
        return Err(ServiceError::NotInstalled(path.to_path_buf()));
    }
    std::fs::remove_file(path).map_err(|source| ServiceError::Io {
        action: "remove",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_systemd_unit() {
        let unit = render_service_unit("/home/user/.local/bin/skaldd", "info");
        assert!(unit.contains("ExecStart=/home/user/.local/bin/skaldd"));
        assert!(unit.contains("Environment=RUST_LOG=info"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
    }
}
