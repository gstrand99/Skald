---
title: "[cleanup]"
description: Opt-in cloud transcript cleanup via OpenRouter.
---

Cleanup rewrites dictated text using a cloud LLM. **Disabled by default.** Audio
never leaves the machine; only transcript text is sent when cleanup runs.

```toml
[cleanup]
enabled = false
provider = "none"
model = "~openai/gpt-mini-latest"
default_style = "default"
temperature = 0.2
timeout_ms = 10000
fallback_to_raw_on_error = true
skip_if_word_count_below = 5
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | Master switch. When false, raw ASR text is used (unless overridden per job with `--cleanup`). |
| `provider` | string | `"none"` | Cleanup backend. Use `"openrouter"` when enabled. Cannot be `"none"` while `enabled = true`. |
| `model` | string | `"~openai/gpt-mini-latest"` | OpenRouter model id when provider is `openrouter`. Required when cleanup is enabled. The `~` prefix is stripped before the API call. |
| `default_style` | string | `"default"` | Name of a style under `paths.config_dir/styles/`. Each style has a `.toml` manifest and `.md` system prompt. |
| `temperature` | float | `0.2` | Sampling temperature sent to the cleanup model. Lower values are more deterministic. |
| `timeout_ms` | integer | `10000` | HTTP timeout for cleanup requests in milliseconds. |
| `fallback_to_raw_on_error` | boolean | `true` | When true, use the raw transcript if cleanup fails or times out. |
| `skip_if_word_count_below` | integer | `5` | Skip cleanup when the word count is below this threshold (short utterances stay raw). |

## Enabling cleanup

```bash
voxline secrets set openrouter
voxline cleanup enable openrouter
voxline cleanup disable
voxline cleanup preview "hey john thanks for the update"
voxline toggle --cleanup
```

Per-job overrides: `voxline toggle --cleanup` / `--no-cleanup`.

## Styles

Cleanup prompts live in `~/.config/voxline/styles/`:

```text
styles/default.toml    # metadata (name, description)
styles/default.md      # system prompt body
```

```bash
voxline styles list
voxline styles edit professional
voxline styles validate
```

`cleanup.default_style` must reference an installed, valid style when cleanup is enabled.

## Routing overrides

Application profiles (`apps/*.toml`) and voice commands can override cleanup per job.
See [Related files](/configuration/related-files/).

## Privacy

- Enabling cleanup sends transcript text to your configured provider.
- `voxline doctor` warns when cleanup is enabled.
- Costs depend on your OpenRouter model and usage.
