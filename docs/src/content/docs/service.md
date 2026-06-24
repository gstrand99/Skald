---
title: Service & shortcuts
description: systemd user service and compositor keybindings.
---

## systemd user service

Install the user-session daemon:

```bash
skald service install
systemctl --user start skaldd
```

`skald service install` writes `~/.config/systemd/user/skaldd.service`, enables
it, and prints shortcut binding examples for your desktop session.

Check status:

```bash
skald service status
systemctl --user status skaldd
```

Stop or restart the service:

```bash
skald service stop
skald service restart
```

Remove the unit:

```bash
skald service uninstall
```

`skald service uninstall` disables the unit, stops it if running, and removes
`~/.config/systemd/user/skaldd.service`.

## Compositor shortcuts

Skald does not capture global hotkeys inside the daemon. Bind your compositor or
desktop environment to an external command:

| Action | Command |
|--------|---------|
| Toggle dictation | `skald toggle` |
| Push-to-talk start | `skald start` or `skald ptt-start` |
| Push-to-talk stop | `skald stop` or `skald ptt-stop` |

Example (Hyprland):

```ini
bind = $mainMod, SPACE, exec, skald toggle
bind = $mainMod, V, exec, skald start
bindr = $mainMod, V, exec, skald stop
```

Push-to-talk requires key-release bindings where your compositor supports them.
Otherwise use toggle mode.

## Session environment (Hyprland / Sway)

Import the graphical session into systemd before starting the service:

```bash
systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP DBUS_SESSION_BUS_ADDRESS
```

`skald service install` and `skald doctor` print session-specific import lines
when they detect a mismatch between the CLI and daemon environment.

## Paste safety

Safe paste is supported on X11 (`xdotool`), Hyprland (`hyprctl` Shift+Insert), and
Sway (`wtype`). GNOME Wayland, KDE Wayland, and unknown sessions default to
clipboard-only output.

Skald captures the active target when recording starts, when it stops, and
immediately before paste. A changed, unknown, or stale target falls back to
clipboard-only with an optional notification.
