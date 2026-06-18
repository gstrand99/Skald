use anyhow::{Result, bail};
use skald_core::{
    cleanup::{CLEANUP_COST_WARNING, DEFAULT_OPENROUTER_MODEL},
    config::Config,
};

pub fn enable(provider: &str) -> Result<()> {
    if provider != "openrouter" {
        bail!("unsupported cleanup provider: {provider}");
    }
    let mut config = Config::load_validated()?;
    config.cleanup.enabled = true;
    config.cleanup.provider = "openrouter".into();
    if config.cleanup.model.trim().is_empty() {
        config.cleanup.model = DEFAULT_OPENROUTER_MODEL.into();
    }
    config.validate()?;
    let path = config.save()?;
    println!("Enabled OpenRouter cleanup in {}", path.display());
    println!();
    println!("{CLEANUP_COST_WARNING}");
    println!("Cleanup adds latency and sends transcript text off-device.");
    println!("Restart skaldd if it is already running so status reflects the change.");
    Ok(())
}

pub fn disable() -> Result<()> {
    let mut config = Config::load_validated()?;
    config.cleanup.enabled = false;
    config.cleanup.provider = "none".into();
    let path = config.save()?;
    println!("Disabled cleanup in {}", path.display());
    Ok(())
}
