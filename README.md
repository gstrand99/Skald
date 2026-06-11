# VoxLine

VoxLine is a Linux-first, local-first dictation daemon and CLI. It records,
transcribes locally, copies the final text to the clipboard, and pastes only
when the active target can be verified as stable.

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
