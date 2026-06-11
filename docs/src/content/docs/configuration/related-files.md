---
title: Related configuration files
description: Styles, application profiles, and snippets outside config.toml.
---

Several features use TOML files under `paths.config_dir` (default
`~/.config/voxline/`). They are referenced from `config.toml` but edited separately.

## Cleanup styles (`styles/`)

Each style is a pair of files:

```text
styles/default.toml
styles/default.md
```

Referenced by `[cleanup].default_style`. The `.md` file is the system prompt sent to
OpenRouter.

```bash
voxline styles list
voxline styles new professional
voxline styles edit professional
voxline styles validate
```

## Application profiles (`apps/`)

Match the active window at recording **start** using `app_id` and title patterns.

Example effects:

- Override cleanup style or disable cleanup for a terminal
- Set `prefer_clipboard_only` for apps where paste is unsafe
- Attach extra prompt context for cleanup

```bash
voxline apps detect
voxline apps list
voxline apps edit terminal
voxline apps validate
```

Bundled `terminal` profile is installed by `voxline config init`.

## Snippets (`snippets/`)

Named insert targets for static text or template-based structured output.

```bash
voxline snippets list
voxline snippets new signature
voxline snippets new standup --template
voxline snippets validate
voxline toggle --snippet signature
```

Template snippets use OpenRouter JSON extraction; require secrets and network like cleanup.

## Priority (routing)

When multiple sources apply to one job:

1. CLI flags (`--style`, `--snippet`, `--cleanup` / `--no-cleanup`)
2. Voice commands (when enabled)
3. Application profile matched at recording start
4. Global `config.toml` defaults

## Validation

`voxline config validate` checks installed styles when cleanup is enabled, and
reports issues in apps and snippets via `voxline doctor`.
