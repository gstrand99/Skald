use serde::{Deserialize, Serialize};

pub const DEFAULT_OPENROUTER_MODEL: &str = "~openai/gpt-mini-latest";

pub const CLEANUP_COST_WARNING: &str = "\
Cleanup sends transcript text to your configured provider and may cost money per request.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOverride {
    Force,
    Disable,
}

#[must_use]
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[must_use]
pub fn should_skip_cleanup(text: &str, skip_if_word_count_below: usize) -> bool {
    word_count(text) < skip_if_word_count_below
}

#[must_use]
pub fn should_run_cleanup(
    enabled: bool,
    override_mode: Option<CleanupOverride>,
    text: &str,
    skip_if_word_count_below: usize,
) -> bool {
    match override_mode {
        Some(CleanupOverride::Disable) => false,
        Some(CleanupOverride::Force) => true,
        None => enabled && !should_skip_cleanup(text, skip_if_word_count_below),
    }
}

#[must_use]
pub fn validate_cleanup_output(input: &str, output: &str) -> bool {
    let output = output.trim();
    if output.is_empty() {
        return false;
    }
    let lower = output.to_ascii_lowercase();
    for prefix in ["here is", "here's", "sure,", "certainly,", "as an ai"] {
        if lower.starts_with(prefix) {
            return false;
        }
    }
    let input_words = word_count(input).max(1);
    let output_words = word_count(output);
    output_words <= input_words * 3 + 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_short_utterances() {
        assert!(should_skip_cleanup("hey john", 5));
        assert!(!should_skip_cleanup(
            "hey john thanks for catching that bug",
            5
        ));
    }

    #[test]
    fn rejects_boilerplate_cleanup_output() {
        assert!(!validate_cleanup_output(
            "hey john thanks",
            "Here is the cleaned text: hello"
        ));
        assert!(validate_cleanup_output(
            "hey john thanks",
            "Hey John, thanks."
        ));
    }
}
