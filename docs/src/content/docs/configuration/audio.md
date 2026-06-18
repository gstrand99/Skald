---
title: "[audio]"
description: Microphone capture and speech detection gates.
---

Audio is captured through CPAL (PipeWire or PulseAudio on Linux). v1 requires
**16 kHz mono** output for the ASR pipeline regardless of the input device format.

```toml
[audio]
backend = "cpal"
device = "default"
target_sample_rate = 16000
channels = 1
max_record_seconds = 300

[audio.gates]
min_record_ms = 350
min_rms_energy = 0.003
min_peak_energy = 0.015
notify_on_no_speech = true
```

## `[audio]` options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `backend` | string | `"cpal"` | Audio capture backend. Only `cpal` is supported in v1. |
| `device` | string | `"default"` | Input device name or `"default"` for the system default microphone. |
| `target_sample_rate` | integer | `16000` | Sample rate written to WAV files and sent to Whisper. **Must be `16000` in v1.** |
| `channels` | integer | `1` | Output channel count after downmix. **Must be `1` in v1.** |
| `max_record_seconds` | integer | `300` | Safety cap on recording length for a single job (5 minutes by default). |

## `[audio.gates]` options

Gates run when a recording stops. Failed gates produce a `no_speech` error instead
of running ASR.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `min_record_ms` | integer | `350` | Minimum recording duration in milliseconds. Shorter clips are rejected. |
| `min_rms_energy` | float | `0.003` | Minimum RMS energy across the clip. Lower values accept quieter speech. |
| `min_peak_energy` | float | `0.015` | Minimum peak sample energy. Helps reject near-silent recordings. |
| `notify_on_no_speech` | boolean | `true` | When `[notifications].enabled` is true, show a desktop notification if gates reject a clip. |

## Tuning gates

- Increase `min_rms_energy` or `min_peak_energy` in noisy environments to reduce false triggers.
- Decrease them if legitimate speech is rejected (`skald doctor` and failed toggles with `no_speech`).
- Setup wizard recording uses the same gates as normal dictation.

## Notes

- Stereo input is mixed to mono before resampling.
- Preview uses a separate RMS threshold in `[preview].min_rms_energy` for chunk gating.
