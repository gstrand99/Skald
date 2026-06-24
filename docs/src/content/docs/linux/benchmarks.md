---
title: Benchmark results
description: Reference workstation latency measurements.
---

Recorded on the primary development machine (Ryzen 5900X-class, RTX 3070 Ti,
Hyprland Wayland).

## Fixtures

| Fixture | Duration | Notes |
|---------|----------|-------|
| Long speech sample | ~50 s | CUDA transcribe-only numbers below |
| Setup fixture | ~9.9 s | CPU and CUDA dictation-path benches |

`bench dictation` reads an existing WAV and does not delete the source file.

## CUDA profile (`power-user-nvidia`)

CUDA `skaldd` build, warm `ggml-large-v3-turbo-q5_0.bin`.

### Transcribe only (`skald bench end-to-end`)

Long fixture (~50 s):

| Run | Audio | Model load | Transcribe | Total ASR |
|-----|-------|------------|------------|-----------|
| Warm | 49962 ms | 0 ms | 614 ms | 614 ms |
| Warm (repeat) | 49962 ms | 0 ms | 607 ms | 607 ms |

Short fixture (~4.9 s):

| Run | Audio | Model load | Transcribe | Total ASR |
|-----|-------|------------|------------|-----------|
| Warm | 4906 ms | 0 ms | 263 ms | 263 ms |

Cold model load (`skald bench model-load` after unload): **125–139 ms**.

### Full dictation path (`skald bench dictation`)

Short fixture (~4.9 s), cleanup disabled (`--no-cleanup`):

| Run | Transcribe | Stop-to-clipboard | Cleanup |
|-----|------------|-------------------|---------|
| 1 | 233 ms | 287 ms | no |
| 2 (repeat) | 235 ms | 287 ms | no |

With cleanup (`--cleanup`, OpenRouter):

| Transcribe | Cleanup | Stop-to-clipboard |
|------------|---------|-------------------|
| 250 ms | 815 ms | 1118 ms |

Paste attempt (`--paste --no-cleanup`) from an unfocused terminal: paste skipped
(active target unstable); stop-to-clipboard **322 ms**. Re-run with a stable
editor focused to measure stop-to-insert.

### CUDA sign-off

| Field | Value |
|-------|-------|
| Machine | Ryzen 5900X-class + RTX 3070 Ti |
| Session | Hyprland Wayland |
| Profile | `power-user-nvidia` |
| ASR model | `ggml-large-v3-turbo-q5_0.bin` |
| Validated | 2026-06-11 |

## CPU profile (`cpu-safe`)

CPU `skaldd` build, `cpu-safe` profile, `ggml-small.en.bin`, setup fixture
(~9.9 s).

### Transcribe only (`skald bench end-to-end`)

| Run | Audio | Model load | Transcribe | Total ASR |
|-----|-------|------------|------------|-----------|
| Warm | 9940 ms | 221 ms | 2718 ms | 2939 ms |
| Warm (repeat) | 9940 ms | 217 ms | 2679 ms | 2896 ms |

Cold model load (`skald bench model-load` after unload): **208 ms**.

### Full dictation path (`skald bench dictation`)

Setup fixture (~9.9 s), cleanup disabled (`--no-cleanup`):

| Run | Model load | Transcribe | Stop-to-clipboard |
|-----|------------|------------|-------------------|
| 1 | 0 ms | 2775 ms | 2891 ms |
| 2 (repeat) | 215 ms | 2702 ms | 3028 ms |

### CPU sign-off

| Field | Value |
|-------|-------|
| Machine | Ryzen 5900X-class |
| Session | Hyprland Wayland |
| Profile | `cpu-safe` |
| ASR model | `ggml-small.en.bin` |
| Validated | 2026-06-24 |

## Latency targets

Targets assume a **10-second** utterance. On the CUDA profile, warm local-only
dictation (no cleanup) is well under the **1.5 s** stop-to-clipboard p50 target.
Cleanup adds provider latency; the short CUDA fixture run stayed under **1.2 s**
total stop-to-clipboard.

On the CPU profile with `small.en`, warm stop-to-clipboard is about **2.9–3.0 s**
for the ~10 s setup fixture.

## Re-run locally

```bash
just bench-e2e /path/to/sample.wav
just bench-dictation /path/to/sample.wav --no-cleanup
just bench-dictation /path/to/sample.wav --cleanup
just bench-dictation /path/to/sample.wav --paste
just bench-model-load
```

For CPU-safe numbers, use a CPU `skaldd` build and `skald config profile cpu-safe`
before benchmarking.
