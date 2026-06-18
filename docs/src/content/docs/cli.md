---
title: CLI reference
description: Common skald and skaldd commands.
---

The `skald` binary talks to `skaldd` over a Unix socket in your runtime
directory (default `$XDG_RUNTIME_DIR/skald/skaldd.sock`).

## Recording

| Command | Description |
|---------|-------------|
| `skald toggle` | Start or stop recording; stop transcribes and delivers output |
| `skald start` | Begin manual recording |
| `skald stop` | End recording and run the dictation pipeline |
| `skald cancel` | Cancel recording without retaining audio |
| `skald record --seconds N` | Timed toggle recording |

Per-job cleanup overrides: `--cleanup`, `--no-cleanup`. Style/snippet:
`--style NAME`, `--snippet NAME`.

Push-to-talk aliases: `skald ptt-start`, `skald ptt-stop`.

## Status and watch

| Command | Description |
|---------|-------------|
| `skald status` | Daemon and job state |
| `skald watch` | Stream daemon events and preview text |
| `skald waybar` | Stream privacy-safe Waybar JSON status updates |
| `skald overlay` | Launch the graphical preview overlay |
| `skald-tray` | Launch the optional StatusNotifier/AppIndicator client |

## Config and doctor

| Command | Description |
|---------|-------------|
| `skald config init` | Scaffold config tree |
| `skald config validate` | Validate `config.toml` |
| `skald config profile NAME` | Apply `power-user-nvidia` or `cpu-safe` |
| `skald doctor` | Session, tools, models, privacy, paste report |

## Setup and service

| Command | Description |
|---------|-------------|
| `skald setup` | Interactive first-time wizard |
| `skald service install` | Write and enable systemd user unit |
| `skald service start` | Start `skaldd` via systemd |
| `skald service stop` | Stop the user service |
| `skald service status` | Show unit status |

## ASR and benchmarks

| Command | Description |
|---------|-------------|
| `skald transcribe FILE` | Transcribe a WAV through the daemon |
| `skald asr status` | Model load state |
| `skald asr load` / `unload` / `restart` | Control ASR lifecycle |
| `skald bench end-to-end FILE` | Transcribe-only timings |
| `skald bench dictation FILE` | Full dictation path timings |

Dictation bench flags: `--no-cleanup`, `--cleanup`, `--paste`, `--json`.

## Cleanup and secrets

```bash
skald secrets set openrouter
skald secrets status openrouter
skald cleanup enable openrouter
skald cleanup disable
skald cleanup preview "sample dictated text"
```

## Styles, apps, snippets

```bash
skald styles list
skald styles edit NAME
skald apps detect
skald apps list
skald snippets list
skald snippets insert NAME
```

## Tests

```bash
skald test mic --seconds 5
skald test clipboard
skald test paste
skald test openrouter
```
