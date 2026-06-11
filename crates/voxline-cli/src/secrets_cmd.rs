use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use dialoguer::Password;
use voxline_core::{
    config::Config,
    secrets::{self, OPENROUTER_SECRET_NAME},
};

pub fn run(command: SecretsCommands) -> Result<()> {
    match command {
        SecretsCommands::Set { provider } => set_secret(&provider),
        SecretsCommands::Clear { provider } => clear_secret(&provider),
        SecretsCommands::Status => status(),
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum SecretsCommands {
    Set { provider: String },
    Clear { provider: String },
    Status,
}

fn set_secret(provider: &str) -> Result<()> {
    if provider != OPENROUTER_SECRET_NAME {
        bail!("unsupported secret provider: {provider}");
    }
    let config = Config::load_validated()?;
    let key = if io::stdin().is_terminal() {
        Password::new()
            .with_prompt("OpenRouter API key")
            .interact()?
    } else {
        let mut key = String::new();
        io::stdin().read_line(&mut key)?;
        key.trim().to_owned()
    };
    let key = key.trim();
    if key.is_empty() {
        bail!("API key cannot be empty");
    }
    secrets::set_openrouter_key(&config.secrets, key).context("failed to store OpenRouter key")?;
    println!("Stored OpenRouter API key in the system keyring");
    Ok(())
}

fn clear_secret(provider: &str) -> Result<()> {
    if provider != OPENROUTER_SECRET_NAME {
        bail!("unsupported secret provider: {provider}");
    }
    secrets::clear_openrouter_key().context("failed to clear OpenRouter key")?;
    println!("Cleared OpenRouter API key from the system keyring");
    Ok(())
}

fn status() -> Result<()> {
    let config = Config::load_or_default()?;
    let report = secrets::secret_status(&config.secrets);
    println!("Secrets status");
    println!("  Keyring available: {}", yes_no(report.keyring_available));
    println!(
        "  Keyring configured: {}",
        yes_no(report.keyring_configured)
    );
    println!("  Env configured: {}", yes_no(report.env_configured));
    println!(
        "  Insecure file enabled: {}",
        yes_no(report.insecure_file_enabled)
    );
    println!(
        "  Insecure file configured: {}",
        yes_no(report.insecure_file_configured)
    );
    println!(
        "  OpenRouter configured: {}",
        yes_no(report.openrouter_configured)
    );
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
