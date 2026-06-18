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
```

CLI edits rewrite `config.toml`. Restart `skaldd` after vocabulary changes: the ASR
worker captures vocabulary when it spawns at daemon start.

## Notes

- Initial prompt biasing works best for short proper nouns and technical terms.
- Replacements fix consistent ASR mistakes without retraining the model.
- `skald vocab test` applies the same whole-word replacement rules used during
  transcription.
