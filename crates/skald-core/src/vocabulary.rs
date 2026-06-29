use std::collections::HashSet;

use thiserror::Error;

use crate::config::{VocabularyConfig, VocabularyPhrase, VocabularyReplacement};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabularyImportFormat {
    PlainText,
    Csv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabularyImportMode {
    Merge,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocabularyImportOptions {
    pub format: VocabularyImportFormat,
    pub mode: VocabularyImportMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyImportReport {
    pub phrases_added: usize,
    pub replacements_added: usize,
    pub phrases_replaced: usize,
    pub replacements_replaced: usize,
    pub duplicates: Vec<VocabularyImportIssue>,
    pub invalid_rows: Vec<VocabularyImportIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyImportIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum VocabularyImportError {
    #[error("CSV row {line} is malformed: {message}")]
    MalformedCsv { line: usize, message: String },
}

#[derive(Debug, Clone, PartialEq)]
enum ImportEntry {
    Phrase(VocabularyPhrase),
    Replacement(VocabularyReplacement),
}

pub fn import_vocabulary(
    vocabulary: &mut VocabularyConfig,
    input: &str,
    options: VocabularyImportOptions,
) -> Result<VocabularyImportReport, VocabularyImportError> {
    let mut report = VocabularyImportReport {
        phrases_added: 0,
        replacements_added: 0,
        phrases_replaced: 0,
        replacements_replaced: 0,
        duplicates: Vec::new(),
        invalid_rows: Vec::new(),
    };
    let entries = match options.format {
        VocabularyImportFormat::PlainText => parse_plain_text(input),
        VocabularyImportFormat::Csv => parse_csv(input, &mut report)?,
    };

    if options.mode == VocabularyImportMode::Replace {
        report.phrases_replaced = vocabulary.phrases.len();
        report.replacements_replaced = vocabulary.replacements.len();
        vocabulary.phrases.clear();
        vocabulary.replacements.clear();
    }

    let mut phrases = vocabulary
        .phrases
        .iter()
        .map(|phrase| phrase_key(&phrase.text))
        .collect::<HashSet<_>>();
    let mut replacements = vocabulary
        .replacements
        .iter()
        .map(replacement_key)
        .collect::<HashSet<_>>();

    for (line, entry) in entries {
        match entry {
            ImportEntry::Phrase(phrase) => {
                let key = phrase_key(&phrase.text);
                if phrases.insert(key) {
                    vocabulary.phrases.push(phrase);
                    report.phrases_added += 1;
                } else {
                    report.duplicates.push(VocabularyImportIssue {
                        line,
                        message: "duplicate phrase".into(),
                    });
                }
            }
            ImportEntry::Replacement(replacement) => {
                let key = replacement_key(&replacement);
                if replacements.insert(key) {
                    vocabulary.replacements.push(replacement);
                    report.replacements_added += 1;
                } else {
                    report.duplicates.push(VocabularyImportIssue {
                        line,
                        message: "duplicate replacement".into(),
                    });
                }
            }
        }
    }

    Ok(report)
}

fn parse_plain_text(input: &str) -> Vec<(usize, ImportEntry)> {
    input
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            let text = line.trim();
            if text.is_empty() || text.starts_with('#') {
                return None;
            }
            Some((
                line_number,
                ImportEntry::Phrase(VocabularyPhrase { text: text.into() }),
            ))
        })
        .collect()
}

fn parse_csv(
    input: &str,
    report: &mut VocabularyImportReport,
) -> Result<Vec<(usize, ImportEntry)>, VocabularyImportError> {
    let mut entries = Vec::new();
    let mut header: Option<Vec<String>> = None;

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let fields = parse_csv_line(line, line_number)?;
        if fields.iter().all(|field| field.trim().is_empty()) {
            continue;
        }
        if header.is_none() && looks_like_header(&fields) {
            header = Some(fields.iter().map(|field| normalize_header(field)).collect());
            continue;
        }
        let entry = if let Some(header) = &header {
            entry_from_named_csv_row(header, &fields, line_number, report)
        } else {
            entry_from_positional_csv_row(&fields, line_number, report)
        };
        if let Some(entry) = entry {
            entries.push((line_number, entry));
        }
    }

    Ok(entries)
}

fn entry_from_named_csv_row(
    header: &[String],
    fields: &[String],
    line: usize,
    report: &mut VocabularyImportReport,
) -> Option<ImportEntry> {
    let get = |name: &str| {
        header
            .iter()
            .position(|field| field == name)
            .and_then(|index| fields.get(index))
            .map_or("", |value| value.trim())
    };
    let phrase = first_non_empty(&[get("phrase"), get("text"), get("term")]);
    let from = get("from");
    let to = get("to");

    if !from.is_empty() || !to.is_empty() {
        replacement_from_parts(from, to, get("case_sensitive"), line, report)
    } else if let Some(phrase) = phrase {
        Some(ImportEntry::Phrase(VocabularyPhrase {
            text: phrase.to_owned(),
        }))
    } else {
        report.invalid_rows.push(VocabularyImportIssue {
            line,
            message: "row must include phrase text or from/to replacement values".into(),
        });
        None
    }
}

fn entry_from_positional_csv_row(
    fields: &[String],
    line: usize,
    report: &mut VocabularyImportReport,
) -> Option<ImportEntry> {
    match fields {
        [phrase] if !phrase.trim().is_empty() => Some(ImportEntry::Phrase(VocabularyPhrase {
            text: phrase.trim().to_owned(),
        })),
        [from, to] | [from, to, _] => {
            let case_sensitive = fields.get(2).map_or("", String::as_str);
            replacement_from_parts(from, to, case_sensitive, line, report)
        }
        _ => {
            report.invalid_rows.push(VocabularyImportIssue {
                line,
                message: "expected one phrase column or from,to[,case_sensitive]".into(),
            });
            None
        }
    }
}

fn replacement_from_parts(
    from: &str,
    to: &str,
    case_sensitive: &str,
    line: usize,
    report: &mut VocabularyImportReport,
) -> Option<ImportEntry> {
    let from = from.trim();
    let to = to.trim();
    if from.is_empty() || to.is_empty() {
        report.invalid_rows.push(VocabularyImportIssue {
            line,
            message: "replacement from and to values are required".into(),
        });
        return None;
    }
    let case_sensitive = match case_sensitive.trim() {
        "" | "false" | "FALSE" | "False" | "0" => false,
        "true" | "TRUE" | "True" | "1" => true,
        _ => {
            report.invalid_rows.push(VocabularyImportIssue {
                line,
                message: "case_sensitive must be true or false".into(),
            });
            return None;
        }
    };
    Some(ImportEntry::Replacement(VocabularyReplacement {
        from: from.to_owned(),
        to: to.to_owned(),
        case_sensitive,
    }))
}

fn parse_csv_line(line: &str, line_number: usize) -> Result<Vec<String>, VocabularyImportError> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_owned());
                field.clear();
            }
            _ => field.push(ch),
        }
    }

    if quoted {
        return Err(VocabularyImportError::MalformedCsv {
            line: line_number,
            message: "unterminated quoted field".into(),
        });
    }
    fields.push(field.trim().to_owned());
    Ok(fields)
}

