use std::path::{Path, PathBuf};

use thiserror::Error;

#[cfg(target_os = "macos")]
pub const SERVICE_UNIT_NAME: &str = "com.gstrand.skald.daemon";
#[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    return dirs::home_dir().map(|home| {
        home.join("Library/LaunchAgents")
            .join(format!("{SERVICE_UNIT_NAME}.plist"))
    });
    #[cfg(not(target_os = "macos"))]
    dirs::config_dir().map(|config| config.join("systemd/user").join(SERVICE_UNIT_NAME))
}

#[must_use]
pub fn render_service_unit(exec_start: &str, log_level: &str) -> String {
    #[cfg(target_os = "macos")]
    let exec_start = escape_xml(exec_start);
    #[cfg(target_os = "macos")]
    let log_level = escape_xml(log_level);
    #[cfg(target_os = "macos")]
    return format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>{SERVICE_UNIT_NAME}</string>\n<key>ProgramArguments</key><array><string>{exec_start}</string><string>--foreground</string></array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n<key>ProcessType</key><string>Interactive</string>\n<key>EnvironmentVariables</key><dict><key>RUST_LOG</key><string>{log_level}</string></dict>\n</dict></plist>\n"
    );
    #[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    #[cfg(not(target_os = "macos"))]
    fn renders_expected_systemd_unit() {
        let unit = render_service_unit("/home/user/.local/bin/skaldd", "info");
        assert!(unit.contains("ExecStart=/home/user/.local/bin/skaldd"));
        assert!(unit.contains("Environment=RUST_LOG=info"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn renders_expected_launch_agent() {
        let unit = render_service_unit("/Applications/Skald.app/Contents/MacOS/skaldd", "info");
        assert!(unit.contains("com.gstrand.skald.daemon"));
        assert!(unit.contains("RunAtLoad"));
        assert!(unit.contains("--foreground"));
    }
}
