---
title: "[injection]"
description: Clipboard output, safe paste, and Linux session adapters.
---

Controls how final text reaches the clipboard and optionally the focused application.

```toml
[injection]
copy_to_clipboard = true
auto_paste = "safe"
max_paste_age_ms = 5000
restore_clipboard = true
paste_delay_ms = 120
fallback_to_clipboard_only = true
notify_on_clipboard_only = true

[injection.linux]
session = "auto"
wayland_paste_command = "wtype -M ctrl -k v -m ctrl"
x11_paste_command = "xdotool key ctrl+v"
gnome_wayland_mode = "clipboard_only"
optional_paste_command = ""
```

## `[injection]` options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `copy_to_clipboard` | boolean | `true` | Copy final text to the system clipboard. Required when `auto_paste` is not `off`. |
| `auto_paste` | string | `"safe"` | **`off`**: never paste; clipboard only. **`safe`**: paste only when the active target is stable and known at start, stop, and paste time, and within `max_paste_age_ms`. **`always`**: attempt paste when a supported backend exists, skipping target-stability and max-age checks; session-support and terminal guards still apply. |
| `max_paste_age_ms` | integer | `5000` | In `safe` mode, maximum milliseconds between stop and paste. Older targets fall back to clipboard-only. |
| `restore_clipboard` | boolean | `true` | Save clipboard contents before copying dictation text; restore after a **successful** paste when true. |
| `paste_delay_ms` | integer | `120` | Delay in milliseconds before paste and before clipboard restore (allows focus to settle). |
| `fallback_to_clipboard_only` | boolean | `true` | When paste cannot be verified safe, leave text on the clipboard instead of failing the job. |
| `notify_on_clipboard_only` | boolean | `true` | When `[notifications].enabled` is true, notify the user if paste was skipped and text is clipboard-only. |

## `[injection.linux]` options

Linux-specific paste routing settings. These keys are validated at config load but
**not yet wired to runtime paste behavior**. Paste backends are currently hardcoded:
Hyprland uses `hyprctl dispatch sendshortcut SHIFT,Insert,activewindow`; Sway uses
`wtype`; X11 uses `xdotool`.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `session` | string | `"auto"` | Reserved for future session routing. **`auto`** is the only supported value in v1. |
| `wayland_paste_command` | string | `wtype ...` | Reserved; not used at runtime yet. |
| `x11_paste_command` | string | `xdotool key ctrl+v` | Reserved; not used at runtime yet. |
| `gnome_wayland_mode` | string | `"clipboard_only"` | Reserved for future GNOME routing. **`clipboard_only`** or **`custom`** (with `optional_paste_command`) are validated only. |
| `optional_paste_command` | string | `""` | Reserved; required when `gnome_wayland_mode = "custom"`. |

## Platform behavior (v1)

| Environment | Paste backend |
|-------------|---------------|
| Hyprland | `hyprctl` Shift+Insert |
| Sway | `wtype` |
| X11 | `xdotool` |
| GNOME / KDE Wayland | Clipboard-only by default |
| Terminals | Often clipboard-only unless compositor supports safe target verification |

Application profiles can set `prefer_clipboard_only` per app. See [Related files](/configuration/related-files/).

## Testing

```bash
voxline test clipboard
voxline test paste
voxline doctor    # paste capability report
```

## Notes

- Safe paste captures target context at recording start, stop, and immediately before paste.
- Changing targets during dictation forces clipboard-only output when `auto_paste = "safe"`.
