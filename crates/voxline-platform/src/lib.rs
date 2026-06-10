use std::{env, process::Command};

use serde::Serialize;

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
