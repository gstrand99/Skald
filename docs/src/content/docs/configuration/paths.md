---
title: "[paths]"
description: Config, model, and runtime directory locations.
---

Defines where Skald stores configuration, Whisper models, and ephemeral runtime
files (socket, temporary WAVs).

```toml
[paths]
config_dir = "~/.config/skald"
model_dir = "~/.local/share/skald/models"
runtime_dir = "auto"
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `config_dir` | string | `"~/.config/skald"` | Root for `config.toml`, `styles/`, `apps/`, and `snippets/`. Expanded from `~`. |
| `model_dir` | string | `"~/.local/share/skald/models"` | Directory for GGML Whisper model files referenced by `asr.model_path` and `preview.model_path`. |
| `runtime_dir` | string | `"auto"` | Runtime working directory. **`auto`** resolves to `$XDG_RUNTIME_DIR/skald` on Linux (recommended). Use an absolute or tilde path only if you have a specific reason. Cannot be empty. |

## Runtime contents

When `runtime_dir = "auto"`:

```text
$XDG_RUNTIME_DIR/skald/
  skaldd.sock          # Unix socket (mode 0600)
  <job-id>.wav           # temporary recordings (deleted unless privacy retains audio)
```

## Notes

- `skald doctor` checks that the runtime directory exists with mode `0700` when using the default layout.
- Setup wizard and `config init` scaffold `config_dir` subdirectories and `model_dir`.
- Changing `runtime_dir` while the daemon is running requires restart; clients must connect to the new socket path.
