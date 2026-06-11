use anyhow::{Context, Result, bail};
use voxline_core::{
    config::Config,
    snippets::{self, SnippetError},
};

pub fn run(command: SnippetsCommands) -> Result<()> {
    match command {
        SnippetsCommands::List => list(),
        SnippetsCommands::New { name } => new(&name),
        SnippetsCommands::Validate { name } => validate(name.as_deref()),
        SnippetsCommands::Insert { .. } => unreachable!("insert is handled in main"),
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum SnippetsCommands {
    List,
    New { name: String },
    Validate { name: Option<String> },
    Insert { name: String },
}

fn list() -> Result<()> {
    let config = Config::load_or_default()?;
    snippets::ensure_snippets_dir(&config.paths).context("failed to ensure snippets directory")?;
    let entries = snippets::list_snippets(&config.paths).context("failed to list snippets")?;
    if entries.is_empty() {
        println!("No insert snippets configured.");
        return Ok(());
    }
    println!("Insert snippets");
    for entry in entries {
        let aliases = if entry.aliases.is_empty() {
            "-".into()
        } else {
            entry.aliases.join(", ")
        };
        println!("  {} — aliases: {aliases}", entry.name);
    }
    Ok(())
}

fn new(name: &str) -> Result<()> {
    let config = Config::load_or_default()?;
    snippets::create_snippet(&config.paths, name).context("failed to create snippet")?;
    let snippets_dir = voxline_core::paths::snippets_dir(&config.paths);
    println!("Created snippet {name} in {}", snippets_dir.display());
    println!("Edit content in: {}/{}.md", snippets_dir.display(), name);
    Ok(())
}

fn validate(name: Option<&str>) -> Result<()> {
    let config = Config::load_or_default()?;
    snippets::ensure_snippets_dir(&config.paths).context("failed to ensure snippets directory")?;
    if let Some(name) = name {
        snippets::validate_snippet(&config.paths, name).map_err(map_snippet_error)?;
        println!("Snippet {name} is valid");
        return Ok(());
    }
    let issues = snippets::validate_installed_snippets(&config.paths);
    if issues.is_empty() {
        println!("All insert snippets are valid");
        return Ok(());
    }
    for issue in issues {
        println!("{}: {}", issue.snippet, issue.message);
    }
    bail!("one or more insert snippets are invalid");
}

fn map_snippet_error(error: SnippetError) -> anyhow::Error {
    error.into()
}
