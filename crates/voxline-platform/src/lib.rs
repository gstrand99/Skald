use std::{
    env,
    io::Write,
    process::{Command, Stdio},
};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("no supported clipboard tool is available (install wl-clipboard or xclip)")]
    ClipboardUnavailable,
    #[error("failed to start {tool}: {source}")]
    Start {
        tool: &'static str,
        source: std::io::Error,
    },
    #[error("failed to write clipboard text to {tool}: {source}")]
    Write {
        tool: &'static str,
        source: std::io::Error,
    },
    #[error("failed to open stdin for {tool}")]
    StdinUnavailable { tool: &'static str },
    #[error("{tool} exited unsuccessfully")]
    Failed { tool: &'static str },
}

pub fn copy_to_clipboard(text: &str) -> Result<(), PlatformError> {
    let (tool, args): (&'static str, &[&str]) = if command_exists("wl-copy") {
        ("wl-copy", &[])
    } else if command_exists("xclip") {
        ("xclip", &["-selection", "clipboard"])
    } else {
        return Err(PlatformError::ClipboardUnavailable);
    };
    let mut child = Command::new(tool)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|source| PlatformError::Start { tool, source })?;
    child
        .stdin
        .take()
        .ok_or(PlatformError::StdinUnavailable { tool })?
        .write_all(text.as_bytes())
        .map_err(|source| PlatformError::Write { tool, source })?;
    let status = child
        .wait()
        .map_err(|source| PlatformError::Start { tool, source })?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::Failed { tool })
    }
}

pub fn notify(summary: &str, body: &str) {
    if command_exists("notify-send")
        && let Err(error) = Command::new("notify-send").args([summary, body]).spawn()
    {
        tracing::warn!(%error, "failed to start notify-send");
    }
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct EnvironmentReport {
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub wayland_display_present: bool,
    pub display_present: bool,
    pub dbus_session_bus_present: bool,
    pub xdg_runtime_dir_present: bool,
    pub tools: Vec<ToolReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolReport {
    pub name: &'static str,
    pub available: bool,
}

#[must_use]
pub fn environment_report() -> EnvironmentReport {
    const TOOLS: &[&str] = &[
        "wtype",
        "xdotool",
        "ydotool",
        "hyprctl",
        "swaymsg",
        "wmctrl",
        "notify-send",
        "wl-copy",
        "wl-paste",
        "xclip",
    ];
    EnvironmentReport {
        session_type: env::var("XDG_SESSION_TYPE").ok(),
        desktop: env::var("XDG_CURRENT_DESKTOP").ok(),
        wayland_display_present: env::var_os("WAYLAND_DISPLAY").is_some(),
        display_present: env::var_os("DISPLAY").is_some(),
        dbus_session_bus_present: env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        xdg_runtime_dir_present: env::var_os("XDG_RUNTIME_DIR").is_some(),
        tools: TOOLS
            .iter()
            .map(|name| ToolReport {
                name,
                available: command_exists(name),
            })
            .collect(),
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "sh", name])
        .status()
        .is_ok_and(|status| status.success())
}
