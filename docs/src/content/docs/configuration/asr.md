---
title: "[asr]"
description: Local Whisper transcription, lifecycle, and hallucination filtering.
---

Final transcription uses a local GGML Whisper model through `whisper_rs`.

```toml
[asr]
backend = "whisper_rs"
model_path = "~/.local/share/skald/models/ggml-large-v3-turbo-q5_0.bin"
language = "en"
threads = 8
gpu = true
gpu_backend = "cuda"

[asr.lifecycle]
mode = "keep_warm"
warm_on_daemon_start = true
idle_unload_seconds = 900

[asr.hallucination_filter]
enabled = true
phrases = [
  "thank you.",
  "thanks for watching.",
  "subtitles by*",
  "subtitle by*",
  "captioned by*",
]
```

## `[asr]` options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `backend` | string | `"whisper_rs"` | ASR engine. Only `whisper_rs` is supported in v1. |
| `model_path` | string | large turbo q5 path under `model_dir` | Path to a GGML `.bin` model file. Tilde paths are expanded. File must exist before transcription (doctor warns if missing). |
| `language` | string | `"en"` | Whisper language code passed to the model. |
| `threads` | integer | `8` | CPU threads for inference (used for CPU paths and non-GPU work inside the backend). |
| `gpu` | boolean | `true` | Request GPU acceleration when `skaldd` was **built with CUDA** (`just release-cuda`). If `true` on a CPU-only build, model load fails with an unsupported-feature error. |
| `gpu_backend` | string | `"cuda"` | Reserved for future backend selection. Currently GPU use is controlled by `gpu` and the CUDA compile feature, not this field. |

## `[asr.lifecycle]` options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `mode` | string | `"keep_warm"` | **`keep_warm`**: keep model loaded between jobs (lower latency, higher memory). **`on_demand`**: load before each transcription and unload after (lower idle memory). |
| `warm_on_daemon_start` | boolean | `true` | When `mode = "keep_warm"`, load the model when the daemon starts. Ignored for `on_demand`. |
| `idle_unload_seconds` | integer | `900` | When `mode = "keep_warm"` and the model is loaded, unload after this many seconds without use. Set `0` to disable idle unload. |

## `[asr.hallucination_filter]` options

Filters hallucination phrases Whisper sometimes emits on silence or noise. The filter
applies only to transcripts of **five words or fewer**; longer output is never dropped
by this check.

Each phrase is compared after normalization: internal whitespace is collapsed,
case is folded to lowercase, and leading or trailing punctuation is stripped.
A phrase ending in `*` is a **prefix** matcher (`*` is removed before normalization);
all other phrases require an **exact** normalized match.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `true` | When true, drop short transcripts that match a listed phrase. |
| `phrases` | array of strings | see defaults | Phrases to treat as empty output. Suffix `*` for prefix match; otherwise exact after normalization. |

Default phrases:

```toml
phrases = [
  "thank you.",
  "thanks for watching.",
  "subtitles by*",
  "subtitle by*",
  "captioned by*",
]
```

## Model selection

- Use [Setup wizard](/setup/) to benchmark candidates, or manage them directly
  with `skald models`.
- Use `skald models recommend` for a read-only recommendation based on CPU, RAM,
  NVIDIA/CUDA availability, installed catalog models, and current config.
- CPU-safe recommendation: `small.en` with `gpu = false` and `on_demand`.
- NVIDIA recommendation: `large-v3-turbo-q5` for final transcription and
  `small.en-q5` for text preview, after validating CUDA performance and VRAM
  use on the target system.
- Visualizer-only overlay mode does not require a preview model.
- After changing `model_path` or `gpu`, restart the daemon or run `skald asr restart`.

Catalog downloads are source-controlled metadata, not remote configuration.
Files are streamed to the model filesystem, verified by exact size and SHA-256,
then renamed into place. Paths outside the catalog remain supported but are
reported as unverified and are never deleted by managed cleanup.

CPU-only selection:

```bash
skald models recommend
skald models install small.en
skald models select small.en
skald models select-preview small.en
skald config profile cpu-safe
```

NVIDIA/CUDA selection:

```bash
skald models recommend
skald models install large-v3-turbo-q5
skald models install small.en-q5
skald models select large-v3-turbo-q5
skald models select-preview small.en-q5
skald config profile power-user-nvidia
```

## Notes

- Preview uses a **separate** small model configured in `[preview]`, not `asr.model_path`.
- Benchmark timings: [Linux benchmarks](/linux/benchmarks/).
