use std::{
    path::{Path, PathBuf},
    process::Command,
};

use rustix::fs::statvfs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemProfile {
    pub cpu_logical_cores: usize,
    pub ram_total_mib: u64,
    pub has_nvidia_gpu: bool,
    pub gpu_name: Option<String>,
    pub gpu_vram_mib: Option<u64>,
    pub model_dir_free_mib: Option<u64>,
    pub distro_id: Option<String>,
    pub audio_stack_available: bool,
    pub cuda_daemon_build: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyCheck {
    pub name: String,
    pub available: bool,
    pub category: String,
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyReport {
    pub checks: Vec<DependencyCheck>,
    pub missing: Vec<String>,
}

#[must_use]
pub fn probe_system(model_dir: &Path) -> SystemProfile {
    let nvidia = nvidia_gpu_info();
    SystemProfile {
        cpu_logical_cores: std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(4),
        ram_total_mib: read_ram_total_mib(),
        has_nvidia_gpu: nvidia.is_some(),
        gpu_name: nvidia.as_ref().map(|(name, _)| name.clone()),
        gpu_vram_mib: nvidia.map(|(_, vram)| vram),
        model_dir_free_mib: free_space_mib(model_dir),
        distro_id: read_distro_id(),
        audio_stack_available: command_exists("pw-cli") || command_exists("pactl"),
        cuda_daemon_build: None,
    }
}

#[must_use]
pub fn dependency_report(distro_id: Option<&str>) -> DependencyReport {
    let session_type = std::env::var("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut checks = vec![
        dependency(
            "PipeWire or PulseAudio",
            command_exists("pw-cli") || command_exists("pactl"),
            "audio",
            distro_id,
            &["pipewire", "pulseaudio"],
        ),
        dependency(
            "wl-clipboard",
            command_exists("wl-copy"),
            "clipboard",
            distro_id,
            &["wl-clipboard"],
        ),
        dependency(
            "xclip",
            command_exists("xclip"),
            "clipboard",
            distro_id,
            &["xclip"],
        ),
        dependency(
            "notify-send",
            command_exists("notify-send"),
            "notifications",
            distro_id,
            &["libnotify"],
        ),
    ];

    if session_type == "wayland" && desktop.contains("hyprland") {
        checks.push(dependency(
            "hyprctl",
            command_exists("hyprctl"),
            "paste",
            distro_id,
            &["hyprland"],
        ));
    } else if session_type == "wayland" && desktop.contains("sway") {
        checks.push(dependency(
            "wtype",
            command_exists("wtype"),
            "paste",
            distro_id,
            &["wtype"],
        ));
    } else if session_type == "x11" {
        checks.push(dependency(
            "xdotool",
            command_exists("xdotool"),
            "paste",
            distro_id,
            &["xdotool"],
        ));
    }

    let missing = checks
        .iter()
        .filter(|check| !check.available)
        .map(|check| check.name.clone())
        .collect();
    DependencyReport { checks, missing }
}

fn dependency(
    name: &str,
    available: bool,
    category: &str,
    distro_id: Option<&str>,
    packages: &[&str],
) -> DependencyCheck {
    DependencyCheck {
        name: name.into(),
        available,
        category: category.into(),
        install_hint: if available {
            None
        } else {
            Some(package_hint(distro_id, packages))
        },
    }
}

fn package_hint(distro_id: Option<&str>, packages: &[&str]) -> String {
    let joined = packages.join(" ");
    match distro_id {
        Some("arch" | "manjaro") => format!("sudo pacman -S {joined}"),
        Some("fedora") => format!("sudo dnf install {joined}"),
        Some("ubuntu" | "debian" | "pop") => {
            format!("sudo apt install {joined}")
        }
        _ => format!("Install system packages: {joined}"),
    }
}

fn read_distro_id() -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    content
        .lines()
        .find_map(|line| line.strip_prefix("ID=").map(str::trim))
        .map(|id| id.trim_matches('"').to_ascii_lowercase())
}

fn read_ram_total_mib() -> u64 {
    let content = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            let kib = value
                .trim()
                .strip_suffix(" kB")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0);
            return kib / 1024;
        }
    }
    0
}

fn nearest_existing_path(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[must_use]
pub fn free_space_mib(path: &Path) -> Option<u64> {
    let path = nearest_existing_path(path)?;
    let stat = statvfs(path.as_os_str()).ok()?;
    // POSIX: f_bavail counts fragments of size f_frsize, not f_bsize.
    let free_bytes = stat.f_frsize * stat.f_bavail;
    Some(free_bytes / (1024 * 1024))
}

fn nvidia_gpu_info() -> Option<(String, u64)> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    let (name, vram) = line.split_once(',')?;
    let vram_mib = vram.trim().parse::<u64>().ok()?;
    Some((name.trim().to_owned(), vram_mib))
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
    fn probe_returns_positive_core_count() {
        let profile = probe_system(Path::new("/tmp"));
        assert!(profile.cpu_logical_cores > 0);
    }

    #[test]
    fn dependency_report_lists_audio_stack() {
        let report = dependency_report(Some("arch"));
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name.contains("PipeWire"))
        );
    }

    #[test]
    fn free_space_mib_reports_root_filesystem() {
        let free = free_space_mib(Path::new("/"));
        assert!(free.is_some_and(|mib| mib > 0));
    }

    #[test]
    fn free_space_mib_walks_to_existing_ancestor() {
        let deep = Path::new("/definitely-not-a-real-skald-path/deep/nested");
        let free = free_space_mib(deep);
        assert!(free.is_some_and(|mib| mib > 0));
    }
}
