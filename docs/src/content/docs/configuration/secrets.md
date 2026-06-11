---
title: "[secrets]"
description: API key lookup for cleanup providers.
---

API keys are **never** stored in `config.toml`. This section controls how VoxLine
finds secrets at runtime.

```toml
[secrets]
mode = "auto"
openrouter_env_var = "OPENROUTER_API_KEY"
allow_insecure_file_fallback = false
insecure_file_path = "~/.config/voxline/secrets.toml"
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `mode` | string | `"auto"` | Reserved for future explicit lookup modes. v1 always uses the lookup order below when resolving keys. |
| `openrouter_env_var` | string | `"OPENROUTER_API_KEY"` | Environment variable name for the OpenRouter API key. |
| `allow_insecure_file_fallback` | boolean | `false` | When true, allow reading keys from `insecure_file_path` if keyring and env are unavailable. **Not recommended** for normal use. |
| `insecure_file_path` | string | `"~/.config/voxline/secrets.toml"` | TOML file used only when `allow_insecure_file_fallback = true`. File must be mode `0600` on Unix. |

## Lookup order (OpenRouter)

1. **System keyring** — set with `voxline secrets set openrouter`
2. **Environment** — variable named by `openrouter_env_var`
3. **File fallback** — only if `allow_insecure_file_fallback = true`

## Commands

```bash
voxline secrets set openrouter
voxline secrets clear openrouter
voxline secrets status
```

## Insecure file format

Only used when explicitly enabled:

```toml
openrouter = "sk-or-..."
```

## Notes

- `voxline doctor` reports keyring availability and whether OpenRouter is configured.
- Cleanup and template snippets that call OpenRouter require a configured key.
