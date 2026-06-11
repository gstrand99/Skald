use crate::config::VocabularyConfig;

/// Apply configured whole-word vocabulary replacements to `input`.
#[must_use]
pub fn apply_vocabulary_replacements(input: &str, vocabulary: &VocabularyConfig) -> String {
    if !vocabulary.enabled || !vocabulary.post_replace_enabled {
        return input.to_owned();
    }
    vocabulary
        .replacements
        .iter()
        .fold(input.to_owned(), |text, rule| {
            replace_whole_words(&text, &rule.from, &rule.to, rule.case_sensitive)
        })
}

#[must_use]
pub fn replace_whole_words(input: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    if from.is_empty() {
        return input.to_owned();
    }
    let haystack = if case_sensitive {
        input.to_owned()
    } else {
        input.to_ascii_lowercase()
    };
    let needle = if case_sensitive {
        from.to_owned()
    } else {
        from.to_ascii_lowercase()
    };
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = haystack[cursor..].find(&needle) {
        let start = cursor + relative;
        let end = start + needle.len();
        let left_boundary = start == 0
            || !input[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let right_boundary = end == input.len()
            || !input[end..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if left_boundary && right_boundary {
            output.push_str(&input[cursor..start]);
            output.push_str(to);
            cursor = end;
        } else {
            let next = input[start..]
                .chars()
                .next()
                .map_or(end, |character| start + character.len_utf8());
            output.push_str(&input[cursor..next]);
            cursor = next;
        }
    }
    output.push_str(&input[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VocabularyConfig;

    #[test]
    fn replaces_only_complete_phrases() {
        let config = VocabularyConfig::default();
        assert_eq!(
            apply_vocabulary_replacements("open router works", &config),
            "OpenRouter works"
        );
        assert_eq!(
            apply_vocabulary_replacements("reopen router", &config),
            "reopen router"
        );
    }
}