fn looks_like_header(fields: &[String]) -> bool {
    fields.iter().any(|field| {
        matches!(
            normalize_header(field).as_str(),
            "phrase" | "text" | "term" | "from" | "to" | "case_sensitive"
        )
    })
}

fn normalize_header(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn first_non_empty<'a>(values: &[&'a str]) -> Option<&'a str> {
    values.iter().copied().find(|value| !value.is_empty())
}

fn phrase_key(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

fn replacement_key(replacement: &VocabularyReplacement) -> (String, String, bool) {
    (
        replacement.from.trim().to_ascii_lowercase(),
        replacement.to.trim().to_owned(),
        replacement.case_sensitive,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VocabularyConfig {
        VocabularyConfig {
            enabled: true,
            initial_prompt_enabled: true,
            post_replace_enabled: true,
            phrases: vec![VocabularyPhrase {
                text: "Skald".into(),
            }],
            replacements: vec![VocabularyReplacement {
                from: "open router".into(),
                to: "OpenRouter".into(),
                case_sensitive: false,
            }],
        }
    }

    #[test]
    fn plain_text_merge_preserves_existing_and_reports_duplicates() {
        let mut vocabulary = config();
        let report = import_vocabulary(
            &mut vocabulary,
            "Skald\nHyprland\n",
            VocabularyImportOptions {
                format: VocabularyImportFormat::PlainText,
                mode: VocabularyImportMode::Merge,
            },
        )
        .unwrap();

        assert_eq!(report.phrases_added, 1);
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(vocabulary.phrases[0].text, "Skald");
        assert_eq!(vocabulary.phrases[1].text, "Hyprland");
        assert_eq!(vocabulary.replacements.len(), 1);
    }

    #[test]
    fn csv_imports_phrases_and_replacements_with_validation() {
        let mut vocabulary = config();
        let report = import_vocabulary(
            &mut vocabulary,
            "phrase,from,to,case_sensitive\nKubernetes,,,\n,hyper land,Hyprland,false\n,bad,,false\n,case,test,maybe\n",
            VocabularyImportOptions {
                format: VocabularyImportFormat::Csv,
                mode: VocabularyImportMode::Merge,
            },
        )
        .unwrap();

        assert_eq!(report.phrases_added, 1);
        assert_eq!(report.replacements_added, 1);
        assert_eq!(report.invalid_rows.len(), 2);
        assert_eq!(vocabulary.phrases.last().unwrap().text, "Kubernetes");
        assert_eq!(vocabulary.replacements.last().unwrap().from, "hyper land");
    }

    #[test]
    fn replace_mode_clears_existing_before_importing_unique_entries() {
        let mut vocabulary = config();
        let report = import_vocabulary(
            &mut vocabulary,
            "from,to\nhyper land,Hyprland\nhyper land,Hyprland\n",
            VocabularyImportOptions {
                format: VocabularyImportFormat::Csv,
                mode: VocabularyImportMode::Replace,
            },
        )
        .unwrap();

        assert_eq!(report.phrases_replaced, 1);
        assert_eq!(report.replacements_replaced, 1);
        assert_eq!(report.replacements_added, 1);
        assert_eq!(report.duplicates.len(), 1);
        assert!(vocabulary.phrases.is_empty());
        assert_eq!(vocabulary.replacements.len(), 1);
    }

    #[test]
    fn malformed_csv_returns_line_context() {
        let mut vocabulary = config();
        let error = import_vocabulary(
            &mut vocabulary,
            "\"unterminated\n",
            VocabularyImportOptions {
                format: VocabularyImportFormat::Csv,
                mode: VocabularyImportMode::Merge,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("row 1"));
    }
}
