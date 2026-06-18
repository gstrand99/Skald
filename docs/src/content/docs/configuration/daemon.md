---
title: "[daemon]"
description: Daemon logging and IPC limits.
---

Controls the headless `skaldd` process. These values apply at daemon startup.

```toml
[daemon]
log_level = "info"
max_concurrent_jobs = 1
protocol_version = 1
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `log_level` | string | `"info"` | Rust tracing filter for daemon logs. Common values: `error`, `warn`, `info`, `debug`, `trace`. Also respects `RUST_LOG` when set. |
| `max_concurrent_jobs` | integer | `1` | Maximum parallel dictation jobs. **Must be `1` in v1.** Validation fails for any other value. |
| `protocol_version` | integer | `1` | IPC protocol version between `skald` and `skaldd`. **Must be `1` in v1.** |

## Notes

- Changing `log_level` affects a running daemon only after restart.
- v1 intentionally allows one active recording/transcription pipeline at a time.
