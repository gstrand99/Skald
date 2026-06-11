---
title: Install
description: Build and install VoxLine on Linux.
---

VoxLine ships as Rust binaries built from this repository. There is no distro
package yet.

## Dependencies

System packages (Arch-oriented names):

- PipeWire or PulseAudio for microphone capture
- `wl-clipboard` or `xclip` for clipboard integration
- Session tools as needed: `hyprctl`, `xdotool`, `wtype`, `notify-send`
- GTK 4 development libraries to build `voxline-overlay`
- Optional: CUDA toolkit for the GPU ASR build profile

## Build

CPU-only workspace:

```bash
just release
```

Power-user CUDA daemon (RTX-class GPU):

```bash
just release-cuda
```

Binaries install to `target/release/` (`voxline`, `voxlined`, `voxline-overlay`).

User-local install (runs the setup wizard when no config exists):

```bash
just install              # after just release (CPU)
just install-cuda         # after just release-cuda (CUDA voxlined)
```

Skip the wizard (CI or manual setup):

```bash
VOXLINE_SKIP_SETUP=1 just install
VOXLINE_SKIP_SETUP=1 just install-cuda
```

## First-time setup

Recommended: run the [Setup wizard](/setup/) (`voxline setup`).

Manual path:

```bash
voxline config init
voxline config profile power-user-nvidia   # or cpu-safe
```

Download Whisper GGML models into `~/.local/share/voxline/models/`:

- Power-user: a large quantized model (for example `ggml-large-v3-turbo-q5_0.bin`)
- CPU-safe: `ggml-small.en.bin`
- Preview (optional): `ggml-small.en.bin` when `preview.enabled = true`

Validate:

```bash
voxline config validate
voxline doctor
```

## Profiles

| Profile | ASR model | Lifecycle | Use case |
|---------|-----------|-----------|----------|
| `power-user-nvidia` | Large CUDA model | `keep_warm` | Primary workstation target |
| `cpu-safe` | `small.en` on CPU | `on_demand` | Laptops and CPU-only hosts |

Restart `voxlined` after changing profiles.

## Benchmarks

```bash
voxline bench model-load
voxline bench end-to-end ./sample.wav
voxline bench dictation ./sample.wav --no-cleanup
voxline bench dictation ./sample.wav --cleanup
voxline bench dictation ./sample.wav --paste
```

See [Benchmark results](/linux/benchmarks/) for recorded numbers on the reference
workstation.
