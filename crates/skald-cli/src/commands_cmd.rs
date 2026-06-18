use anyhow::{Context, Result, bail};
use skald_core::{
    commands::{self, CommandError, CommandTarget},
    config::Config,
};

pub fn run(command: CommandsCommands) -> Result<()> {
    match command {
        CommandsCommands::Test { text } => test(&text),
        CommandsCommands::Conflicts => conflicts(),
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum CommandsCommands {
    Test { text: String },
    Conflicts,
}

fn test(text: &str) -> Result<()> {
    let config = Config::load_or_default()?;
    if !config.voice_commands.enabled {
        println!("Voice commands are disabled (experimental).");
        println!("Enable with [voice_commands] enabled = true in config.toml");
        return Ok(());
    }
    commands::validate_voice_commands(&config.voice_commands, &config.paths)
        .map_err(map_command_error)?;
    let registry =
        commands::build_command_registry(&config.paths).context("failed to build registry")?;
    if let Some(parsed) = commands::parse_voice_command(&config.voice_commands, &registry, text) {
        println!("Voice command recognized (experimental)");
        println!("  prefix: {}", config.voice_commands.prefix);
        println!("  alias: {}", parsed.matched_alias);
        print_target(&parsed.target);
        if parsed.remainder.trim().is_empty() {
            println!("  remainder: (empty)");
        } else {
            println!("  remainder: {}", parsed.remainder);
        }
    } else {
        println!("No voice command recognized");
        println!("  remainder: {text}");
    }
    Ok(())
}

fn conflicts() -> Result<()> {
    let config = Config::load_or_default()?;
    let registry =
        commands::build_command_registry(&config.paths).context("failed to build registry")?;
    let issues = commands::detect_command_conflicts(&registry);
    if issues.is_empty() {
        println!("No voice command alias conflicts detected");
        return Ok(());
    }
    println!("Voice command alias conflicts");
    for issue in issues {
        println!("  {} — {}", issue.alias, issue.targets.join(", "));
    }
    if config.voice_commands.enabled {
        bail!("resolve alias conflicts before enabling voice commands");
    }
    Ok(())
}

fn print_target(target: &CommandTarget) {
    match target {
        CommandTarget::Style { name } => println!("  target: style {name}"),
        CommandTarget::Snippet { name } => println!("  target: snippet {name}"),
    }
}

fn map_command_error(error: CommandError) -> anyhow::Error {
    error.into()
}
