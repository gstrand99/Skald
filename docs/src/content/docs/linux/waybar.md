---
title: Waybar
description: Add privacy-safe Skald status and controls to Waybar.
---

`skald waybar` subscribes to daemon state and error events and writes one JSON
object per update. It reconnects after daemon restarts and does not emit
transcript, clipboard, window-title, or audio content.

Add `"custom/skald"` to the preferred module list in
`~/.config/waybar/config.jsonc`, then define:

```jsonc
"custom/skald": {
  "exec": "$HOME/.local/bin/skald waybar",
  "return-type": "json",
  "restart-interval": 5,
  "on-click": "$HOME/.local/bin/skald toggle",
  "on-click-right": "$HOME/.local/bin/skald cancel",
  "on-click-middle": "$HOME/.local/bin/skald overlay",
  "tooltip": true
}
```

The command emits stable classes: `idle`, `recording`, `transcribing`,
`cleaning`, `error`, and `disconnected`.

Example `~/.config/waybar/style.css`:

```css
#custom-skald {
  padding: 0 8px;
}

#custom-skald.recording {
  color: #f38ba8;
}

#custom-skald.transcribing {
  color: #89b4fa;
}

#custom-skald.cleaning {
  color: #cba6f7;
}

#custom-skald.error,
#custom-skald.disconnected {
  color: #f9e2af;
}
```

On Omarchy, apply the changes with:

```bash
omarchy restart waybar
```

Use `just waybar` to inspect the JSON stream directly. Click actions use the
normal CLI and daemon safety pipeline.
