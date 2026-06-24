---
title: Performance diagnostics
description: Local performance diagnostics and privacy boundaries.
---

Skald performance diagnostics are optional and disabled by default.

```toml
[diagnostics]
enabled = false
max_records = 50
```

When enabled, `skaldd` keeps a bounded in-memory ring of recent redacted
performance records. Records are cleared when the daemon exits or when you run:

```sh
skald diagnostics clear
```

Inspect retained records:

```sh
skald diagnostics performance
skald diagnostics performance --json
```

Run a local fixture benchmark without emitting transcript text:

```sh
skald diagnostics benchmark sample.wav
```

Include only high-level performance warnings in doctor output:

```sh
skald doctor --include-performance
```

Collected fields include:

- stage durations in milliseconds
- ASR real-time factor
- cleanup duration, failure, and fallback state
- clipboard and paste outcome categories
- preview enabled/effective state
- process resident memory when available
- GPU memory as `unavailable` until a reliable local adapter exists
- build version, model filename or managed ID, acceleration backend, thread count, and coarse desktop session data

Diagnostics never include audio samples, transcript text, clipboard contents,
prompt text, API keys, provider request or response bodies, active window titles,
application document names, or home-directory paths.

Measurement values distinguish `value`, `not_attempted`, `failed`, and
`unavailable` so zero-duration values are not confused with missing data.
