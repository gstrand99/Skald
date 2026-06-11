use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    config::PathsConfig,
    paths,
    snippets::{self, SnippetError},
};

pub const TEMPLATE_SNIPPET_TYPE: &str = "template";

const DEFAULT_MISSING_FALLBACK: &str = "(not provided)";
const DEFAULT_TEMPLATE_BODY: &str = "\
## Yesterday
{{yesterday}}

## Today
{{today}}

## Blocked
{{blocked}}
";

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("{0}")]
    Snippet(#[from] SnippetError),
    #[error("invalid template snippet: {0}")]
    Validation(String),
    #[error("failed to read template at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("extraction response was not valid JSON: {0}")]
    InvalidJson(String),
    #[error("extraction JSON is missing required field '{0}'")]
    MissingRequiredField(String),
    #[error("rendered template failed validation: {0}")]
    RenderValidation(String),
    #[error("template still contains unresolved placeholders")]
    UnresolvedPlaceholders,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateFieldSpec {
    pub name: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct TemplateValidationConfig {
    pub max_total_chars: Option<usize>,
    pub max_field_chars: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TemplateFailureMode {
    #[default]
    Error,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TemplateFallbackConfig {
    pub on_extract_failure: TemplateFailureMode,
}

impl Default for TemplateFallbackConfig {
    fn default() -> Self {
        Self {
            on_extract_failure: TemplateFailureMode::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateSnippetMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub snippet_type: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub template_file: String,
    pub fields: Vec<TemplateFieldSpec>,
    #[serde(default)]
    pub validation: TemplateValidationConfig,
    #[serde(default)]
    pub fallback: TemplateFallbackConfig,
}

pub fn create_template_snippet(paths: &PathsConfig, name: &str) -> Result<(), TemplateError> {
    snippets::validate_snippet_name(name)?;
    let snippets_dir = paths::snippets_dir(paths);
    fs::create_dir_all(&snippets_dir).map_err(|source| SnippetError::Write {
        path: snippets_dir.clone(),
        source,
    })?;
    let metadata_path = snippets_dir.join(format!("{name}.toml"));
    let template_path = snippets_dir.join(format!("{name}.md"));
    if metadata_path.exists() || template_path.exists() {
        return Err(SnippetError::AlreadyExists(name.into()).into());
    }
    let metadata = TemplateSnippetMetadata {
        name: name.into(),
        snippet_type: TEMPLATE_SNIPPET_TYPE.into(),
        aliases: vec![name.into()],
        template_file: format!("{name}.md"),
        fields: vec![
            TemplateFieldSpec {
                name: "yesterday".into(),
                required: true,
                fallback: None,
                max_chars: None,
            },
            TemplateFieldSpec {
                name: "today".into(),
                required: true,
                fallback: None,
                max_chars: None,
            },
            TemplateFieldSpec {
                name: "blocked".into(),
                required: false,
                fallback: Some("None".into()),
                max_chars: None,
            },
        ],
        validation: TemplateValidationConfig::default(),
        fallback: TemplateFallbackConfig::default(),
    };
    write_template_metadata(&metadata_path, &metadata)?;
    fs::write(&template_path, DEFAULT_TEMPLATE_BODY).map_err(|source| SnippetError::Write {
        path: template_path,
        source,
    })?;
    Ok(())
}

pub fn load_template_metadata(
    paths: &PathsConfig,
    name: &str,
) -> Result<TemplateSnippetMetadata, TemplateError> {
    snippets::validate_snippet_name(name)?;
    let metadata_path = paths::snippets_dir(paths).join(format!("{name}.toml"));
    if !metadata_path.is_file() {
        return Err(SnippetError::NotFound(name.into()).into());
    }
    let metadata_text =
        fs::read_to_string(&metadata_path).map_err(|source| SnippetError::Read {
            path: metadata_path.clone(),
            source,
        })?;
    let metadata: TemplateSnippetMetadata =
        toml::from_str(&metadata_text).map_err(|source| SnippetError::InvalidMetadata {
            path: metadata_path,
            source,
        })?;
    if metadata.name != name {
        return Err(
            SnippetError::NotFound(format!("{name} (metadata name is {})", metadata.name)).into(),
        );
    }
    validate_template_metadata(&metadata)?;
    Ok(metadata)
}

pub fn load_template_body(
    paths: &PathsConfig,
    metadata: &TemplateSnippetMetadata,
) -> Result<String, TemplateError> {
    let template_path = paths::snippets_dir(paths).join(&metadata.template_file);
    if !template_path.is_file() {
        return Err(SnippetError::Read {
            path: template_path,
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "template file missing"),
        }
        .into());
    }
    let body = fs::read_to_string(&template_path).map_err(|source| SnippetError::Read {
        path: template_path.clone(),
        source,
    })?;
    if body.trim().is_empty() {
        return Err(SnippetError::EmptyContent(template_path).into());
    }
    Ok(body)
}

pub fn validate_template_snippet(paths: &PathsConfig, name: &str) -> Result<(), TemplateError> {
    let metadata = load_template_metadata(paths, name)?;
    let body = load_template_body(paths, &metadata)?;
    ensure_template_placeholders(&metadata, &body)?;
    Ok(())
}

#[must_use]
pub fn build_extraction_prompt(metadata: &TemplateSnippetMetadata) -> String {
    let field_lines = metadata
        .fields
        .iter()
        .map(|field| {
            let requirement = if field.required {
                "required"
            } else {
                "optional"
            };
            format!("- {} ({requirement})", field.name)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You extract structured fields from dictated speech for VoxLine template rendering.\n\
         Return ONLY a single JSON object with these keys:\n\
         {field_lines}\n\
         Rules:\n\
         - Use JSON strings for every value.\n\
         - Do not wrap the JSON in markdown fences.\n\
         - Do not add commentary or extra keys.\n\
         - Use an empty string for unknown optional fields."
    )
}

pub fn parse_extraction_json(response: &str) -> Result<BTreeMap<String, String>, TemplateError> {
    let trimmed = strip_json_fences(response.trim());
    let value: Value = serde_json::from_str(trimmed)
        .map_err(|error| TemplateError::InvalidJson(error.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        TemplateError::InvalidJson("top-level JSON value must be an object".into())
    })?;
    let mut fields = BTreeMap::new();
    for (key, value) in object {
        let text = json_value_to_string(value)?;
        fields.insert(key.clone(), text);
    }
    Ok(fields)
}

pub fn resolve_field_values(
    metadata: &TemplateSnippetMetadata,
    extracted: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, TemplateError> {
    let mut resolved = BTreeMap::new();
    for field in &metadata.fields {
        let value = extracted.get(&field.name).cloned().unwrap_or_default();
        let trimmed = value.trim();
        let resolved_value = if trimmed.is_empty() {
            if field.required {
                field
                    .fallback
                    .clone()
                    .filter(|fallback| !fallback.trim().is_empty())
                    .ok_or_else(|| TemplateError::MissingRequiredField(field.name.clone()))?
            } else {
                field
                    .fallback
                    .clone()
                    .unwrap_or_else(|| DEFAULT_MISSING_FALLBACK.into())
            }
        } else {
            trimmed.to_owned()
        };
        validate_field_length(field, &resolved_value, &metadata.validation)?;
        resolved.insert(field.name.clone(), resolved_value);
    }
    Ok(resolved)
}

pub fn render_template(
    body: &str,
    values: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    let mut rendered = body.to_owned();
    for (name, value) in values {
        let placeholder = format!("{{{{{name}}}}}");
        rendered = rendered.replace(&placeholder, value);
    }
    if rendered.contains("{{") && rendered.contains("}}") {
        return Err(TemplateError::UnresolvedPlaceholders);
    }
    Ok(rendered.trim().to_owned())
}

pub fn render_template_snippet(
    metadata: &TemplateSnippetMetadata,
    body: &str,
    extracted: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    let values = resolve_field_values(metadata, extracted)?;
    ensure_template_placeholders(metadata, body)?;
    let rendered = render_template(body, &values)?;
    validate_rendered_output(&rendered, &metadata.validation)?;
    Ok(rendered)
}

fn validate_template_metadata(metadata: &TemplateSnippetMetadata) -> Result<(), TemplateError> {
    if metadata.snippet_type != TEMPLATE_SNIPPET_TYPE {
        return Err(TemplateError::Validation(format!(
            "unsupported snippet type '{}'",
            metadata.snippet_type
        )));
    }
    if metadata.template_file.trim().is_empty() {
        return Err(TemplateError::Validation(
            "template_file cannot be empty".into(),
        ));
    }
    if metadata.template_file.contains('/')
        || metadata.template_file.contains('\\')
        || metadata.template_file.contains("..")
    {
        return Err(TemplateError::Validation(
            "template_file must be a file name in the snippets directory".into(),
        ));
    }
    if metadata.fields.is_empty() {
        return Err(TemplateError::Validation(
            "template snippets require at least one field".into(),
        ));
    }
    let mut names = HashSet::new();
    for field in &metadata.fields {
        if field.name.trim().is_empty() {
            return Err(TemplateError::Validation(
                "field names cannot be empty".into(),
            ));
        }
        if !names.insert(field.name.clone()) {
            return Err(TemplateError::Validation(format!(
                "duplicate field '{}'",
                field.name
            )));
        }
    }
    Ok(())
}

fn ensure_template_placeholders(
    metadata: &TemplateSnippetMetadata,
    body: &str,
) -> Result<(), TemplateError> {
    for field in &metadata.fields {
        let placeholder = format!("{{{{{}}}}}", field.name);
        if !body.contains(&placeholder) {
            return Err(TemplateError::Validation(format!(
                "template is missing placeholder {placeholder}"
            )));
        }
    }
    Ok(())
}

fn validate_field_length(
    field: &TemplateFieldSpec,
    value: &str,
    validation: &TemplateValidationConfig,
) -> Result<(), TemplateError> {
    let limit = field.max_chars.or(validation.max_field_chars);
    if let Some(limit) = limit
        && value.chars().count() > limit
    {
        return Err(TemplateError::RenderValidation(format!(
            "field '{}' exceeds max length of {limit}",
            field.name
        )));
    }
    Ok(())
}

fn validate_rendered_output(
    rendered: &str,
    validation: &TemplateValidationConfig,
) -> Result<(), TemplateError> {
    if let Some(limit) = validation.max_total_chars
        && rendered.chars().count() > limit
    {
        return Err(TemplateError::RenderValidation(format!(
            "rendered template exceeds max_total_chars ({limit})"
        )));
    }
    Ok(())
}

fn json_value_to_string(value: &Value) -> Result<String, TemplateError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(TemplateError::InvalidJson(
            "field values must be JSON strings or scalars".into(),
        )),
    }
}

fn strip_json_fences(text: &str) -> &str {
    let text = text.trim();
    if let Some(inner) = text.strip_prefix("```json") {
        return inner.trim_end_matches("```").trim();
    }
    if let Some(inner) = text.strip_prefix("```") {
        return inner.trim_end_matches("```").trim();
    }
    text
}

fn write_template_metadata(
    path: &PathBuf,
    metadata: &TemplateSnippetMetadata,
) -> Result<(), SnippetError> {
    let text = toml::to_string_pretty(metadata).map_err(|error| SnippetError::Write {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
    })?;
    fs::write(path, text).map_err(|source| SnippetError::Write {
        path: path.clone(),
        source,
    })
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standup_metadata() -> TemplateSnippetMetadata {
        TemplateSnippetMetadata {
            name: "standup".into(),
            snippet_type: TEMPLATE_SNIPPET_TYPE.into(),
            aliases: vec!["standup".into()],
            template_file: "standup.md".into(),
            fields: vec![
                TemplateFieldSpec {
                    name: "yesterday".into(),
                    required: true,
                    fallback: None,
                    max_chars: None,
                },
                TemplateFieldSpec {
                    name: "today".into(),
                    required: true,
                    fallback: None,
                    max_chars: None,
                },
                TemplateFieldSpec {
                    name: "blocked".into(),
                    required: false,
                    fallback: Some("None".into()),
                    max_chars: None,
                },
            ],
            validation: TemplateValidationConfig {
                max_total_chars: Some(500),
                max_field_chars: None,
            },
            fallback: TemplateFallbackConfig::default(),
        }
    }

    #[test]
    fn parses_json_and_renders_template() {
        let metadata = standup_metadata();
        let body = "## Yesterday\n{{yesterday}}\n\n## Today\n{{today}}\n\n## Blocked\n{{blocked}}";
        let extracted = parse_extraction_json(
            r#"{"yesterday":"fixed bugs","today":"template snippets","blocked":""}"#,
        )
        .unwrap();
        let rendered = render_template_snippet(&metadata, body, &extracted).unwrap();
        assert!(rendered.contains("fixed bugs"));
        assert!(rendered.contains("template snippets"));
        assert!(rendered.contains("Blocked\nNone"));
    }

    #[test]
    fn rejects_missing_required_field_without_fallback() {
        let metadata = standup_metadata();
        let body = "## Yesterday\n{{yesterday}}\n\n## Today\n{{today}}\n\n## Blocked\n{{blocked}}";
        let extracted = parse_extraction_json(r#"{"today":"only today"}"#).unwrap();
        let error = render_template_snippet(&metadata, body, &extracted).unwrap_err();
        assert!(matches!(error, TemplateError::MissingRequiredField(_)));
    }

    #[test]
    fn rejects_unresolved_placeholders() {
        let mut values = BTreeMap::new();
        values.insert("yesterday".into(), "done".into());
        let error = render_template("{{yesterday}} {{today}}", &values).unwrap_err();
        assert!(matches!(error, TemplateError::UnresolvedPlaceholders));
    }

    #[test]
    fn strips_markdown_json_fences() {
        let extracted = parse_extraction_json("```json\n{\"today\":\"ship it\"}\n```").unwrap();
        assert_eq!(extracted.get("today").map(String::as_str), Some("ship it"));
    }
}
