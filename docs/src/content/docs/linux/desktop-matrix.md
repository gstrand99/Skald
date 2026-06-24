---
title: Desktop matrix
description: Manual Linux session validation checklist.
---

Manual checklist for the first Linux release. Run after `just check` and
`just release` or `just release-cuda`.

For **Linux 1.0**, only **Hyprland Wayland** is validated and signed off. The
sessions table below records expected behavior from code and documentation on
other desktops. Those rows are **not** signed off until manual validation
completes on that session.

## Sessions

| Session | Validated for 1.0 | Doctor | Toggle record | Clipboard | Safe paste | Preview overlay | Waybar | Tray | Notes |
|---------|-------------------|--------|---------------|-----------|------------|-----------------|--------|------|-------|
| Hyprland Wayland | yes | yes | yes | yes | yes | yes | yes | | Primary dev target; layer-shell overlay |
| Sway Wayland | no | | | | | | | depends on bar | Requires a StatusNotifier host |
| GNOME Wayland | no | | | | | | | extension required | Install an AppIndicator extension |
| KDE Wayland | no | | | | | | | expected | Native StatusNotifier support |
| X11 | no | | | | | | | desktop dependent | xdotool paste when installed |
| SSH / headless | no | | | | | n/a | n/a | `skald watch` only; no desktop clients |

## Privacy checks

- `[privacy]` defaults: no storage, no transcript logging
- `skald doctor` reports sensitive options when enabled
- Cleanup off by default; enabling shows doctor warning
- Daemon socket mode `0600`, runtime dir mode `0700`

## Sign-off

| Session | Machine | Profile | Validated | Notes |
|---------|---------|---------|-----------|-------|
| Hyprland Wayland | Ryzen 5900X + RTX 3070 Ti | `power-user-nvidia` | 2026-06-18 | Doctor, toggle, clipboard, safe paste, layer-shell overlay, Omarchy Waybar module |

Other sessions remain unchecked for 1.0. Revisit this page before claiming
broader desktop support in release notes.

Record additional machines, profiles, model files, and revised latency notes in
[Benchmark results](/linux/benchmarks/) before tagging a release candidate.
