---
title: Service & shortcuts
description: systemd user service and compositor keybindings.
---

## systemd user service

Install the user-session daemon:

```bash
voxline service install
systemctl --user start voxlined
```

`voxline service install` writes `~/.config/systemd/user/voxlined.service`, enables
it, and prints shortcut binding examples for your desktop session.

Check status:

```bash
voxline service status
systemctl --user status voxlined
```

## Compositor shortcuts

VoxLine does not capture global hotkeys inside the daemon. Bind your compositor or
desktop environment to an external command:

| Action | Command |
|--------|---------|
| Toggle dictation | `voxline toggle` |
| Push-to-talk start | `voxline start` or `voxline ptt-start` |
| Push-to-talk stop | `voxline stop` or `voxline ptt-stop` |

Example (Hyprland):

```ini
bind = $mainMod, SPACE, exec, voxline toggle
bind = $mainMod, V, exec, voxline start
bindr = $mainMod, V, exec, voxline stop
```

Push-to-talk requires key-release bindings where your compositor supports them.
Otherwise use toggle mode.

## Session environment (Hyprland / Sway)

Import the graphical session into systemd before starting the service:

```bash
systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP DBUS_SESSION_BUS_ADDRESS
```

`voxline service install` and `voxline doctor` print session-specific import lines
when they detect a mismatch between the CLI and daemon environment.

## Paste safety

Safe paste is supported on X11 (`xdotool`), Hyprland (`hyprctl` Shift+Insert), and
Sway (`wtype`). GNOME Wayland, KDE Wayland, and unknown sessions default to
clipboard-only output.

VoxLine captures the active target when recording starts, when it stops, and
immediately before paste. A changed, unknown, or stale target falls back to
clipboard-only with an optional notification.
