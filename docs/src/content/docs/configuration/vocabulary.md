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
| `phrases` | array of tables | OpenRouter, Hyprland, VoxLine | List of `[[vocabulary.phrases]]` entries. |
| `replacements` | array of tables | see defaults | List of `[[vocabulary.replacements]]` entries. |

## `[[vocabulary.phrases]]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | yes | Phrase to include in the Whisper initial prompt when `initial_prompt_enabled` is true. |

## `[[vocabulary.replacements]]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `from` | string | — | Text to match in the raw transcript (whole-phrase matching in the ASR layer). |
| `to` | string | — | Replacement text. |
| `case_sensitive` | boolean | `false` | When false, matching is case-insensitive. |

## CLI management

```bash
voxline vocab list
voxline vocab test "hyper land is great"
voxline vocab add phrase "My Project"
voxline vocab add replace --from "my project" --to "MyProject"
```

CLI edits rewrite `config.toml`; restart is not required for vocabulary-only changes on the next job (daemon reloads config per job for some paths; vocabulary is read from the ASR worker at transcribe time via loaded config).

## Notes

- Initial prompt biasing works best for short proper nouns and technical terms.
- Replacements fix consistent ASR mistakes without retraining the model.
