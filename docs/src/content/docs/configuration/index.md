---
title: Configuration overview
description: config.toml location, validation, profiles, and directory layout.
---

Skald uses a single TOML file at `~/.config/skald/config.toml`. If the file is
missing, the daemon and CLI use the same built-in defaults as `skald config init`.

This path is fixed: `paths.config_dir` relocates styles, apps, and snippets only,
not `config.toml` itself (the bootstrap loader cannot read a path from a file it has
not yet loaded).

## Commands

```bash
skald config init          # write defaults and scaffold directories
skald config validate      # migrate in memory and validate current rules
skald config upgrade       # rewrite with migrated schema and new defaults
skald config path          # print the active config path
skald config profile NAME  # apply power-user-nvidia or cpu-safe preset
skald doctor               # runtime checks including config and models
```

Restart `skaldd` after changing ASR, paths, or preview settings.

## Config versions and migration

`config_version` identifies the on-disk schema. Skald migrates supported older
versions in memory before deserialization and validation; loading does not rewrite
the file. `skald config upgrade` preserves configured values, writes the current
schema and newly defaulted fields, and refreshes optional config directories and
built-in files.

Version 2 renames the former `[overlay]` key `style` to `visualizer_style`.
Configs without that legacy key migrate without other changes. Versions newer
than the running binary, versions older than the supported migration chain, and
ambiguous migrations fail with a clear error. Run `skald config validate` or
`skald doctor` to report migration and validation failures.

## Reference sections

Each top-level table in `config.toml` is documented in its own page:

| Section | Page |
|---------|------|
| `[daemon]` | [Daemon](/configuration/daemon/) |
| `[paths]` | [Paths](/configuration/paths/) |
| `[audio]` / `[audio.gates]` | [Audio](/configuration/audio/) |
| `[asr]` / lifecycle / hallucination filter | [ASR](/configuration/asr/) |
| `[vocabulary]` | [Vocabulary](/configuration/vocabulary/) |
| `[cleanup]` | [Cleanup](/configuration/cleanup/) |
| `[secrets]` | [Secrets](/configuration/secrets/) |
| `[injection]` / `[injection.linux]` | [Injection](/configuration/injection/) |
| `[notifications]` | [Notifications](/configuration/notifications/) |
| `[privacy]` | [Privacy](/configuration/privacy/) |
| `[voice_commands]` | [Voice commands](/configuration/voice-commands/) |
| `[preview]` | [Preview](/configuration/preview/) |
| `[overlay]` | [Overlay](/configuration/overlay/) |

Styles, app profiles, and snippets live as separate files under `paths.config_dir`.
See [Related files](/configuration/related-files/).

## Example file

A commented reference copy ships in the repository:
[`config-example/linux/config.toml`](https://github.com/gstrand/skald/blob/main/config-example/linux/config.toml).

## Preset profiles

`power-user-nvidia` resets nearly the entire config to built-in defaults,
preserving only `[secrets]` and `[cleanup]`. `cpu-safe` applies CPU-safe ASR and
lifecycle settings and disables cleanup without a full reset.

```bash
skald config profile power-user-nvidia
skald config profile cpu-safe
```

| Profile | ASR | GPU | Lifecycle | Cleanup |
|---------|-----|-----|-----------|---------|
| `power-user-nvidia` | Large turbo model (default path) | yes | `keep_warm` | unchanged |
| `cpu-safe` | `ggml-small.en.bin` | no | `on_demand` | disabled |

For tailored settings from benchmarks, use [Setup wizard](/setup/) instead of a
fixed profile.

## Directory layout

`skald config init` creates:

```text
~/.config/skald/
  config.toml
  styles/       # cleanup prompt styles
  apps/         # per-window application profiles
  snippets/     # insert and template snippets
~/.local/share/skald/models/    # Whisper GGML files
$XDG_RUNTIME_DIR/skald/        # socket and temporary WAVs (runtime_dir = auto)
```

Tilde paths (`~/...`) in `config.toml` are expanded relative to your home directory.
