---
title: Setup wizard
description: Guided first-time install with model benchmarks.
---

`voxline setup` guides first-time installation: system probe, dependency hints, a
10-second microphone fixture, model download, multi-model benchmarks, config
generation, and optional systemd service install.

## Quick start

```bash
just install          # installs binaries and launches setup when unconfigured
# or
voxline setup
```

Skip the post-install prompt in CI or scripts:

```bash
VOXLINE_SKIP_SETUP=1 just install
```

## What the wizard does

1. **System profile** — CPU cores, RAM, NVIDIA GPU/VRAM (via `nvidia-smi`), free
   disk space in the model directory, and whether `voxlined` was built with CUDA.
2. **Dependencies** — checks PipeWire/Pulse, clipboard tools, paste helpers, and
   prints distro-specific install commands when something is missing.
3. **Recording** — saves `~/.local/share/voxline/samples/setup.wav` (10 seconds by
   default). This file stays on disk for repeatable benchmarks.
4. **Models** — offers to download candidate GGML models from Hugging Face
   (`ggerganov/whisper.cpp`). Candidates depend on your hardware profile.
5. **Benchmarks** — transcribes the fixture with each downloaded model and shows
   cold-load and transcribe timings plus transcript previews.
6. **Selection** — you pick the ASR model, optional preview overlay, cleanup, and
   lifecycle settings. The wizard writes `config.toml` and a setup-complete marker.
7. **Service** — optional `voxline service install` and compositor shortcut guidance.

## Commands

```bash
voxline setup                    # full interactive wizard
voxline setup --if-missing       # exit if already configured (used by just install)
voxline setup --force            # re-run on an existing installation
voxline setup --non-interactive  # probe-driven defaults, no prompts
voxline setup --json             # machine-readable profile and results
voxline setup record --seconds 10
```

## Privacy

- The setup fixture is stored locally only.
- Model downloads go to `~/.local/share/voxline/models/`.
- Cleanup (OpenRouter) is opt-in during setup; no transcript text leaves the
  machine unless you enable it.

## Manual path

Advanced users can follow [Install](/install/) without the wizard.
