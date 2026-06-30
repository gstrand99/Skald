use toml::Value;

const REDACTED: &str = "[redacted]";

#[must_use]
pub fn redact_config_toml(input: &str) -> String {
    match input.parse::<Value>() {
        Ok(mut value) => {
            redact_toml_value(&mut value);
            toml::to_string_pretty(&value).unwrap_or_else(|_| redact_text(input))
        }
        Err(_) => redact_text(input),
    }
}

#[must_use]
pub fn redact_text(input: &str) -> String {
    input
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_toml_value(value: &mut Value) {
    match value {
        Value::Table(table) => {
            for (key, child) in table {
                if sensitive_key(key) {
                    *child = Value::String(REDACTED.into());
                } else {
                    redact_toml_value(child);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                redact_toml_value(child);
            }
        }
        _ => {}
    }
}

fn redact_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some((key, _)) = trimmed.split_once('=')
        && sensitive_key(key.trim())
    {
        let prefix_len = line.len() - trimmed.len();
        return format!("{}{}={}", &line[..prefix_len], key.trim_end(), REDACTED);
    }
    if looks_like_private_text_log(trimmed) {
        return format!("{}{}", &line[..line.len() - trimmed.len()], REDACTED);
    }
    redact_inline_assignments(&redact_tokens(line))
}

fn redact_inline_assignments(line: &str) -> String {
    let mut redacted = line.to_owned();
    for key in ["api_key", "openrouter_api_key", "text", "from", "to"] {
        let mut search_from = 0;
        let pattern = format!("{key} = \"");
        while let Some(relative_start) = redacted[search_from..].find(&pattern) {
            let start = search_from + relative_start;
            let value_start = start + key.len() + 4;
            let Some(relative_end) = redacted[value_start..].find('"') else {
                break;
            };
            let value_end = value_start + relative_end;
            redacted.replace_range(value_start..value_end, REDACTED);
            search_from = value_start + REDACTED.len();
        }
    }
    redacted
}

fn redact_tokens(line: &str) -> String {
    line.split_whitespace()
        .map(|word| {
            let bare =
                word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
            if looks_like_secret_token(bare) {
                word.replace(bare, REDACTED)
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sensitive_key(key: &str) -> bool {
    let key = key
        .trim_matches(|ch: char| ch == '"' || ch == '\'')
        .to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("apikey")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("password")
        || key.contains("credential")
        || key == "authorization"
        || key == "text"
        || key == "from"
        || key == "to"
}

fn looks_like_private_text_log(line: &str) -> bool {
    let key = line
        .split_once([':', '='])
        .map_or(line, |(key, _)| key)
        .to_ascii_lowercase();
    key.contains("transcript")
        || key.contains("clipboard")
        || key.contains("dictated_text")
        || key.contains("cleaned_text")
        || key.contains("raw_text")
}

fn looks_like_secret_token(value: &str) -> bool {
    value.starts_with("sk-or-")
        || value.starts_with("Bearer")
        || (value.len() >= 32
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_config_keys_without_hiding_shape() {
        let input = r#"
[cleanup]
provider = "openrouter"
api_key = "sk-or-secret"

[secrets]
allow_insecure_file_fallback = true
openrouter_api_key = "abc123"

[vocabulary]
phrases = [{ text = "private project name" }]
replacements = [{ from = "private shorthand", to = "private expansion" }]

[asr]
model_path = "/home/user/.local/share/skald/models/ggml-small.en.bin"
"#;

        let redacted = redact_config_toml(input);

        assert!(redacted.contains("[cleanup]"));
        assert!(redacted.contains("provider = \"openrouter\""));
        assert_eq!(redacted.matches("[redacted]").count(), 5);
        assert!(redacted.contains("model_path"));
        assert!(!redacted.contains("sk-or-secret"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("private project name"));
        assert!(!redacted.contains("private shorthand"));
    }

    #[test]
    fn redacts_log_tokens_and_private_text_lines() {
        let input = "\
INFO started
OPENROUTER_API_KEY=sk-or-v1-secret
transcript: private dictated words
clipboard=private clipboard contents
authorization: Bearer abcdefghijklmnopqrstuvwxyz123456";

        let redacted = redact_text(input);

        assert!(redacted.contains("INFO started"));
        assert!(redacted.contains("OPENROUTER_API_KEY=[redacted]"));
        assert!(redacted.contains("[redacted]"));
        assert!(!redacted.contains("private dictated words"));
        assert!(!redacted.contains("private clipboard contents"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz123456"));
    }
}
