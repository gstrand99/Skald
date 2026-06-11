use std::collections::HashMap;

use thiserror::Error;

use crate::{
    config::{PathsConfig, VoiceCommandsConfig},
    snippets,
    styles::{self, StyleError},
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("voice command prefix cannot be empty when voice commands are enabled")]
    EmptyPrefix,
    #[error("failed to load command registry: {0}")]
    Registry(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandTarget {
    Style { name: String },
    Snippet { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCommand {
    pub alias: String,
    pub target: CommandTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandConflict {
    pub alias: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedVoiceCommand {
    pub matched_alias: String,
    pub target: CommandTarget,
    pub remainder: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRegistry {
    entries: Vec<RegisteredCommand>,
}

impl CommandRegistry {
    #[must_use]
    pub fn entries(&self) -> &[RegisteredCommand] {
        &self.entries
    }
}

pub fn build_command_registry(paths: &PathsConfig) -> Result<CommandRegistry, CommandError> {
    let mut entries = Vec::new();
    for style in styles::list_styles(paths)
        .map_err(|error: StyleError| CommandError::Registry(error.to_string()))?
    {
        entries.push(RegisteredCommand {
            alias: style.name.clone(),
            target: CommandTarget::Style { name: style.name },
        });
    }
    for snippet in
        snippets::list_snippets(paths).map_err(|error| CommandError::Registry(error.to_string()))?
    {
        let mut aliases = snippet.aliases;
        if !aliases.iter().any(|alias| alias == &snippet.name) {
            aliases.push(snippet.name.clone());
        }
        for alias in aliases {
            let alias = alias.trim().to_string();
            if alias.is_empty() {
                continue;
            }
            entries.push(RegisteredCommand {
                alias: alias.clone(),
                target: CommandTarget::Snippet {
                    name: snippet.name.clone(),
                },
            });
        }
    }
    entries.sort_by(|left, right| {
        word_count(&right.alias)
            .cmp(&word_count(&left.alias))
            .then_with(|| left.alias.cmp(&right.alias))
    });
    Ok(CommandRegistry { entries })
}

#[must_use]
pub fn detect_command_conflicts(registry: &CommandRegistry) -> Vec<CommandConflict> {
    let mut by_alias: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &registry.entries {
        let key = normalize_command_text(&entry.alias);
        let label = target_label(&entry.target);
        by_alias.entry(key).or_default().push(label);
    }
    let mut conflicts = Vec::new();
    for (alias, mut targets) in by_alias {
        targets.sort();
        targets.dedup();
        if targets.len() > 1 {
            conflicts.push(CommandConflict { alias, targets });
        }
    }
    conflicts.sort_by(|left, right| left.alias.cmp(&right.alias));
    conflicts
}

#[must_use]
pub fn parse_voice_command(
    config: &VoiceCommandsConfig,
    registry: &CommandRegistry,
    transcript: &str,
) -> Option<ParsedVoiceCommand> {
    if !config.enabled {
        return None;
    }
    let prefix = config.prefix.trim();
    if prefix.is_empty() {
        return None;
    }
    let words = split_words(transcript.trim());
    if words.is_empty() {
        return None;
    }
    let prefix_words = normalize_words(prefix);
    if prefix_words.is_empty() {
        return None;
    }
    let normalized: Vec<String> = words.iter().map(|word| normalize_word(word)).collect();
    let rest_start = match_prefix_end(&normalized, &prefix_words)?;
    for entry in &registry.entries {
        let alias_words = normalize_words(&entry.alias);
        if alias_words.is_empty() || normalized.len() < rest_start + alias_words.len() {
            continue;
        }
        if normalized[rest_start..rest_start + alias_words.len()] == alias_words {
            let remainder = words[rest_start + alias_words.len()..].join(" ");
            return Some(ParsedVoiceCommand {
                matched_alias: entry.alias.clone(),
                target: entry.target.clone(),
                remainder,
            });
        }
    }
    None
}

pub fn validate_voice_commands(
    config: &VoiceCommandsConfig,
    paths: &PathsConfig,
) -> Result<(), CommandError> {
    if !config.enabled {
        return Ok(());
    }
    if config.prefix.trim().is_empty() {
        return Err(CommandError::EmptyPrefix);
    }
    let registry = build_command_registry(paths)?;
    if let Some(conflict) = detect_command_conflicts(&registry).into_iter().next() {
        return Err(CommandError::Registry(format!(
            "alias '{}' is used by: {}",
            conflict.alias,
            conflict.targets.join(", ")
        )));
    }
    Ok(())
}

#[must_use]
pub fn normalize_command_text(text: &str) -> String {
    normalize_words(text).join(" ")
}

fn split_words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn normalize_words(text: &str) -> Vec<String> {
    split_words(text)
        .into_iter()
        .map(normalize_word)
        .filter(|word| !word.is_empty())
        .collect()
}

fn normalize_word(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase()
}

fn match_prefix_end(normalized: &[String], prefix_words: &[String]) -> Option<usize> {
    if prefix_words.is_empty() || normalized.len() < prefix_words.len() {
        return None;
    }
    if normalized[..prefix_words.len()] == prefix_words[..] {
        return Some(prefix_words.len());
    }
    if prefix_words.len() == 1 {
        let target = &prefix_words[0];
        let mut accumulated = String::new();
        for (index, word) in normalized.iter().enumerate() {
            accumulated.push_str(word);
            if accumulated == *target {
                return Some(index + 1);
            }
            if !target.starts_with(&accumulated) {
                break;
            }
        }
    }
    None
}

fn word_count(text: &str) -> usize {
    normalize_words(text).len()
}

fn target_label(target: &CommandTarget) -> String {
    match target {
        CommandTarget::Style { name } => format!("style:{name}"),
        CommandTarget::Snippet { name } => format!("snippet:{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippets;
    use crate::styles;

    fn temp_paths(base: &std::path::Path) -> PathsConfig {
        PathsConfig {
            config_dir: base.join("config").display().to_string(),
            model_dir: base.join("models").display().to_string(),
            runtime_dir: base.join("runtime").display().to_string(),
        }
    }

    fn enabled_config() -> VoiceCommandsConfig {
        VoiceCommandsConfig {
            enabled: true,
            prefix: "voxline".into(),
        }
    }

    #[test]
    fn parses_prefix_when_asr_splits_voxline() {
        let base = std::env::temp_dir().join(format!("voxline-commands-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        styles::create_style(&paths, "professional", Some("Professional prose.")).unwrap();
        let registry = build_command_registry(&paths).unwrap();
        let parsed = parse_voice_command(
            &enabled_config(),
            &registry,
            "Vox Line professional hey John thanks",
        )
        .unwrap();
        assert_eq!(
            parsed.target,
            CommandTarget::Style {
                name: "professional".into()
            }
        );
        assert_eq!(parsed.remainder, "hey John thanks");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_style_command_with_remainder() {
        let base = std::env::temp_dir().join(format!("voxline-commands-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        styles::create_style(&paths, "professional", Some("Professional prose.")).unwrap();
        let registry = build_command_registry(&paths).unwrap();
        let parsed = parse_voice_command(
            &enabled_config(),
            &registry,
            "VoxLine professional hey John thanks",
        )
        .unwrap();
        assert_eq!(
            parsed.target,
            CommandTarget::Style {
                name: "professional".into()
            }
        );
        assert_eq!(parsed.remainder, "hey John thanks");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ignores_sentences_without_prefix() {
        let base = std::env::temp_dir().join(format!("voxline-commands-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        styles::create_style(&paths, "professional", None).unwrap();
        let registry = build_command_registry(&paths).unwrap();
        assert!(
            parse_voice_command(
                &enabled_config(),
                &registry,
                "professional mode is important here",
            )
            .is_none()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_snippet_command_and_detects_conflicts() {
        let base = std::env::temp_dir().join(format!("voxline-commands-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        styles::create_style(&paths, "signature", None).unwrap();
        snippets::create_snippet(&paths, "signature").unwrap();
        let registry = build_command_registry(&paths).unwrap();
        let conflicts = detect_command_conflicts(&registry);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].alias, "signature");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn matches_multi_word_snippet_alias() {
        let base = std::env::temp_dir().join(format!("voxline-commands-{}", ulid::Ulid::new()));
        let paths = temp_paths(&base);
        snippets::create_snippet(&paths, "signature").unwrap();
        let snippets_dir = crate::paths::snippets_dir(&paths);
        std::fs::write(
            snippets_dir.join("signature.toml"),
            r#"
name = "signature"
type = "insert"
aliases = ["signature", "insert signature"]
content_file = "signature.md"
"#,
        )
        .unwrap();
        let registry = build_command_registry(&paths).unwrap();
        let parsed =
            parse_voice_command(&enabled_config(), &registry, "voxline insert signature").unwrap();
        assert_eq!(
            parsed.target,
            CommandTarget::Snippet {
                name: "signature".into()
            }
        );
        assert!(parsed.remainder.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
