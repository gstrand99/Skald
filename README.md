# VoxLine

VoxLine is a Linux-first, local-first dictation daemon and CLI. It records,
transcribes locally, copies the final text to the clipboard, and pastes only
when the active target can be verified as stable.

## Configuration

VoxLine reads a single TOML file at `~/.config/voxline/config.toml`. If the file
is missing, built-in defaults apply (same values as the Linux example below).

A commented reference copy lives in
[`config-example/linux/config.toml`](config-example/linux/config.toml). Platform
stubs for future ports are under [`config-example/`](config-example/).

### Setup

```bash
voxline config init          # write config.toml and scaffold directories
voxline config validate      # check the file against v1 rules
voxline config path          # print the active config path
voxline doctor               # session, paths, daemon, and privacy checks
```

`config init` creates:

```text
~/.config/voxline/
  config.toml
  styles/      # cleanup styles (M8a)
  apps/        # per-application profiles (M8b)
  snippets/    # insert snippets (M8c)
~/.local/share/voxline/models/
$XDG_RUNTIME_DIR/voxline/    # WAV files, Unix socket (when runtime_dir = "auto")
```

Runtime files and the daemon socket use `paths.runtime_dir`. The default `auto`
resolves to `$XDG_RUNTIME_DIR/voxline`.

### Preset profiles

Profiles adjust ASR and cleanup settings without replacing secrets:

```bash
voxline config profile power-user-nvidia   # large CUDA model, keep_warm lifecycle
voxline config profile cpu-safe            # small.en CPU model, on_demand lifecycle
```

Restart `voxlined` after changing config if the daemon is already running.

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
`[[vocabulary.replacements]]`). Manage entries with `voxline vocab`.

**`[cleanup]`** — Opt-in cloud cleanup. Disabled by default (`provider = "none"`).
When enabled with OpenRouter, only transcript text is sent off-device—not audio.
Enable via `voxline cleanup enable openrouter` or edit config directly. Default
model when enabled: `~openai/gpt-mini-latest`. The cleanup system prompt comes
from a style under `styles/` (default: `cleanup.default_style = "default"` loads
`styles/default.toml` → `styles/default.md`). `config init` installs the bundled
default style files; edit the `.md` file to change cleanup behavior.

**`[secrets]`** — Where to look for API keys. Keys are never stored in
`config.toml`. Use `voxline secrets set openrouter` (keyring), or set
`OPENROUTER_API_KEY`, or opt into the insecure file fallback explicitly.

**`[injection]`** — Clipboard-first output. `auto_paste = "safe"` pastes only
when the active target is stable; otherwise text stays on the clipboard.
`[injection.linux]` holds session-specific paste commands for future routing;
GNOME Wayland defaults to clipboard-only.

**`[notifications]`** — Desktop notifications for errors and clipboard-only fallback.

**`[privacy]`** — Off by default. Enabling storage or transcript logging is explicit.

### Secrets and cleanup

```bash
voxline secrets set openrouter
voxline cleanup enable openrouter
voxline toggle --cleanup        # per-dictation cleanup
voxline toggle --no-cleanup     # skip cleanup for one job
```

See [Cleanup (opt-in)](#cleanup-opt-in) below.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo run -p voxlined -- --foreground
cargo run -p voxline-cli -- status
```

Useful validation commands:

```bash
just test-clipboard
just test-paste
just doctor
```

## Service and shortcuts

Install the user-session daemon:

```bash
voxline service install
systemctl --user start voxlined
```

`voxline service install` writes `~/.config/systemd/user/voxlined.service`, enables
it, and prints shortcut binding examples for your desktop session.

Bind an external shortcut to `voxline toggle` (or use `voxline start` /
`voxline stop` / `voxline ptt-start` / `voxline ptt-stop` for push-to-talk where
your compositor supports key-release bindings).

On Hyprland and Sway, import the graphical session environment into systemd
before starting the service. `voxline service install` and `voxline doctor`
print the recommended `systemctl --user import-environment` lines.

## Paste Safety

Safe paste is supported on X11 with `xdotool`, on Omarchy/Hyprland through the
compositor's universal `Shift+Insert` shortcut, and on Sway with `wtype`.
VoxLine captures the active target when recording starts, when it stops, and
immediately before paste. A changed, unknown, or stale target falls back to
clipboard-only output.

GNOME Wayland, KDE Wayland, unknown Wayland sessions, and terminals outside
Omarchy/Hyprland default to clipboard-only behavior. Application profiles will
add more target-specific paste commands later.

The product and architecture specification is in
[`VoxLine_implementation_plan.md`](VoxLine_implementation_plan.md).

## Cleanup (opt-in)

Cleanup is disabled by default. To enable OpenRouter cleanup:

```bash
voxline secrets set openrouter
voxline cleanup enable openrouter
voxline cleanup preview "hey john thanks for catching that"
voxline toggle --cleanup
```

The default cleanup model is `~openai/gpt-mini-latest` on OpenRouter. Override
`cleanup.model` in `~/.config/voxline/config.toml` if needed.

Cleanup sends transcript text to your configured provider, adds latency, and may
cost money per request. Use `--no-cleanup` for sensitive content or when you want
the raw transcript.

## Privacy

VoxLine does not store transcripts or audio by default. Cloud cleanup is opt-in
and sends transcript text off-device only when explicitly enabled.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
