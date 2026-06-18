---
title: Setup wizard
description: Guided first-time install with model benchmarks.
---

`skald setup` guides first-time installation: system probe, dependency hints, a
10-second microphone fixture, model download, multi-model benchmarks, config
generation, and optional systemd service install.

## Quick start

```bash
just install          # installs binaries and launches setup when unconfigured
# or
skald setup
```

Skip the post-install prompt in CI or scripts:

```bash
SKALD_SKIP_SETUP=1 just install
```

## What the wizard does

1. **System profile** — CPU cores, RAM, NVIDIA GPU/VRAM (via `nvidia-smi`), free
   disk space in the model directory, and whether `skaldd` was built with CUDA.
2. **Dependencies** — checks PipeWire/Pulse, clipboard tools, paste helpers, and
   prints distro-specific install commands when something is missing.
3. **Recording** — saves `~/.local/share/skald/models/samples/setup.wav` (10 seconds by
   default). This file stays on disk for repeatable benchmarks.
4. **Models** — uses the same versioned catalog and verified download path as
   `skald models install`. Downloads require HTTPS and are checked for exact
   size and SHA-256 before atomic placement.
5. **Benchmarks** — transcribes the fixture with each downloaded model and shows
   cold-load and transcribe timings plus transcript previews.
6. **Selection** — you pick the ASR model, optional preview overlay, cleanup, and
   lifecycle settings. The wizard writes `config.toml` and a setup-complete marker.
7. **Service** — optional `skald service install` and compositor shortcut guidance.

## Commands

```bash
skald setup                    # full interactive wizard
skald setup --if-missing       # exit if already configured (used by just install)
skald setup --force            # re-run on an existing installation
skald setup --non-interactive  # probe-driven defaults, no prompts
skald setup --json             # machine-readable profile and results
skald setup record --seconds 10
```

## Privacy

- The setup fixture is stored locally only.
- Model downloads go to `~/.local/share/skald/models/`.
- Cleanup (OpenRouter) is opt-in during setup; no transcript text leaves the
  machine unless you enable it.

## Manual path

Advanced users can follow [Install](/install/) and use:

```bash
skald models list
skald models install small.en
skald models select small.en
```

For a noninteractive profile:

```bash
just setup-cpu
just setup-nvidia  # requires NVIDIA drivers and a CUDA-enabled build
```

Validate the resulting installation with `just validate-models-cpu` or
`just validate-models-nvidia`. NVIDIA validation requires working local NVIDIA
drivers and cannot be inferred from model-file verification alone.
