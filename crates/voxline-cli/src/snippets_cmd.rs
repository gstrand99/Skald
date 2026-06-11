use anyhow::{Context, Result, bail};
use voxline_core::{
    config::Config,
    snippet_templates,
    snippets::{self, SnippetError, SnippetKind},
};

pub fn run(command: SnippetsCommands) -> Result<()> {
    match command {
        SnippetsCommands::List => list(),
        SnippetsCommands::New { name, template } => new(&name, template),
        SnippetsCommands::Validate { name } => validate(name.as_deref()),
        SnippetsCommands::Insert { .. } => unreachable!("insert is handled in main"),
        SnippetsCommands::Preview { name: _, text: _ } => {
            unreachable!("preview is handled in main")
        }
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum SnippetsCommands {
    List,
    New {
        name: String,
        #[arg(long)]
        template: bool,
    },
    Validate {
        name: Option<String>,
    },
    Insert {
        name: String,
    },
    Preview {
        name: String,
        text: String,
    },
}

fn list() -> Result<()> {
    let config = Config::load_validated()?;
    snippets::ensure_snippets_dir(&config.paths).context("failed to ensure snippets directory")?;
    let entries = snippets::list_snippets(&config.paths).context("failed to list snippets")?;
    if entries.is_empty() {
        println!("No snippets configured.");
        return Ok(());
    }
    println!("Snippets");
    for entry in entries {
        let kind = match entry.kind {
            SnippetKind::Insert => "insert",
            SnippetKind::Template => "template",
        };
        let aliases = if entry.aliases.is_empty() {
            "-".into()
        } else {
            entry.aliases.join(", ")
        };
        println!("  {} ({kind}) — aliases: {aliases}", entry.name);
    }
    Ok(())
}

fn new(name: &str, template: bool) -> Result<()> {
    let config = Config::load_validated()?;
    let snippets_dir = voxline_core::paths::snippets_dir(&config.paths);
    if template {
        snippet_templates::create_template_snippet(&config.paths, name)
            .context("failed to create template snippet")?;
        println!(
            "Created template snippet {name} in {}",
            snippets_dir.display()
        );
        println!("Edit template in: {}/{}.md", snippets_dir.display(), name);
    } else {
        snippets::create_snippet(&config.paths, name).context("failed to create snippet")?;
        println!(
            "Created insert snippet {name} in {}",
            snippets_dir.display()
        );
        println!("Edit content in: {}/{}.md", snippets_dir.display(), name);
    }
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
        println!("All snippets are valid");
        return Ok(());
    }
    for issue in issues {
        println!("{}: {}", issue.snippet, issue.message);
    }
    bail!("one or more snippets are invalid");
}

fn map_snippet_error(error: SnippetError) -> anyhow::Error {
    error.into()
}
