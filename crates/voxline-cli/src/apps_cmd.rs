use anyhow::{Context, Result, bail};
use voxline_core::{
    apps::{self, AppError},
    config::Config,
};
use voxline_platform::{TargetBackend, capture_active_target};

pub fn run(command: AppsCommands) -> Result<()> {
    match command {
        AppsCommands::Detect => detect(),
        AppsCommands::List => list(),
        AppsCommands::Edit { name } => edit(&name),
        AppsCommands::Validate { name } => validate(name.as_deref()),
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum AppsCommands {
    Detect,
    List,
    Edit { name: String },
    Validate { name: Option<String> },
}

fn detect() -> Result<()> {
    let config = Config::load_validated()?;
    apps::ensure_default_app_profiles(&config.paths).context("failed to ensure app profiles")?;
    let target = capture_active_target();
    let report = if let Some(target) = &target {
        apps::detect_app_profile(
            &config.paths,
            backend_name(&target.backend),
            target.app_id.as_deref(),
            target.title.as_deref(),
        )
    } else {
        apps::detect_app_profile(&config.paths, "unknown", None, None)
    };
    println!("Active target");
    println!("  backend: {}", report.backend);
    println!("  app_id: {}", report.app_id.as_deref().unwrap_or("-"));
    println!("  title: {}", report.title.as_deref().unwrap_or("-"));
    println!(
        "  matched profile: {}",
        report.matched_profile.as_deref().unwrap_or("none")
    );
    Ok(())
}

fn list() -> Result<()> {
    let config = Config::load_validated()?;
    apps::ensure_default_app_profiles(&config.paths).context("failed to ensure app profiles")?;
    let profiles = apps::list_app_profiles(&config.paths).context("failed to list app profiles")?;
    if profiles.is_empty() {
        println!("No application profiles configured.");
        return Ok(());
    }
    println!("Application profiles");
    for profile in profiles {
        let style = profile
            .default_style
            .as_deref()
            .unwrap_or("(config default)");
        println!("  {} — style: {style}", profile.name);
    }
    Ok(())
}

fn edit(name: &str) -> Result<()> {
    let config = Config::load_validated()?;
    let path = apps::edit_app_profile(&config.paths, name).context("failed to edit app profile")?;
    println!("Updated {}", path.display());
    Ok(())
}

fn validate(name: Option<&str>) -> Result<()> {
    let config = Config::load_or_default()?;
    apps::ensure_default_app_profiles(&config.paths).context("failed to ensure app profiles")?;
    if let Some(name) = name {
        apps::validate_app_profile(&config.paths, name).map_err(map_app_error)?;
        println!("App profile {name} is valid");
        return Ok(());
    }
    let issues = apps::validate_installed_app_profiles(&config.paths);
    if issues.is_empty() {
        println!("All application profiles are valid");
        return Ok(());
    }
    for issue in issues {
        println!("{}: {}", issue.app, issue.message);
    }
    bail!("one or more application profiles are invalid");
}

fn backend_name(backend: &TargetBackend) -> &'static str {
    match backend {
        TargetBackend::X11 => "x11",
        TargetBackend::Hyprland => "hyprland",
        TargetBackend::Sway => "sway",
    }
}

fn map_app_error(error: AppError) -> anyhow::Error {
    error.into()
}
