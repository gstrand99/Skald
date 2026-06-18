---
title: "[preview]"
description: Realtime transcription while recording.
---

Optional streaming ASR on a **separate small model** while you record. Preview text
is shown in `skald watch` or the overlay; it is **never** copied or pasted.

```toml
[preview]
enabled = false
chunk_ms = 2000
step_ms = 1000
overlap_ms = 500
min_rms_energy = 0.003
ring_buffer_seconds = 30
model_path = "~/.local/share/skald/models/ggml-small.en.bin"
gpu = false
threads = 0
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | When true, run preview ASR on rolling audio during recording. Requires a valid `model_path` file at validate time. |
| `chunk_ms` | integer | `2000` | Length of each audio window sent to preview ASR, in milliseconds. Must be positive. |
| `step_ms` | integer | `1000` | How often to advance the preview window. Must be positive. |
| `overlap_ms` | integer | `500` | Overlap between consecutive windows. Must be less than `chunk_ms`. |
| `min_rms_energy` | float | `0.003` | Skip preview inference when recent audio RMS is below this threshold. |
| `ring_buffer_seconds` | integer | `30` | Seconds of audio retained in the rolling buffer. Must be positive. |
| `model_path` | string | `ggml-small.en.bin` under `model_dir` | GGML model for preview only. Empty string uses the same default path. |
| `gpu` | boolean | `false` | Request GPU for the preview model (separate worker from final ASR). |
| `threads` | integer | `0` | CPU threads for preview. **`0`** means use `4` threads (not `asr.threads`). |

## Usage

```bash
# Set preview.enabled = true, download small model, restart daemon
skald watch
skald overlay
skald toggle
```

## Validation rules

When `enabled = true`, `skald config validate` requires:

- `chunk_ms` and `step_ms` are positive
- `overlap_ms` is less than `chunk_ms`
- `ring_buffer_seconds` is positive
- Preview model file exists on disk

## Notes

- Final dictation always uses `[asr]`, not preview settings.
- Preview model is kept warm in its own worker with `keep_warm` lifecycle internally.
- When recording stops, the daemon unloads the preview model before final transcription so the
  large ASR model has GPU/RAM headroom. The preview model reloads on the next recording start
  (small-model reload latency; validate on your hardware).
- CPU preview is recommended to avoid competing with large CUDA ASR on the same GPU.
