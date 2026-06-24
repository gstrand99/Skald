---
title: CLI reference
description: skald, skaldd, and companion binary commands.
---

The `skald` binary talks to `skaldd` over a Unix socket in your runtime
directory (default `$XDG_RUNTIME_DIR/skald/skaldd.sock`).

Companion binaries:

- `skald-overlay` — preview overlay (also launched via `skald overlay`)
- `skald-tray` — optional StatusNotifier/AppIndicator client

## Recording

| Command | Description |
|---------|-------------|
| `skald toggle` | Start or stop recording; stop transcribes and delivers output |
| `skald start` | Begin manual recording |
| `skald stop` | End recording and run the dictation pipeline |
| `skald ptt-start` | Alias for `start` |
| `skald ptt-stop` | Alias for `stop` |
| `skald cancel` | Cancel recording without retaining audio |
| `skald record --seconds N` | Timed toggle recording |

Per-job overrides on `toggle` / `record`: `--cleanup`, `--no-cleanup`, `--style NAME`,
`--snippet NAME`.

## Status and watch

| Command | Description |
|---------|-------------|
| `skald version` | Print build version |
| `skald version --json` | Version, commit, tag, target, toolchain, acceleration |
| `skaldd --build-info-json` | Daemon build metadata, including CPU or CUDA ASR backend |
| `skald status` | Daemon and job state |
| `skald watch` | Stream daemon events and preview text |
| `skald waybar` | Stream privacy-safe Waybar JSON status updates |
| `skald overlay` | Launch the graphical preview overlay |
| `skald overlay preview` | Preview overlay styles without a dictation job |

`skald overlay preview` flags: `--style`, `--cycle`, `--microphone`, `--mode`,
`--anchor`, `--save`.

| Command | Description |
|---------|-------------|
| `skald-tray` | Launch the optional tray client |

See [Waybar](/linux/waybar/) and [Tray client](/linux/tray/).

## Config and doctor

| Command | Description |
|---------|-------------|
| `skald config path` | Print active `config.toml` path |
| `skald config init` | Scaffold config tree |
| `skald config init --force` | Overwrite an existing config tree |
| `skald config validate` | Validate `config.toml` in memory |
| `skald config upgrade` | Migrate, validate, and rewrite config on disk |
| `skald config profile NAME` | Apply `power-user-nvidia` or `cpu-safe` |
| `skald doctor` | Session, tools, models, privacy, paste report |
| `skald doctor --json` | Machine-readable doctor output |
| `skald doctor --include-performance` | Include performance diagnostics warnings |

## Setup and service

| Command | Description |
|---------|-------------|
| `skald setup` | Interactive first-time wizard |
| `skald setup --if-missing` | Exit when already configured |
| `skald setup --force` | Re-run on an existing installation |
| `skald setup --non-interactive` | Probe-driven defaults, no prompts |
| `skald setup --json` | Machine-readable setup output |
| `skald setup record --seconds N` | Record the setup benchmark fixture only |
| `skald service install` | Write and enable systemd user unit |
| `skald service uninstall` | Disable and remove the user unit |
| `skald service start` | Start `skaldd` via systemd |
| `skald service stop` | Stop the user service |
| `skald service restart` | Restart the user service |
| `skald service status` | Show unit status |

## ASR and transcription

| Command | Description |
|---------|-------------|
| `skald transcribe FILE` | Transcribe a WAV through the daemon |
| `skald asr status` | Model load state |
| `skald asr load` | Load the configured ASR model |
| `skald asr unload` | Unload the ASR model |
| `skald asr restart` | Reload the ASR model |

## Benchmarks

| Command | Description |
|---------|-------------|
| `skald bench asr FILE` | Transcribe a WAV and print timings |
| `skald bench end-to-end FILE` | Transcribe-only timings |
| `skald bench dictation FILE` | Full dictation path timings |
| `skald bench model-load` | Benchmark loading the configured ASR model |

`bench end-to-end` and `bench dictation` support `--json`. Dictation bench flags:
`--no-cleanup`, `--cleanup`, `--paste`.

## Managed models

| Command | Description |
|---------|-------------|
| `skald models list` | Show catalog models, installed state, size, and intended use |
| `skald models recommend` | Detect hardware and print final/preview recommendations |
| `skald models install ID` | Download, verify, and atomically install a catalog model |
| `skald models verify [ID]` | Check exact size and SHA-256 without loading a model |
| `skald models select ID` | Select the final ASR model |
| `skald models select-preview ID` | Select the text-preview model |
| `skald models remove ID` | Confirm and remove an unused Skald-managed model |
| `skald models prune` | Review and remove unused managed models |

Skald never removes arbitrary model files. Cleanup is limited to files recorded
in `managed-models.json`, and configured or loaded models are protected.

Use `--select` or `--select-preview` during installation to configure the model
immediately. Model commands support `--json`.

## Vocabulary

| Command | Description |
|---------|-------------|
| `skald vocab list` | List configured phrases and replacements |
| `skald vocab test TEXT` | Show post-replacement output for sample text |
| `skald vocab add phrase TEXT` | Add a phrase for ASR biasing |
| `skald vocab add replace FROM TO` | Add a post-transcription replacement |

## Diagnostics

| Command | Description |
|---------|-------------|
| `skald diagnostics performance` | Print stored performance warnings |
| `skald diagnostics performance --json` | Machine-readable performance warnings |
| `skald diagnostics benchmark FILE` | Run a redacted diagnostics benchmark |
| `skald diagnostics benchmark FILE --json` | Machine-readable benchmark output |
| `skald diagnostics clear` | Clear stored performance diagnostics |

## Cleanup and secrets

```bash
skald secrets set openrouter
skald secrets clear openrouter
skald secrets status
skald cleanup enable openrouter
skald cleanup disable
skald cleanup preview "sample dictated text"
skald cleanup preview "sample text" --style NAME
```

## Styles, apps, snippets

```bash
skald styles list
skald styles new NAME [--description TEXT]
skald styles edit NAME
skald styles validate [NAME]

skald apps detect
skald apps list
skald apps edit NAME
skald apps validate [NAME]

skald snippets list
skald snippets new NAME [--template]
skald snippets insert NAME
skald snippets preview NAME "sample dictated text"
skald snippets validate [NAME]
```

## Voice commands

```bash
skald commands test "skald snippet standup"
skald commands conflicts
```

Voice commands are experimental. Enable `[voice_commands] enabled = true` in
`config.toml` before testing.

## Tests

```bash
skald test mic --seconds 5
skald test clipboard
skald test paste
skald test openrouter
```

## Shell completions

```bash
skald completions bash
skald completions zsh
skald completions fish
skald completions elvish
skald completions powershell
```

Catalog model IDs are included as completion candidates where supported.
