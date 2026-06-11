use std::{
    env,
    io::Write,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
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
    #[error("failed to decode {tool} output: {message}")]
    InvalidOutput { tool: &'static str, message: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetBackend {
    X11,
    Hyprland,
    Sway,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TargetContext {
    pub backend: TargetBackend,
    pub id: String,
    pub app_id: Option<String>,
    pub title: Option<String>,
}

impl TargetContext {
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        const TERMINALS: &[&str] = &[
            "alacritty",
            "com.mitchellh.ghostty",
            "foot",
            "ghostty",
            "kitty",
            "konsole",
            "org.wezfurlong.wezterm",
            "terminal",
            "wezterm",
            "xterm",
        ];
        self.app_id.as_deref().is_some_and(|app_id| {
            let normalized = app_id.to_ascii_lowercase();
            TERMINALS
                .iter()
                .any(|terminal| normalized.contains(terminal))
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PasteReport {
    pub clipboard_available: bool,
    pub paste_available: bool,
    pub target_detection_available: bool,
    pub backend: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteBackend {
    X11,
    Hyprland,
    Wtype,
}

pub struct ClipboardSnapshot {
    text: Option<String>,
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

pub fn read_clipboard() -> Result<String, PlatformError> {
    let (tool, args): (&'static str, &[&str]) = if command_exists("wl-paste") {
        ("wl-paste", &["--no-newline"])
    } else if command_exists("xclip") {
        ("xclip", &["-selection", "clipboard", "-o"])
    } else {
        return Err(PlatformError::ClipboardUnavailable);
    };
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|source| PlatformError::Start { tool, source })?;
    if !output.status.success() {
        return Err(PlatformError::Failed { tool });
    }
    String::from_utf8(output.stdout).map_err(|error| PlatformError::InvalidOutput {
        tool,
        message: error.to_string(),
    })
}

#[must_use]
pub fn save_clipboard() -> ClipboardSnapshot {
    ClipboardSnapshot {
        text: read_clipboard().ok(),
    }
}

pub fn restore_clipboard(snapshot: ClipboardSnapshot) -> Result<(), PlatformError> {
    if let Some(text) = snapshot.text {
        copy_to_clipboard(&text)?;
    }
    Ok(())
}

#[must_use]
pub fn capture_active_target() -> Option<TargetContext> {
    let session = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if session == "x11" {
        return capture_x11_target();
    }
    if desktop.contains("hyprland") {
        return capture_hyprland_target();
    }
    if desktop.contains("sway") {
        return capture_sway_target();
    }
    None
}

#[must_use]
pub fn paste_backend() -> Option<PasteBackend> {
    let session = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if session == "x11" && command_exists("xdotool") {
        Some(PasteBackend::X11)
    } else if desktop.contains("hyprland") && command_exists("hyprctl") {
        Some(PasteBackend::Hyprland)
    } else if desktop.contains("sway") && command_exists("wtype") {
        Some(PasteBackend::Wtype)
    } else {
        None
    }
}

pub fn paste(backend: PasteBackend) -> Result<(), PlatformError> {
    if backend == PasteBackend::Hyprland {
        sync_primary_selection_best_effort();
    }
    let (tool, args): (&'static str, &[&str]) = match backend {
        PasteBackend::X11 => ("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
        PasteBackend::Hyprland => (
            "hyprctl",
            &["dispatch", "sendshortcut", "SHIFT,Insert,activewindow"],
        ),
        PasteBackend::Wtype => ("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"]),
    };
    let status = Command::new(tool)
        .args(args)
        .status()
        .map_err(|source| PlatformError::Start { tool, source })?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::Failed { tool })
    }
}

fn sync_primary_selection_best_effort() {
    if !command_exists("wl-copy") {
        tracing::debug!("wl-copy unavailable; skipping primary selection sync for Hyprland paste");
        return;
    }
    let Ok(text) = read_clipboard() else {
        tracing::debug!("could not read clipboard for primary selection sync");
        return;
    };
    let mut child = match Command::new("wl-copy")
        .args(["--primary"])
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            tracing::debug!(%error, "failed to start wl-copy for primary selection sync");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(text.as_bytes())
    {
        tracing::debug!(%error, "failed to write primary selection");
        return;
    }
    match child.wait() {
        Ok(status) if status.success() => {}
        Ok(_) => tracing::debug!("wl-copy --primary exited unsuccessfully"),
        Err(error) => tracing::debug!(%error, "failed to wait for wl-copy --primary"),
    }
}

pub fn wait_for_clipboard(delay_ms: u64) {
    thread::sleep(Duration::from_millis(delay_ms));
}

#[must_use]
pub fn classify_paste_backend(session_type: &str, desktop: &str) -> &'static str {
    let desktop = desktop.to_ascii_lowercase();
    if session_type == "x11" {
        "x11"
    } else if desktop.contains("hyprland") {
        "hyprland"
    } else if desktop.contains("sway") {
        "sway"
    } else if desktop.contains("gnome") {
        "gnome_wayland"
    } else if desktop.contains("kde") {
        "kde_wayland"
    } else {
        "unknown"
    }
}

#[must_use]
pub fn paste_reason_for_backend(
    backend: &str,
    paste_available: bool,
    target_detection_available: bool,
) -> String {
    match backend {
        "gnome_wayland" => "GNOME Wayland defaults to clipboard-only".into(),
        "kde_wayland" => "KDE Wayland defaults to clipboard-only".into(),
        _ if paste_available && target_detection_available => {
            "safe paste is available when the active target remains stable".into()
        }
        _ if !paste_available => "no supported paste tool is available".into(),
        _ => "active target detection is unavailable".into(),
    }
}

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlaySessionHint {
    pub id: &'static str,
    pub detail: &'static str,
    pub layer_shell_recommended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPlacementHint {
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
}

impl OverlayPlacementHint {
    #[must_use]
    pub fn monitor_local_x(&self) -> i32 {
        self.cursor_x - self.monitor_x
    }

    #[must_use]
    pub fn monitor_local_y(&self) -> i32 {
        self.cursor_y - self.monitor_y
    }

    /// Prefer placing the overlay below the cursor when there is more room beneath it.
    #[must_use]
    pub fn prefer_below_cursor(&self) -> bool {
        let local_y = self.monitor_local_y();
        local_y + 120 <= self.monitor_height || local_y < self.monitor_height / 2
    }
}

#[must_use]
pub fn capture_overlay_placement_hint() -> Option<OverlayPlacementHint> {
    let session = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if session == "x11" {
        return capture_x11_placement();
    }
    if desktop.contains("hyprland") {
        return capture_hyprland_placement();
    }
    None
}

pub fn overlay_session_hint() -> OverlaySessionHint {
    let environment = environment_report();
    let session = environment.session_type.as_deref().unwrap_or("unknown");
    let desktop = environment
        .desktop
        .as_deref()
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    if session == "wayland" && desktop.contains("gnome") {
        OverlaySessionHint {
            id: "gnome_wayland",
            detail: "GNOME Wayland/Mutter does not expose wlr-layer-shell; overlay uses a floating window",
            layer_shell_recommended: false,
        }
    } else if session == "wayland"
        && (desktop.contains("hyprland") || desktop.contains("sway") || desktop.contains("river"))
    {
        OverlaySessionHint {
            id: "layer_shell",
            detail: "wlroots-style compositor; layer-shell overlay recommended",
            layer_shell_recommended: true,
        }
    } else if session == "x11" {
        OverlaySessionHint {
            id: "x11",
            detail: "X11 session; overlay uses a floating window",
            layer_shell_recommended: false,
        }
    } else {
        OverlaySessionHint {
            id: "unknown",
            detail: "unknown session; overlay may have limited placement support",
            layer_shell_recommended: false,
        }
    }
}

#[must_use]
pub fn paste_report() -> PasteReport {
    let environment = environment_report();
    let desktop = environment.desktop.as_deref().unwrap_or("unknown");
    let session = environment.session_type.as_deref().unwrap_or("unknown");
    let clipboard_available = command_exists("wl-copy") || command_exists("xclip");
    let backend = classify_paste_backend(session, desktop);
    let paste_available = paste_backend().is_some();
    let target_detection_available = capture_active_target().is_some();
    let reason = paste_reason_for_backend(backend, paste_available, target_detection_available);
    PasteReport {
        clipboard_available,
        paste_available,
        target_detection_available,
        backend: backend.into(),
        reason,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SessionEnvironmentSnapshot {
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub wayland_display_present: bool,
    pub display_present: bool,
    pub dbus_session_bus_present: bool,
    pub xdg_runtime_dir_present: bool,
}

impl From<EnvironmentReport> for SessionEnvironmentSnapshot {
    fn from(report: EnvironmentReport) -> Self {
        Self {
            session_type: report.session_type,
            desktop: report.desktop,
            wayland_display_present: report.wayland_display_present,
            display_present: report.display_present,
            dbus_session_bus_present: report.dbus_session_bus_present,
            xdg_runtime_dir_present: report.xdg_runtime_dir_present,
        }
    }
}

impl From<&EnvironmentReport> for SessionEnvironmentSnapshot {
    fn from(report: &EnvironmentReport) -> Self {
        Self {
            session_type: report.session_type.clone(),
            desktop: report.desktop.clone(),
            wayland_display_present: report.wayland_display_present,
            display_present: report.display_present,
            dbus_session_bus_present: report.dbus_session_bus_present,
            xdg_runtime_dir_present: report.xdg_runtime_dir_present,
        }
    }
}

#[must_use]
pub fn session_environment_mismatch(
    cli: &SessionEnvironmentSnapshot,
    daemon: &SessionEnvironmentSnapshot,
) -> Option<String> {
    let cli_has_display = cli.display_present || cli.wayland_display_present;
    let daemon_has_display = daemon.display_present || daemon.wayland_display_present;
    if cli_has_display && !daemon_has_display {
        return Some("Likely systemd user environment import problem.".into());
    }
    if cli.dbus_session_bus_present && !daemon.dbus_session_bus_present {
        return Some("Daemon is missing DBUS_SESSION_BUS_ADDRESS.".into());
    }
    if cli.xdg_runtime_dir_present && !daemon.xdg_runtime_dir_present {
        return Some("Daemon is missing XDG_RUNTIME_DIR.".into());
    }
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggerGuidance {
    pub recommended_command: &'static str,
    pub push_to_talk_note: &'static str,
    pub binding_examples: Vec<String>,
    pub environment_import: Vec<String>,
}

#[must_use]
pub fn trigger_guidance(session_type: &str, desktop: &str) -> TriggerGuidance {
    let desktop = desktop.to_ascii_lowercase();
    let mut binding_examples = Vec::new();
    let mut environment_import = Vec::new();

    if desktop.contains("gnome") {
        binding_examples.push("GNOME Settings -> Keyboard -> Custom Shortcuts".into());
        binding_examples.push("Name: VoxLine Toggle".into());
        binding_examples.push("Command: voxline toggle".into());
    } else if desktop.contains("kde") {
        binding_examples.push("System Settings -> Shortcuts -> Custom Shortcut".into());
        binding_examples.push("Command/URL: voxline toggle".into());
    } else if desktop.contains("hyprland") {
        binding_examples.push("bind = $mainMod, SPACE, exec, voxline toggle".into());
        binding_examples.push("bind = $mainMod, V, exec, voxline start".into());
        binding_examples.push("bindr = $mainMod, V, exec, voxline stop".into());
        environment_import.push(
            "exec-once = systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP DBUS_SESSION_BUS_ADDRESS".into(),
        );
        environment_import.push("exec-once = systemctl --user start voxlined".into());
    } else if desktop.contains("sway") {
        binding_examples.push("bindsym $mod+space exec voxline toggle".into());
        binding_examples.push("bindsym $mod+v exec voxline start".into());
        binding_examples.push("bindsym --release $mod+v exec voxline stop".into());
        environment_import.push(
            "exec systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP DBUS_SESSION_BUS_ADDRESS".into(),
        );
        environment_import.push("exec systemctl --user start voxlined".into());
    } else if session_type == "x11" {
        binding_examples.push("Bind a desktop shortcut to: voxline toggle".into());
    } else {
        binding_examples.push("Bind an external shortcut to: voxline toggle".into());
        environment_import.push(
            "Ensure the user service inherits WAYLAND_DISPLAY, DISPLAY, XDG_CURRENT_DESKTOP, and DBUS_SESSION_BUS_ADDRESS".into(),
        );
    }

    TriggerGuidance {
        recommended_command: "voxline toggle",
        push_to_talk_note: "Push-to-talk is available when your compositor supports key-release bindings (voxline start / voxline stop).",
        binding_examples,
        environment_import,
    }
}

pub fn notify(summary: &str, body: &str) {
    if command_exists("notify-send")
        && let Err(error) = Command::new("notify-send").args([summary, body]).spawn()
    {
        tracing::warn!(%error, "failed to start notify-send");
    }
}

#[derive(Deserialize)]
struct HyprlandWindow {
    address: String,
    class: String,
    title: String,
}

#[derive(Deserialize)]
struct HyprlandMonitor {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn capture_hyprland_placement() -> Option<OverlayPlacementHint> {
    let pos = command_stdout("hyprctl", &["cursorpos"])?;
    let (cursor_x, cursor_y) = parse_xy_pair(&pos)?;
    let output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let monitors: Vec<HyprlandMonitor> = serde_json::from_slice(&output.stdout).ok()?;
    let monitor = monitors.into_iter().find(|monitor| {
        cursor_x >= monitor.x
            && cursor_x < monitor.x + monitor.width
            && cursor_y >= monitor.y
            && cursor_y < monitor.y + monitor.height
    })?;
    Some(OverlayPlacementHint {
        cursor_x,
        cursor_y,
        monitor_x: monitor.x,
        monitor_y: monitor.y,
        monitor_width: monitor.width,
        monitor_height: monitor.height,
    })
}

fn capture_x11_placement() -> Option<OverlayPlacementHint> {
    let output = command_stdout("xdotool", &["getmouselocation", "--shell"])?;
    let values = parse_xdotool_shell(&output);
    let cursor_x: i32 = values.get("X")?.parse().ok()?;
    let cursor_y: i32 = values.get("Y")?.parse().ok()?;
    let screen = values
        .get("SCREEN")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let (monitor_x, monitor_y, monitor_width, monitor_height) = x11_screen_geometry(screen)
        .unwrap_or_else(|| {
            // Without `xrandr`, only the primary screen size is known and offsets stay at the
            // origin. Multi-monitor X11 placement needs hardware validation.
            let geometry = command_stdout("xdotool", &["getdisplaygeometry"])
                .unwrap_or_else(|| "1920 1080".into());
            let mut parts = geometry.split_whitespace();
            let monitor_width = parts.next().and_then(|value| value.parse().ok());
            let monitor_height = parts.next().and_then(|value| value.parse().ok());
            (
                0,
                0,
                monitor_width.unwrap_or(1920),
                monitor_height.unwrap_or(1080),
            )
        });
    Some(OverlayPlacementHint {
        cursor_x: cursor_x.clamp(
            monitor_x,
            monitor_x.saturating_add(monitor_width.saturating_sub(1)),
        ),
        cursor_y: cursor_y.clamp(
            monitor_y,
            monitor_y.saturating_add(monitor_height.saturating_sub(1)),
        ),
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
    })
}

fn parse_xdotool_shell(output: &str) -> std::collections::HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn x11_screen_geometry(screen: usize) -> Option<(i32, i32, i32, i32)> {
    let output = command_stdout("xrandr", &["--current"])?;
    let screens = parse_xrandr_connected_geometries(&output);
    let (monitor_x, monitor_y, monitor_width, monitor_height) =
        screens.get(screen).or_else(|| screens.first()).copied()?;
    Some((monitor_x, monitor_y, monitor_width, monitor_height))
}

fn parse_xrandr_connected_geometries(output: &str) -> Vec<(i32, i32, i32, i32)> {
    let mut screens = Vec::new();
    for line in output.lines() {
        if !line.contains(" connected") {
            continue;
        }
        for token in line.split_whitespace() {
            if let Some((monitor_width, monitor_height, monitor_x, monitor_y)) =
                parse_xrandr_geometry_token(token)
            {
                screens.push((monitor_x, monitor_y, monitor_width, monitor_height));
                break;
            }
        }
    }
    screens
}

fn parse_xrandr_geometry_token(token: &str) -> Option<(i32, i32, i32, i32)> {
    let (size, offsets) = token.split_once('+')?;
    let (monitor_width, monitor_height) = size.split_once('x')?;
    let (monitor_x, monitor_y) = offsets.split_once('+')?;
    Some((
        monitor_width.parse().ok()?,
        monitor_height.parse().ok()?,
        monitor_x.parse().ok()?,
        monitor_y.parse().ok()?,
    ))
}

fn parse_xy_pair(text: &str) -> Option<(i32, i32)> {
    let mut parts = text.split([',', ' ']).filter(|part| !part.is_empty());
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    Some((x, y))
}

fn capture_hyprland_target() -> Option<TargetContext> {
    let output = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let window: HyprlandWindow = serde_json::from_slice(&output.stdout).ok()?;
    (!window.address.is_empty()).then(|| TargetContext {
        backend: TargetBackend::Hyprland,
        id: window.address,
        app_id: (!window.class.is_empty()).then_some(window.class),
        title: (!window.title.is_empty()).then_some(window.title),
    })
}

fn capture_x11_target() -> Option<TargetContext> {
    let id = command_stdout("xdotool", &["getactivewindow"])?;
    let title = command_stdout("xdotool", &["getwindowname", &id]);
    let app_id = command_stdout("xdotool", &["getactivewindow", "getwindowclassname"]);
    Some(TargetContext {
        backend: TargetBackend::X11,
        id,
        app_id,
        title,
    })
}

fn capture_sway_target() -> Option<TargetContext> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_tree", "-r"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tree: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let focused = find_focused(&tree)?;
    Some(TargetContext {
        backend: TargetBackend::Sway,
        id: focused.get("id")?.to_string(),
        app_id: focused
            .get("app_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        title: focused
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

fn find_focused(value: &serde_json::Value) -> Option<&serde_json::Value> {
    if value.get("focused").and_then(serde_json::Value::as_bool) == Some(true) {
        return Some(value);
    }
    for key in ["nodes", "floating_nodes"] {
        for child in value
            .get(key)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(focused) = find_focused(child) {
                return Some(focused);
            }
        }
    }
    None
}

fn command_stdout(tool: &'static str, args: &[&str]) -> Option<String> {
    let output = Command::new(tool).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_focused_node_in_nested_sway_tree() {
        let tree = serde_json::json!({
            "focused": false,
            "nodes": [{
                "focused": false,
                "nodes": [{
                    "focused": true,
                    "id": 42,
                    "app_id": "terminal",
                    "name": "shell"
                }]
            }]
        });
        assert_eq!(
            find_focused(&tree).and_then(|node| node["id"].as_u64()),
            Some(42)
        );
    }

    #[test]
    fn identifies_known_terminal_application_ids() {
        let target = TargetContext {
            backend: TargetBackend::Hyprland,
            id: "0x1".into(),
            app_id: Some("com.mitchellh.ghostty".into()),
            title: None,
        };
        assert!(target.is_terminal());
    }

    #[test]
    fn classifies_kde_wayland_backend() {
        assert_eq!(classify_paste_backend("wayland", "KDE"), "kde_wayland");
        assert_eq!(
            paste_reason_for_backend("kde_wayland", false, false),
            "KDE Wayland defaults to clipboard-only"
        );
    }

    #[test]
    fn classifies_gnome_wayland_backend() {
        assert_eq!(classify_paste_backend("wayland", "GNOME"), "gnome_wayland");
        assert_eq!(
            paste_reason_for_backend("gnome_wayland", false, false),
            "GNOME Wayland defaults to clipboard-only"
        );
    }

    #[test]
    fn detects_daemon_display_import_problem() {
        let cli = SessionEnvironmentSnapshot {
            session_type: Some("wayland".into()),
            desktop: Some("Hyprland".into()),
            wayland_display_present: true,
            display_present: false,
            dbus_session_bus_present: true,
            xdg_runtime_dir_present: true,
        };
        let daemon = SessionEnvironmentSnapshot {
            wayland_display_present: false,
            display_present: false,
            dbus_session_bus_present: true,
            xdg_runtime_dir_present: true,
            ..cli.clone()
        };
        assert_eq!(
            session_environment_mismatch(&cli, &daemon),
            Some("Likely systemd user environment import problem.".into())
        );
    }

    #[test]
    fn hyprland_trigger_guidance_includes_environment_import() {
        let guidance = trigger_guidance("wayland", "Hyprland");
        assert!(
            guidance
                .environment_import
                .iter()
                .any(|line| line.contains("import-environment"))
        );
        assert!(
            guidance
                .binding_examples
                .iter()
                .any(|line| line.contains("voxline toggle"))
        );
    }
}
