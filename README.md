# Skald

Skald is a Linux-first, local-first dictation daemon and CLI. It records,
transcribes locally, copies the final text to the clipboard, and pastes only
when the active target can be verified as stable.

## Configuration

Skald reads a single TOML file at `~/.config/skald/config.toml`. If the file
is missing, built-in defaults apply (same values as the Linux example below).

A commented reference copy lives in
[`config-example/linux/config.toml`](config-example/linux/config.toml). Platform
stubs for future ports are under [`config-example/`](config-example/).

### Setup

```bash
skald config init          # write config.toml and scaffold directories
skald config validate      # migrate in memory and validate current rules
skald config upgrade       # persist migrations and newly defaulted fields
skald config path          # print the active config path
skald doctor               # session, paths, daemon, and privacy checks
```

`config init` creates:

```text
~/.config/skald/
  config.toml
  styles/      # cleanup styles
  apps/        # per-application profiles
  snippets/    # insert snippets
~/.local/share/skald/models/
$XDG_RUNTIME_DIR/skald/    # WAV files, Unix socket (when runtime_dir = "auto")
```

Runtime files and the daemon socket use `paths.runtime_dir`. The default `auto`
resolves to `$XDG_RUNTIME_DIR/skald`.

### Preset profiles

```bash
skald config profile power-user-nvidia   # large CUDA model, keep_warm lifecycle
skald config profile cpu-safe            # small.en CPU model, on_demand lifecycle
```

`power-user-nvidia` resets nearly the entire config to built-in defaults,
preserving only `[secrets]` and `[cleanup]`. `cpu-safe` applies CPU-safe ASR and
lifecycle settings and disables cleanup without a full reset.

Restart `skaldd` after changing config if the daemon is already running.

### Migrating from VoxLine

Skald uses new binary, service, config, data, runtime, and keyring names. Stop
VoxLine before moving an existing installation:

```bash
systemctl --user disable --now voxlined.service
mv ~/.config/voxline ~/.config/skald
mv ~/.local/share/voxline ~/.local/share/skald
sed -i 's|/voxline|/skald|g; s|voxline/|skald/|g' ~/.config/skald/config.toml
skald config validate
skald service install
systemctl --user daemon-reload
systemctl --user start skaldd.service
```

Remove `~/.config/systemd/user/voxlined.service` and the old `voxline`,
`voxlined`, and `voxline-overlay` binaries after validating Skald. Keyring
entries are intentionally not copied; run `skald secrets set openrouter` again
if cleanup was configured through the VoxLine keyring.

### Config sections

**`[daemon]`** — Logging and IPC limits. `protocol_version` must stay `1` in v1.

**`[paths]`** — Locations for config data, Whisper models, and runtime files.
Tilde paths (`~/...`) are expanded. Use `runtime_dir = "auto"` on Linux unless you
have a specific alternate runtime path.

**`[audio]`** — CPAL capture settings. v1 requires 16 kHz mono output.
`[audio.gates]` rejects very short or quiet recordings before transcription.

**`[asr]`** — Local Whisper backend. Set `model_path` to your GGML model.
`[asr.lifecycle]` controls whether the model stays loaded between jobs.
`[asr.hallucination_filter]` drops common silence hallucination phrases.

**`[vocabulary]`** — Custom phrases for ASR biasing and deterministic
post-transcription replacements (`[[vocabulary.phrases]]`,
`[[vocabulary.replacements]]`). Manage entries with `skald vocab`.

**`[cleanup]`** — Opt-in cloud cleanup. Disabled by default (`provider = "none"`).
When enabled with OpenRouter, only transcript text is sent off-device—not audio.
Enable via `skald cleanup enable openrouter` or edit config directly. Default
model when enabled: `~openai/gpt-mini-latest`. The cleanup system prompt comes
from a style under `styles/` (default: `cleanup.default_style = "default"` loads
`styles/default.toml` → `styles/default.md`). `config init` installs the bundled
default style files; edit the `.md` file to change cleanup behavior.

Manage cleanup styles:

```bash
skald styles list
skald styles new professional
skald styles edit professional
skald styles validate
skald cleanup preview --style professional "hey john thanks"
skald toggle --style professional --cleanup
```

Application profiles under `apps/` match the active window at recording start and
can override cleanup style, disable cleanup, add a prompt layer, or prefer
clipboard-only paste (the bundled `terminal` profile does the latter two):

```bash
skald apps detect
skald apps list
skald apps edit terminal
skald apps validate
```

Snippets under `snippets/` support static insert content and template snippets:

```bash
skald snippets list
skald snippets new signature
skald snippets new standup --template
skald snippets validate
skald snippets insert signature
skald snippets preview standup "yesterday I fixed bugs today I'll ship templates blocked nothing"
skald toggle --snippet signature
```

Insert snippets copy static content directly. Template snippets use OpenRouter JSON
field extraction and render a `{{field}}` template. Route them with a voice command
such as `skald standup ...` when `[voice_commands]` is enabled.

Experimental voice commands (opt-in, disabled by default) parse a required
prefix at the **start** of the transcript after ASR. The default prefix
`skald` also matches when ASR splits it into two words (`Skald`). They can
select a cleanup style or insert a snippet when the remainder is empty:

```bash
# enable [voice_commands] in config.toml first
skald commands test "skald professional hey john thanks"
skald commands conflicts
```

