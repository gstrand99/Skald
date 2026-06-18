---
title: Troubleshooting
description: Doctor checks, common failures, and privacy verification.
---

## Run the doctor

```bash
skald doctor
```

Doctor reports session type, desktop, tool availability, config validity, runtime
and socket permissions, daemon reachability, ASR model presence, paste backend,
secrets status, and privacy flags. Follow printed **Suggestions** for remediation.

Common suggestions:

- Run `skald setup` when first-time configuration is incomplete.
- Start the daemon: `skaldd --foreground` or `systemctl --user start skaldd`.
- Download a GGML model to the configured `asr.model_path`.
- Import the graphical session into systemd when CLI and daemon environments differ.
- Run `skald secrets set openrouter` before enabling cleanup.

## Failure modes

| Scenario | Expected behavior |
|----------|-------------------|
| Missing microphone | Start/toggle returns a clear error |
| Missing ASR model | Doctor warns; transcribe fails cleanly |
| Missing preview model | Doctor warns when preview enabled |
| OpenRouter key missing | Cleanup preview fails; dictation falls back to raw text |
| Cleanup timeout / error | Raw transcript used when fallback enabled |
| Target changes before paste | Clipboard-only with notification |
| Short / silent recording | Rejected by audio gates |
| Locked keyring | Secrets status reports unavailable |
| Daemon restart | Overlay/watch reconnect |
| Stale socket permissions | Doctor reports socket not secure; restart fixes |

## Cannot connect to socket

```text
cannot connect to .../skaldd.sock; is skaldd running?
```

Start the daemon:

```bash
skaldd --foreground
# or
systemctl --user start skaldd
```

Verify the socket is user-owned mode `0600`:

```bash
ls -la $XDG_RUNTIME_DIR/skald/skaldd.sock
```

## CUDA build mismatch

If `asr.gpu = true` but `skaldd` was built CPU-only, model load fails with an
unsupported-feature error. Rebuild with `just release-cuda` or set `gpu = false`.

## Privacy checks

- `[privacy]` defaults: no storage, no transcript logging
- `skald doctor` reports sensitive options when enabled
- Cleanup off by default; enabling shows a doctor warning
- Daemon socket mode `0600`, runtime dir mode `0700`

## Desktop validation

See the [Desktop matrix](/linux/desktop-matrix/) for session-by-session support
and manual sign-off notes.
