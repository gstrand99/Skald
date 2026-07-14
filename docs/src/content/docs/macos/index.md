---
title: macOS
description: Install, operate, and release Skald on Apple Silicon macOS.
---

Skald supports Apple Silicon and macOS 14 or newer. The disk image contains a
native menu-bar app, the Rust dictation daemon and CLI, and a signed native
helper. Models and dictated audio remain local unless OpenRouter cleanup is
explicitly enabled.

## Install

Download `Skald-arm64.dmg` and `Skald-arm64.dmg.sha256` from the matching GitHub
release. Put both files in the same directory and verify the download:

```sh
shasum -a 256 -c Skald-arm64.dmg.sha256
```

Open the disk image, drag Skald to Applications, and open Skald normally. Do not
bypass a Gatekeeper failure; report it against the release instead. Choose
**Install or repair daemon** from the Skald menu.

The CLI is embedded in the app. These examples use a convenience variable:

```sh
SKALD_BIN=/Applications/Skald.app/Contents/Resources/bin/skald
"$SKALD_BIN" models install small.en-q5 --select
"$SKALD_BIN" doctor
```

If Hugging Face's direct model URL returns HTTP 403, install its official Xet
client with `brew install hf` and rerun the model command. Skald detects the
client automatically and still verifies the downloaded model checksum.

## Permissions and operation

Skald asks for microphone access to record speech. Safe paste additionally
needs Accessibility access so the native helper can send Command-V. Use
**Grant Accessibility access** in the Skald menu and enable the listed Skald
helper in System Settings. If access is denied, dictation still copies the
final result to the clipboard.

The global shortcut is selected in the menu and does not require Accessibility
or Input Monitoring. If macOS reports that the shortcut is unavailable, choose
another combination. Terminal targets intentionally remain clipboard-only;
paste there manually with Command-V.

The menu app streams daemon state and audio levels into a non-activating
overlay. Enable `[preview].enabled` and install a preview model to also show
ephemeral realtime text. Preview text is cleared when each job ends. The
`skald overlay` command opens the native app; Linux-only overlay preview flags
are unavailable on macOS.

The daemon starts `skald-native` as a long-lived broker over inherited
stdin/stdout pipes. It creates no public broker socket or transcript-bearing
command line. Requests are versioned, operation-specific, and size-limited. If
the broker exits, Skald restarts it once and retains clipboard-only recovery.

Run `just test-macos-broker` in a source checkout to test the broker protocol.
`skald doctor` reports broker and permission state. Previous-clipboard
restoration is disabled by default because enabling it requires reading the
existing pasteboard and may trigger macOS privacy controls.

## Upgrade

Before upgrading, quit Skald and save a copy of
`~/Library/Application Support/Skald/config.toml`. Verify the new DMG, replace
Skald in Applications, and reopen it. Then run:

```sh
SKALD_BIN=/Applications/Skald.app/Contents/Resources/bin/skald
"$SKALD_BIN" config upgrade
```

Choose **Install or repair daemon** so the LaunchAgent points to the new signed
daemon. Models, configuration, styles, snippets, and Keychain secrets remain in
their user locations and are not replaced by the app.

## Rollback

Quit Skald, verify and reinstall the previous release, and restore the config
copy made before upgrading if the older binary cannot read the migrated
configuration. Reopen the previous app and choose **Install or repair daemon**.
Config migrations are not assumed to be reversible.

## Uninstall

Remove the LaunchAgent before deleting the app:

```sh
SKALD_BIN=/Applications/Skald.app/Contents/Resources/bin/skald
"$SKALD_BIN" service uninstall
"$SKALD_BIN" secrets clear openrouter  # only for a complete data purge
```

Quit Skald and move it from Applications to the Trash. Normal uninstall keeps
configuration, models, styles, snippets, and Keychain entries. For a complete
purge, remove these retained directories after confirming they contain nothing
you need:

```sh
rm -rf "$HOME/Library/Application Support/Skald"
rm -rf "$HOME/Library/Caches/Skald"
```

The installed LaunchAgent is
`~/Library/LaunchAgents/com.gstrand.skald.daemon.plist`.

## Build and release

Local development builds use ad-hoc signing:

```sh
just build-macos
just macos-package
just macos-release-check
```

The release check verifies every nested code signature and arm64 architecture,
an exact privacy-safe app-content allowlist, the DMG checksum and filesystem,
the Applications link, and execution after copying the app from the mounted
image. Gatekeeper and notarization checks run when the app has a Developer ID
signature.

Published releases are built from the matching version tag on `main`. Store
notary credentials in a Keychain profile rather than passing secrets on the
command line. Then package, submit, staple, and validate:

```sh
export SKALD_CODESIGN_IDENTITY='Developer ID Application: Example (TEAMID)'
export SKALD_BUILD_NUMBER=1
export SKALD_NOTARY_PROFILE=skald-notary
just macos-package
just macos-notarize
SKALD_REQUIRE_DEVELOPER_ID=1 just macos-release-check
just macos-release-checklist
```

Upload only the DMG, its SHA-256 file, and concise release notes. Record the
notary submission ID and complete the printed checklist on a clean macOS user
account before publishing.
