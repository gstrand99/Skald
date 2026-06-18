use anyhow::{Context, Result, bail};
use skald_core::{
    config::Config,
    styles::{self, StyleError},
};

pub fn run(command: StylesCommands) -> Result<()> {
    match command {
        StylesCommands::List => list(),
        StylesCommands::New { name, description } => new(&name, description.as_deref()),
        StylesCommands::Edit { name } => edit(&name),
        StylesCommands::Validate { name } => validate(name.as_deref()),
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum StylesCommands {
    List,
    New {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    Edit {
        name: String,
    },
    Validate {
        name: Option<String>,
    },
}

fn list() -> Result<()> {
    let config = Config::load_validated()?;
    styles::ensure_default_style_files(&config.paths).context("failed to ensure default style")?;
    let entries = styles::list_styles(&config.paths).context("failed to list styles")?;
    if entries.is_empty() {
        println!("No cleanup styles configured.");
        return Ok(());
    }
    println!("Cleanup styles");
    for entry in entries {
        println!("  {} — {}", entry.name, entry.description);
    }
    Ok(())
}

fn new(name: &str, description: Option<&str>) -> Result<()> {
    let config = Config::load_validated()?;
    styles::create_style(&config.paths, name, description).context("failed to create style")?;
    let styles_dir = skald_core::paths::styles_dir(&config.paths);
    println!("Created style {name} in {}", styles_dir.display());
    println!("Edit the prompt with: skald styles edit {name}");
    Ok(())
}

fn edit(name: &str) -> Result<()> {
    let config = Config::load_validated()?;
    let path = styles::edit_style(&config.paths, name).context("failed to edit style")?;
    println!("Updated {}", path.display());
    Ok(())
}

fn validate(name: Option<&str>) -> Result<()> {
    let config = Config::load_or_default()?;
    styles::ensure_default_style_files(&config.paths).context("failed to ensure default style")?;
    if let Some(name) = name {
        styles::validate_style(&config.paths, name).map_err(map_style_error)?;
        println!("Style {name} is valid");
        return Ok(());
    }
    let issues = styles::validate_installed_styles(&config.paths);
    if issues.is_empty() {
        println!("All cleanup styles are valid");
        return Ok(());
    }
    for issue in issues {
        println!("{}: {}", issue.style, issue.message);
    }
    bail!("one or more cleanup styles are invalid");
}

fn map_style_error(error: StyleError) -> anyhow::Error {
    error.into()
}
