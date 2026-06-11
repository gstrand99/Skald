---
title: "[privacy]"
description: Local storage and transcript logging controls.
---

All options default to **off** for a local-first, privacy-safe baseline.

```toml
[privacy]
store_history = false
store_audio = false
store_raw_transcript = false
store_cleaned_transcript = false
log_transcripts = false
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `store_history` | boolean | `false` | Reserved for future persistent dictation history. v1 does not write history files when false. |
| `store_audio` | boolean | `false` | When false, temporary WAV files are deleted after jobs complete (unless a code path explicitly retains them, such as setup fixtures). When true, retain recorded audio on disk. |
| `store_raw_transcript` | boolean | `false` | Reserved for persisting raw ASR output to disk. |
| `store_cleaned_transcript` | boolean | `false` | Reserved for persisting cleanup output to disk. |
| `log_transcripts` | boolean | `false` | When true, daemon debug logs may include transcript text. **Keep false** unless debugging in a controlled environment. |

## Doctor and warnings

`voxline doctor` reports when any sensitive storage or logging option is enabled.

Cloud cleanup is configured separately in `[cleanup]` and sends transcript text off-device only when cleanup runs—not via these flags.

## Related settings

- `[cleanup].enabled` — opt-in cloud processing of transcript text
- `[secrets]` — API keys never stored in `config.toml`
- Daemon socket `0600`, runtime directory `0700` — see [Troubleshooting](/troubleshooting/)

## Notes

- Enabling `log_transcripts` can leak dictated content into journald or terminal output when `voxlined` runs in the foreground.
- Setup wizard fixture audio is stored at `~/.local/share/voxline/samples/setup.wav` regardless of `store_audio` (separate setup path).
