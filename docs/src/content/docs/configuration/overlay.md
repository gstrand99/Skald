---
title: "[overlay]"
description: Graphical preview overlay window settings.
---

Configures `voxline-overlay`, a separate process that subscribes to preview events.
Requires `[preview].enabled = true`.

```toml
[overlay]
margin_px = 16
max_width_px = 720
anchor = "auto"
use_layer_shell = true
hide_when_idle = true
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
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
| SSH / headless | Use `voxline watch` instead |

## Launch

```bash
voxline overlay
```

Closing the overlay window does not stop recording. The overlay reconnects after daemon restarts.

## Notes

- GNOME does not implement `wlr-layer-shell`; overlay cannot dock like on Hyprland.
- Overlay does not block or slow the daemon; IPC is one-way event streaming.
