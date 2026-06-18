---
title: "[notifications]"
description: Desktop notifications for errors and clipboard-only fallback.
---

```toml
[notifications]
enabled = true
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `true` | When true, send desktop notifications via `notify-send` for selected events. |

## When notifications fire

Examples (when enabled):

- No speech detected after audio gates reject a recording (`[audio.gates].notify_on_no_speech`)
- Clipboard-only fallback because paste was unsafe (`[injection].notify_on_clipboard_only`)
- Empty or failed transcription messages from the daemon

## Requirements

- `notify-send` must be on `PATH` for notifications to appear.
- `skald doctor` lists whether `notify-send` is available.

## Notes

- Notifications are non-blocking; they do not delay dictation completion.
- Headless SSH sessions typically have no notification daemon; failures are silent.
