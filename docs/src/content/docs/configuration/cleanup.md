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
| `model` | string | `""` in a fresh `config init` | OpenRouter model id when provider is `openrouter`. Required when cleanup is enabled. The `~` prefix is stripped before the API call. When empty on disk, `skald cleanup enable` and the daemon fill in `~openai/gpt-mini-latest` at runtime. |
| `default_style` | string | `"default"` | Name of a style under `paths.config_dir/styles/`. Each style has a `.toml` manifest and `.md` system prompt. |
| `temperature` | float | `0.2` | Sampling temperature sent to the cleanup model. Lower values are more deterministic. |
| `timeout_ms` | integer | `10000` | HTTP timeout for cleanup requests in milliseconds. |
| `fallback_to_raw_on_error` | boolean | `true` | When true, use the raw transcript if cleanup fails or times out. |
| `skip_if_word_count_below` | integer | `5` | Skip cleanup when the word count is below this threshold (short utterances stay raw). |

Fresh `config init` writes `model = ""` while cleanup is disabled. The example
above shows the effective model after enabling cleanup.

## Enabling cleanup

```bash
skald secrets set openrouter
skald cleanup enable openrouter
skald cleanup disable
skald cleanup preview "hey john thanks for the update"
skald toggle --cleanup
```

Per-job overrides: `skald toggle --cleanup` / `--no-cleanup`.

## Styles

Cleanup prompts live in `~/.config/skald/styles/`:

```text
styles/default.toml    # metadata (name, description)
styles/default.md      # system prompt body
```

```bash
skald styles list
skald styles edit professional
skald styles validate
```

`cleanup.default_style` must reference an installed, valid style when cleanup is enabled.

## Routing overrides

Application profiles (`apps/*.toml`) and voice commands can override cleanup per job.
See [Related files](/configuration/related-files/).

## Privacy

- Enabling cleanup sends transcript text to your configured provider.
- `skald doctor` warns when cleanup is enabled.
- Costs depend on your OpenRouter model and usage.
