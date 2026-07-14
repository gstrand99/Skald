---
title: macOS
description: Install and run Skald on Apple Silicon macOS.
---

The macOS port targets Apple Silicon and macOS 14 or newer. It packages the
Rust dictation daemon and CLI inside a native menu-bar app with a global
shortcut and preview overlay.

Build locally with `just build-macos`. Create an ad-hoc signed app and disk
image with `just macos-package`.

If Hugging Face's direct model URL returns HTTP 403, install its official Xet
client with `brew install hf` and rerun `skald models install`. Skald detects
the client automatically and still verifies the downloaded model checksum.

Skald uses CoreAudio through CPAL, Whisper Metal acceleration, the macOS
pasteboard, Keychain, and a per-user LaunchAgent. Models and dictated audio
remain local unless OpenRouter cleanup is explicitly enabled.

The daemon starts the signed `skald-native` helper as a long-lived broker. A
versioned JSON protocol travels only over inherited stdin/stdout pipes, so no
public broker socket or transcript-bearing command line is created. Requests
are size-limited and operation-specific. If the broker exits, Skald restarts it
once and retains the one-shot native and clipboard-only recovery paths.

## Permissions

Skald needs microphone access to record speech. Safe paste additionally needs
Accessibility access so the native helper can send Command-V. If Accessibility
is denied, dictation still copies the final result to the clipboard.

The menu-bar app streams daemon state and audio levels into a non-activating
overlay. Enable `[preview].enabled` and install a preview model to also show
ephemeral realtime text; preview text is cleared when each job ends. The global
shortcut can be changed from the menu and is retained in macOS preferences.
The `skald overlay` command opens this native app; Linux-only overlay preview
flags are not available on macOS.

Run `just test-macos-broker` to check the broker protocol and Accessibility
status. `skald doctor` reports broker availability and keeps safe paste disabled
when Accessibility permission is absent.

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
