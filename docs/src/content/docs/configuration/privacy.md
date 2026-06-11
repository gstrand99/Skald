---
title: "[privacy]"
description: Local storage and transcript logging controls.
---

All options default to **off** for a local-first, privacy-safe baseline.

```toml
config_version = 1

[privacy]
store_history = false
store_audio = false
store_raw_transcript = false
store_cleaned_transcript = false
log_transcripts = false
```

`config_version` must be `1` in v1. Future releases may use it for migrations.

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `store_history` | boolean | `false` | **Reserved / not implemented.** Must stay `false`; validation rejects `true`. |
| `store_audio` | boolean | `false` | When false, temporary WAV files are deleted after jobs complete (unless a code path explicitly retains them, such as setup fixtures). When true, retain recorded audio on disk. |
| `store_raw_transcript` | boolean | `false` | **Reserved / not implemented.** Must stay `false`; validation rejects `true`. |
| `store_cleaned_transcript` | boolean | `false` | **Reserved / not implemented.** Must stay `false`; validation rejects `true`. |
| `log_transcripts` | boolean | `false` | When true, daemon debug logs may include transcript text. **Keep false** unless debugging in a controlled environment. |

## Doctor and warnings

`voxline doctor` reports when `store_audio`, `log_transcripts`, or
`emit_transcript_in_events` is enabled. Reserved storage flags are not treated as
active controls.

Cloud cleanup is configured separately in `[cleanup]` and sends transcript text off-device only when cleanup runs—not via these flags.

## Related settings

- `[cleanup].enabled` — opt-in cloud processing of transcript text
- `[secrets]` — API keys never stored in `config.toml`
- Daemon socket `0600`, runtime directory `0700` — see [Troubleshooting](/troubleshooting/)

## Notes

- Enabling `log_transcripts` can leak dictated content into journald or terminal output when `voxlined` runs in the foreground.
- Setup wizard fixture audio is stored at `~/.local/share/voxline/models/samples/setup.wav` (under `paths.model_dir`) regardless of `store_audio`.
