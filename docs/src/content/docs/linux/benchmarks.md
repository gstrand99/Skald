---
title: Benchmark results
description: Reference workstation latency measurements.
---

Recorded on the primary development machine (Ryzen 5900X-class, RTX 3070 Ti,
Hyprland Wayland, CUDA `voxlined` build, `power-user-nvidia` profile, warm ASR
model).

## Fixtures

| Fixture | Duration | Notes |
|---------|----------|-------|
| Long speech sample | ~50 s | Transcribe-only numbers below |
| Short bench clip | ~4.9 s | Full dictation-path benches |

`bench dictation` reads an existing WAV and does not delete the source file.

## Transcribe only (`voxline bench end-to-end`)

Long fixture (~50 s):

| Run | Audio | Model load | Transcribe | Total ASR |
|-----|-------|------------|------------|-----------|
| Warm | 49962 ms | 0 ms | 614 ms | 614 ms |
| Warm (repeat) | 49962 ms | 0 ms | 607 ms | 607 ms |

Short fixture (~4.9 s):

| Run | Audio | Model load | Transcribe | Total ASR |
|-----|-------|------------|------------|-----------|
| Warm | 4906 ms | 0 ms | 263 ms | 263 ms |

Cold model load (`voxline bench model-load` after unload): **125–139 ms**.

## Full dictation path (`voxline bench dictation`)

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

## Latency targets

Targets assume a **10-second** utterance. On this profile, warm local-only
dictation (no cleanup) is well under the **1.5 s** stop-to-clipboard p50 target.
Cleanup adds provider latency; the short-fixture run above stayed under **1.2 s**
total stop-to-clipboard.

## Re-run locally

```bash
just bench-e2e /path/to/sample.wav
just bench-dictation /path/to/sample.wav --no-cleanup
just bench-dictation /path/to/sample.wav --cleanup
just bench-dictation /path/to/sample.wav --paste
just bench-model-load
```

## Sign-off

| Field | Value |
|-------|-------|
| Machine | Ryzen 5900X-class + RTX 3070 Ti |
| Session | Hyprland Wayland |
| Profile | `power-user-nvidia` |
| ASR model | `ggml-large-v3-turbo-q5_0.bin` |
| Validated | 2026-06-11 |
