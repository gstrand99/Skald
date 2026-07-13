---
title: macOS
description: Install and run Skald on Apple Silicon macOS.
---

The macOS port targets Apple Silicon and macOS 14 or newer. It packages the
Rust dictation daemon and CLI inside a native menu-bar app with a global
shortcut and preview overlay.

Build locally with `just build-macos`. Create an ad-hoc signed app and disk
image with `just macos-package`.

Skald uses CoreAudio through CPAL, Whisper Metal acceleration, the macOS
pasteboard, Keychain, and a per-user LaunchAgent. Models and dictated audio
remain local unless OpenRouter cleanup is explicitly enabled.

## Permissions

Skald needs microphone access to record speech. Safe paste additionally needs
Accessibility access so the native helper can send Command-V. If Accessibility
is denied, dictation still copies the final result to the clipboard.

Previous-clipboard restoration is disabled by default on macOS. Enabling it
causes Skald to read the existing pasteboard before writing the transcript and
may trigger macOS pasteboard privacy controls.

## Files

- Configuration: `~/Library/Application Support/Skald/config.toml`
- Models: `~/Library/Application Support/Skald/models`
- Runtime socket: `~/Library/Caches/Skald/run/skaldd.sock`
- LaunchAgent: `~/Library/LaunchAgents/com.gstrand.skald.daemon.plist`

Uninstalling the app does not remove configuration, models, styles, snippets,
or Keychain entries.
