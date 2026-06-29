use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use skald_core::service::{self, SERVICE_UNIT_NAME};
use skald_platform::trigger_guidance;

pub fn install(log_level: &str) -> Result<()> {
    install_inner(log_level, true)
}

pub fn install_quiet(log_level: &str) -> Result<()> {
    install_inner(log_level, false)
}

fn install_inner(log_level: &str, print_output: bool) -> Result<()> {
    let unit_path =
        service::service_unit_path().context("systemd user config directory unavailable")?;
    let skaldd = resolve_skaldd_path()?;
    service::write_service_unit(&unit_path, &skaldd.display().to_string(), log_level)
        .map_err(|error| anyhow::anyhow!(error))?;

    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", SERVICE_UNIT_NAME])?;

    if print_output {
        println!("Installed {}", unit_path.display());
        println!();
        println!("Start the service:");
        println!("  systemctl --user start {SERVICE_UNIT_NAME}");
        println!("  skald service start");
        println!();
        println!("Check status:");
        println!("  systemctl --user status {SERVICE_UNIT_NAME}");
        println!("  skald service status");
        println!();
        print_trigger_guidance();
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let unit_path =
        service::service_unit_path().context("systemd user config directory unavailable")?;
    let _ = run_systemctl(&["disable", "--now", SERVICE_UNIT_NAME]);
    service::remove_service_unit(&unit_path).map_err(|error| match error {
        service::ServiceError::NotInstalled(_) => {
            anyhow::anyhow!("service unit is not installed at {}", unit_path.display())
        }
        other => anyhow::anyhow!(other),
    })?;
    run_systemctl(&["daemon-reload"])?;
    println!("Removed {}", unit_path.display());
    Ok(())
}

pub fn start() -> Result<()> {
    run_systemctl(&["start", SERVICE_UNIT_NAME])?;
    println!("Started {SERVICE_UNIT_NAME}");
    Ok(())
}

pub fn restart() -> Result<()> {
    run_systemctl(&["restart", SERVICE_UNIT_NAME])?;
    println!("Restarted {SERVICE_UNIT_NAME}");
    Ok(())
}

pub fn restart_quiet() -> Result<()> {
    run_systemctl(&["restart", SERVICE_UNIT_NAME])
}

pub fn stop() -> Result<()> {
    run_systemctl(&["stop", SERVICE_UNIT_NAME])?;
    println!("Stopped {SERVICE_UNIT_NAME}");
    Ok(())
}

pub fn status() -> Result<()> {
    Command::new("systemctl")
        .args(["--user", "status", SERVICE_UNIT_NAME])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run systemctl --user status")?;
    Ok(())
}

pub fn print_trigger_guidance() {
    let environment = skald_platform::environment_report();
    let session = environment.session_type.as_deref().unwrap_or("unknown");
    let desktop = environment.desktop.as_deref().unwrap_or("unknown");
    let guidance = trigger_guidance(session, desktop);

    println!("Shortcut binding:");
    println!("  Recommended command: {}", guidance.recommended_command);
    println!("  {}", guidance.push_to_talk_note);
    for line in &guidance.binding_examples {
        println!("  {line}");
    }
    if !guidance.environment_import.is_empty() {
        println!();
        println!("Session environment import:");
        for line in &guidance.environment_import {
            println!("  {line}");
        }
    }
}

fn resolve_skaldd_path() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to resolve current executable")?;
    let sibling = current.with_file_name("skaldd");
    if sibling.is_file() {
        return Ok(sibling);
    }
    let output = Command::new("sh")
        .args(["-c", "command -v skaldd"])
        .output()
        .context("failed to search PATH for skaldd")?;
    if output.status.success() {
        let path = String::from_utf8(output.stdout)
            .context("invalid UTF-8 from command -v")?
            .trim()
            .to_owned();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    bail!("could not find skaldd binary; build or install it next to skald");
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .with_context(|| format!("failed to run systemctl --user {}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        bail!("systemctl --user {} exited with {}", args.join(" "), status);
    }
}
