---
title: Tray client
description: Run the optional Linux StatusNotifier/AppIndicator client.
---

`skald-tray` is a separate user-session process. It subscribes to daemon events
and reconnects with bounded backoff after daemon restarts. Closing it does not
stop recording or the daemon.

The menu provides:

- daemon and dictation state
- start, stop, cancel, and toggle through typed IPC
- overlay launch and close actions plus configured mode and visualizer style
- configured microphone and a microphone-test shortcut
- cleanup status with an explicit network and cost warning
- configuration and documentation shortcuts
- daemon restart and tray quit actions

The tray does not display or retain transcript content. Dictation actions still
use the daemon's normal target-safety and paste pipeline.

## Run

```bash
just tray
```

Installed builds can run `skald-tray` directly.

## Autostart

Create `~/.config/autostart/skald-tray.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=Skald Tray
Comment=Skald dictation status and controls
Exec=skald-tray
Terminal=false
X-GNOME-Autostart-enabled=true
```

For Hyprland or Sway, the equivalent compositor startup command is:

```text
exec-once = skald-tray
```

## Desktop support

The client requires a D-Bus user session and a StatusNotifier/AppIndicator host.
It exits with an actionable error if those are unavailable.

- KDE has native StatusNotifier support.
- GNOME requires an AppIndicator/KStatusNotifierItem extension.
- Hyprland and Sway require a bar or panel with StatusNotifier support.
- X11 support depends on the selected desktop panel.
- Headless and SSH sessions should use `skald status`, `skald watch`, or
  `skald waybar` instead.
