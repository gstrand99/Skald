---
title: CLI reference
description: Common voxline and voxlined commands.
---

The `voxline` binary talks to `voxlined` over a Unix socket in your runtime
directory (default `$XDG_RUNTIME_DIR/voxline/voxlined.sock`).

## Recording

| Command | Description |
|---------|-------------|
| `voxline toggle` | Start or stop recording; stop transcribes and delivers output |
| `voxline start` | Begin manual recording |
| `voxline stop` | End recording and run the dictation pipeline |
| `voxline cancel` | Cancel recording without retaining audio |
| `voxline record --seconds N` | Timed toggle recording |

Per-job cleanup overrides: `--cleanup`, `--no-cleanup`. Style/snippet:
`--style NAME`, `--snippet NAME`.

Push-to-talk aliases: `voxline ptt-start`, `voxline ptt-stop`.

## Status and watch

| Command | Description |
|---------|-------------|
| `voxline status` | Daemon and job state |
| `voxline watch` | Stream daemon events and preview text |
| `voxline overlay` | Launch the graphical preview overlay |

## Config and doctor

| Command | Description |
|---------|-------------|
| `voxline config init` | Scaffold config tree |
| `voxline config validate` | Validate `config.toml` |
| `voxline config profile NAME` | Apply `power-user-nvidia` or `cpu-safe` |
| `voxline doctor` | Session, tools, models, privacy, paste report |

## Setup and service

| Command | Description |
|---------|-------------|
| `voxline setup` | Interactive first-time wizard |
| `voxline service install` | Write and enable systemd user unit |
| `voxline service start` | Start `voxlined` via systemd |
| `voxline service stop` | Stop the user service |
| `voxline service status` | Show unit status |

## ASR and benchmarks

| Command | Description |
|---------|-------------|
| `voxline transcribe FILE` | Transcribe a WAV through the daemon |
| `voxline asr status` | Model load state |
| `voxline asr load` / `unload` / `restart` | Control ASR lifecycle |
| `voxline bench end-to-end FILE` | Transcribe-only timings |
| `voxline bench dictation FILE` | Full dictation path timings |

Dictation bench flags: `--no-cleanup`, `--cleanup`, `--paste`, `--json`.

## Cleanup and secrets

```bash
voxline secrets set openrouter
voxline secrets status openrouter
voxline cleanup enable openrouter
voxline cleanup disable
voxline cleanup preview "sample dictated text"
```

## Styles, apps, snippets

```bash
voxline styles list
voxline styles edit NAME
voxline apps detect
voxline apps list
voxline snippets list
voxline snippets insert NAME
```

## Tests

```bash
voxline test mic --seconds 5
voxline test clipboard
voxline test paste
voxline test openrouter
```
