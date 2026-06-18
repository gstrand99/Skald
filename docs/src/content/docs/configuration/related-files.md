---
title: Related configuration files
description: Styles, application profiles, and snippets outside config.toml.
---

Several features use TOML files under `paths.config_dir` (default
`~/.config/skald/`). They are referenced from `config.toml` but edited separately.

## Cleanup styles (`styles/`)

Each style is a pair of files:

```text
styles/default.toml
styles/default.md
```

Referenced by `[cleanup].default_style`. The `.md` file is the system prompt sent to
OpenRouter.

```bash
skald styles list
skald styles new professional
skald styles edit professional
skald styles validate
```

## Application profiles (`apps/`)

Match the active window at recording **start** using `app_id` and title patterns.

Example effects:

- Override cleanup style or disable cleanup for a terminal
- Set `prefer_clipboard_only` for apps where paste is unsafe
- Attach extra prompt context for cleanup

```bash
skald apps detect
skald apps list
skald apps edit terminal
skald apps validate
```

Bundled `terminal` profile is installed by `skald config init`.

## Snippets (`snippets/`)

Named insert targets for static text or template-based structured output.

```bash
skald snippets list
skald snippets new signature
skald snippets new standup --template
skald snippets validate
skald toggle --snippet signature
```

Template snippets use OpenRouter JSON extraction; require secrets and network like cleanup.

## Priority (routing)

When multiple sources apply to one job:

1. CLI flags (`--style`, `--snippet`, `--cleanup` / `--no-cleanup`)
2. Voice commands (when enabled)
3. Application profile matched at recording start
4. Global `config.toml` defaults

## Validation

`skald config validate` checks installed styles when cleanup is enabled, and
reports issues in apps and snippets via `skald doctor`.
