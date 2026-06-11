---
title: Configuration overview
description: config.toml location, validation, profiles, and directory layout.
---

VoxLine uses a single TOML file at `~/.config/voxline/config.toml`. If the file is
missing, the daemon and CLI use the same built-in defaults as `voxline config init`.

## Commands

```bash
voxline config init          # write defaults and scaffold directories
voxline config validate      # validate against v1 rules
voxline config path          # print the active config path
voxline config profile NAME  # apply power-user-nvidia or cpu-safe preset
voxline doctor               # runtime checks including config and models
```

Restart `voxlined` after changing ASR, paths, or preview settings.

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
[`config-example/linux/config.toml`](https://github.com/gstrand/voxline/blob/main/config-example/linux/config.toml).

## Preset profiles

`power-user-nvidia` resets nearly the entire config to built-in defaults,
preserving only `[secrets]` and `[cleanup]`. `cpu-safe` applies CPU-safe ASR and
lifecycle settings and disables cleanup without a full reset.

```bash
voxline config profile power-user-nvidia
voxline config profile cpu-safe
```

| Profile | ASR | GPU | Lifecycle | Cleanup |
|---------|-----|-----|-----------|---------|
| `power-user-nvidia` | Large turbo model (default path) | yes | `keep_warm` | unchanged |
| `cpu-safe` | `ggml-small.en.bin` | no | `on_demand` | disabled |

For tailored settings from benchmarks, use [Setup wizard](/setup/) instead of a
fixed profile.

## Directory layout

`voxline config init` creates:

```text
~/.config/voxline/
  config.toml
  styles/       # cleanup prompt styles
  apps/         # per-window application profiles
  snippets/     # insert and template snippets
~/.local/share/voxline/models/    # Whisper GGML files
$XDG_RUNTIME_DIR/voxline/        # socket and temporary WAVs (runtime_dir = auto)
```

Tilde paths (`~/...`) in `config.toml` are expanded relative to your home directory.