**`[secrets]`** — Where to look for API keys. Keys are never stored in
`config.toml`. Use `skald secrets set openrouter` (keyring), or set
`OPENROUTER_API_KEY`, or opt into the insecure file fallback explicitly.

**`[injection]`** — Clipboard-first output. `auto_paste = "safe"` pastes only
when the active target is stable; otherwise text stays on the clipboard.
`[injection.linux]` holds session-specific paste commands for future routing;
GNOME Wayland defaults to clipboard-only.

**`[notifications]`** — Desktop notifications for errors and clipboard-only fallback.

**`[privacy]`** — Off by default. Enabling storage or transcript logging is explicit.

**`[preview]`** — Opt-in realtime transcription while recording. Preview uses a
separate small model (default `ggml-small.en.bin` on CPU) while final transcription
keeps `[asr]`. Preview text is shown in `skald watch` only; it is never copied or
pasted.

```bash
# download ggml-small.en.bin to ~/.local/share/skald/models/
# set preview.enabled = true in config.toml
skald watch          # terminal fallback
skald overlay        # graphical overlay (separate process)
skald toggle
```

**Overlay (M11)** — `skald-overlay` subscribes to daemon preview events and
renders stable/provisional text in a small overlay window. It is a separate
process and cannot block the daemon. Closing the overlay does not stop an active
recording.

| Session | Overlay behavior |
|---------|------------------|
| Hyprland / X11 | `overlay.anchor = "auto"` places preview near the cursor (above or below by available space) |
| Hyprland / Sway / River | `top` / `bottom` use a full-width layer-shell bar |
| GNOME Wayland | floating window; positioning is limited |
| Headless / SSH | use `skald watch` instead |

On GNOME Wayland, Mutter does not implement `wlr-layer-shell`, so Skald cannot
anchor a compositor overlay the way it does on Hyprland or Sway. The overlay
falls back to a normal GTK window. For a universal text-only fallback, use
`skald watch`.

### Secrets and cleanup

```bash
skald secrets set openrouter
skald cleanup enable openrouter
skald toggle --cleanup        # per-dictation cleanup
skald toggle --no-cleanup     # skip cleanup for one job
```

See [Cleanup (opt-in)](#cleanup-opt-in) below.

## Linux release

User documentation is published at [tryskald.dev](https://tryskald.dev) (source in
[`docs/`](docs/)): install, setup wizard, configuration, CLI reference,
troubleshooting, and the Linux desktop matrix. Linux 1.0 validates **Hyprland
Wayland**; other sessions are documented as best-effort.

Planned release work is tracked in [ROADMAP.md](ROADMAP.md).

```bash
# Tagged archive release
just release-archives
just release-smoke
just release-checksums
just release-sign
just release-checklist

# Local GPU source install (CUDA daemon)
just release-cuda
just install-cuda         # install without rebuilding a CPU skaldd

# Local CPU source install
just release
just install

skald version --json
skaldd --build-info-json
skald setup             # interactive probe, models, benchmarks, config
skald doctor
```

Release archives include `skald`, `skaldd`, `skald-overlay`, and `skald-tray`.
They do not include model weights, API keys, user configuration, or proprietary
CUDA libraries. See [Linux releases](docs/src/content/docs/linux/releases.md)
for signing, upgrade, rollback, CUDA target, and validation details.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo run -p skaldd -- --foreground
cargo run -p skald-cli -- status
```

Useful validation commands:

```bash
just test-clipboard
just test-paste
just doctor
just bench-e2e ./sample.wav
```

## Service and shortcuts

Install the user-session daemon:

```bash
skald service install
systemctl --user start skaldd
```

`skald service install` writes `~/.config/systemd/user/skaldd.service`, enables
it, and prints shortcut binding examples for your desktop session.

Bind an external shortcut to `skald toggle` (or use `skald start` /
`skald stop` / `skald ptt-start` / `skald ptt-stop` for push-to-talk where
your compositor supports key-release bindings).

On Hyprland and Sway, import the graphical session environment into systemd
before starting the service. `skald service install` and `skald doctor`
print the recommended `systemctl --user import-environment` lines.

## Paste Safety

Safe paste is supported on X11 with `xdotool`, on Omarchy/Hyprland through the
compositor's universal `Shift+Insert` shortcut, and on Sway with `wtype`.
Skald captures the active target when recording starts, when it stops, and
immediately before paste. A changed, unknown, or stale target falls back to
clipboard-only output.

GNOME Wayland, KDE Wayland, unknown Wayland sessions, and terminals outside
Omarchy/Hyprland default to clipboard-only behavior. Application profiles will
add more target-specific paste commands later.

The product and user documentation is at [tryskald.dev](https://tryskald.dev).

## Cleanup (opt-in)

Cleanup is disabled by default. To enable OpenRouter cleanup:

```bash
skald secrets set openrouter
skald cleanup enable openrouter
skald cleanup preview "hey john thanks for catching that"
skald toggle --cleanup
```

The default cleanup model is `~openai/gpt-mini-latest` on OpenRouter. Override
`cleanup.model` in `~/.config/skald/config.toml` if needed.

Cleanup sends transcript text to your configured provider, adds latency, and may
cost money per request. Use `--no-cleanup` for sensitive content or when you want
the raw transcript.

## Privacy

Skald does not store transcripts or audio by default. Cloud cleanup is opt-in
and sends transcript text off-device only when explicitly enabled.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
