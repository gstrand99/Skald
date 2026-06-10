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

## Privacy

VoxLine does not store transcripts or audio by default. Cloud cleanup is not
implemented in this milestone and will remain opt-in when added.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
