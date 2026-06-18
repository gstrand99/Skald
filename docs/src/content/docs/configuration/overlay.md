---
title: "[overlay]"
description: Graphical preview overlay window settings.
---

Configures `skald-overlay`, a separate process that subscribes to preview events.
Text mode requires `[preview].enabled = true`; visualizer mode does not.

```toml
[overlay]
mode = "text"
visualizer_style = "waveform"
margin_px = 16
max_width_px = 720
anchor = "auto"
use_layer_shell = true
hide_when_idle = true
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `mode` | string | `"text"` | **`text`** shows stable/provisional transcription. **`visualizer`** shows microphone level bars without displaying transcript text or requiring preview ASR. |
| `visualizer_style` | string | `"waveform"` | Visualizer appearance: **`waveform`**, **`bars`**, **`pulse`**, or **`dots`**. Ignored in text mode. |
| `margin_px` | integer | `16` | Margin from screen edges or cursor anchor in pixels. |
| `max_width_px` | integer | `720` | Maximum overlay width in pixels. |
| `anchor` | string | `"auto"` | **`top`**: full-width bar at top. **`bottom`**: full-width bar at bottom. **`auto`**: place near cursor on Hyprland/X11 when supported; otherwise fall back to floating window behavior. |
| `use_layer_shell` | boolean | `true` | Prefer `wlr-layer-shell` on compositors that support it (Hyprland, Sway, River). |
| `hide_when_idle` | boolean | `true` | Hide overlay when there is no active recording or preview text. |

## Session behavior

| Session | Behavior |
|---------|----------|
| Hyprland / X11 | `anchor = "auto"` places preview near cursor |
| Hyprland / Sway / River | `top` / `bottom` use layer-shell bars |
| GNOME Wayland | Floating GTK window; limited positioning |
| SSH / headless | Use `skald watch` instead |

## Launch

```bash
skald overlay
```

Closing the overlay window does not stop recording. The overlay reconnects after daemon restarts.

## Visualizer mode

Set `mode = "visualizer"` for recording feedback without scrolling text. The daemon sends
rate-limited normalized RMS and peak levels to the overlay; raw audio is never sent over IPC.
Visualizer mode works with `[preview].enabled = false`, so it does not load the preview model.
Text and visualizer modes are separate in this release.

Available styles:

| Style | Appearance |
|-------|------------|
| `waveform` | Scrolling mirrored waveform history |
| `bars` | Seven vertical level bars |
| `pulse` | A centered circle that expands with input level |
| `dots` | Scrolling mirrored dots |

To validate it manually:

1. Set `overlay.mode = "visualizer"`, choose `overlay.visualizer_style`, and optionally set `preview.enabled = false`.
2. Restart `skaldd`, then run `just overlay`.
3. Start a recording and confirm the bars react to normal speech, settle after speech stops,
   and disappear or return to idle when recording ends.

## Notes

- GNOME does not implement `wlr-layer-shell`; overlay cannot dock like on Hyprland.
- Overlay does not block or slow the daemon; IPC is one-way event streaming.
