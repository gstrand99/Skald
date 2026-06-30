---
title: "[vocabulary]"
description: ASR biasing phrases and post-transcription replacements.
---

Improves recognition of domain terms and fixes systematic mis-hearings.

```toml
[vocabulary]
enabled = true
initial_prompt_enabled = true
post_replace_enabled = true

[[vocabulary.phrases]]
text = "Hyprland"

[[vocabulary.replacements]]
from = "hyper land"
to = "Hyprland"
case_sensitive = false
```

## Top-level options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `true` | Master switch for vocabulary features. When false, phrases and replacements are ignored. |
| `initial_prompt_enabled` | boolean | `true` | When true, pass configured phrases to Whisper as an initial prompt (comma-separated). Biases spelling of names and product terms. |
| `post_replace_enabled` | boolean | `true` | When true, apply `[[vocabulary.replacements]]` after transcription. |
| `phrases` | array of tables | OpenRouter, Hyprland, Skald | List of `[[vocabulary.phrases]]` entries. |
| `replacements` | array of tables | see defaults | List of `[[vocabulary.replacements]]` entries. |

## `[[vocabulary.phrases]]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | yes | Phrase to include in the Whisper initial prompt when `initial_prompt_enabled` is true. |

## `[[vocabulary.replacements]]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `from` | string | — | Text to match in the raw transcript at whole-word boundaries in the ASR layer. |
| `to` | string | — | Replacement text. |
| `case_sensitive` | boolean | `false` | When false, matching is case-insensitive. |

## CLI management

```bash
skald vocab list
skald vocab test "hyper land is great"
skald vocab add phrase "My Project"
skald vocab add replace "open router" "OpenRouter"
skald vocab import vocabulary.txt
skald vocab import replacements.csv --format csv
```

CLI edits rewrite `config.toml`. Each new transcription loads one validated
vocabulary snapshot, so phrase and replacement changes apply without restarting
`skaldd`. An in-flight transcription keeps the snapshot it started with. If the
edited config is invalid, the daemon logs the validation error and keeps the last
valid vocabulary.

## Bulk import

Plain text imports treat each non-empty, non-comment line as a phrase:

```text
Hyprland
OpenRouter
Project Skald
```

CSV imports support either positional rows or a header row. One column imports a
phrase. Two or three columns import a deterministic replacement:

```csv
from,to,case_sensitive
hyper land,Hyprland,false
open router,OpenRouter,false
```

Header names can use `phrase`, `text`, or `term` for phrases, and `from`, `to`,
and `case_sensitive` for replacements. `case_sensitive` defaults to false when
omitted.

Imports merge by default and preserve existing entries. Duplicate phrases or
replacement rows are skipped and printed with line numbers. Empty values,
malformed CSV, and invalid `case_sensitive` values are reported with line
context. Use `--replace` only when the imported file should become the complete
vocabulary.

Import files are read locally and are not sent to a network service. Vocabulary
phrases may appear in local Whisper prompts, and replacement text is applied
locally after transcription. Transcript text is sent to a cleanup provider only
when cleanup is explicitly enabled.

## Notes

- Initial prompt biasing works best for short proper nouns and technical terms.
- Replacements fix consistent ASR mistakes without retraining the model.
- `skald vocab test` applies the same whole-word replacement rules used during
  transcription.
