# Skald

Skald is a Linux-first, local-first dictation daemon and CLI. It records speech,
transcribes with a local Whisper model, copies the result to the clipboard, and
pastes only when the active target can be verified as stable.

Linux **1.0** validates **Hyprland Wayland**. Other desktop sessions are
best-effort. See the [desktop matrix](https://tryskald.dev/linux/desktop-matrix/).

## Quick start

Download a release archive from
[GitHub Releases](https://github.com/gstrand99/Skald/releases) or build from
source. Dependencies, checksum verification, and CUDA targets are in the
[install guide](https://tryskald.dev/install/).

```bash
tar -xzf skald-*-cpu.tar.gz
install -m 0755 skald-*/bin/* ~/.local/bin/
skald setup --if-missing
skald doctor
skald service install
systemctl --user start skaldd
```

Bind a compositor shortcut to `skald toggle`. On Hyprland and Sway, import the
graphical session into systemd before starting the service. `skald doctor` prints
the recommended `systemctl --user import-environment` lines.

Use the CPU archive with `cpu-safe` on CPU-only hosts. Use the CUDA archive with
`power-user-nvidia` when `skaldd` was built with CUDA support. See the
[setup wizard](https://tryskald.dev/setup/) for model selection and benchmarks.

## Documentation

User documentation lives at [tryskald.dev](https://tryskald.dev).

| Topic | Link |
|-------|------|
| Install | [tryskald.dev/install](https://tryskald.dev/install/) |
| Setup wizard | [tryskald.dev/setup](https://tryskald.dev/setup/) |
| Configuration | [tryskald.dev/configuration](https://tryskald.dev/configuration/) |
| CLI reference | [tryskald.dev/cli](https://tryskald.dev/cli/) |
| Service and shortcuts | [tryskald.dev/service](https://tryskald.dev/service/) |
| Benchmarks | [tryskald.dev/linux/benchmarks](https://tryskald.dev/linux/benchmarks/) |
| Releases | [tryskald.dev/linux/releases](https://tryskald.dev/linux/releases/) |
| Troubleshooting | [tryskald.dev/troubleshooting](https://tryskald.dev/troubleshooting/) |

Commented reference config:
[`config-example/linux/config.toml`](config-example/linux/config.toml).

## Config profiles

```bash
skald config profile cpu-safe            # small.en, CPU ASR, on_demand lifecycle
skald config profile power-user-nvidia   # large CUDA model, keep_warm lifecycle
```

Restart `skaldd` after profile or config changes.

## Privacy and cleanup

Skald does not store transcripts or audio by default. OpenRouter cleanup is
opt-in and sends transcript text only when enabled — never audio. See
[privacy](https://tryskald.dev/configuration/privacy/) and
[cleanup](https://tryskald.dev/configuration/cleanup/).

## Development

```bash
just check
cargo build --workspace
cargo test --workspace
```

Maintainer release flow:

```bash
just release-archives
just release-smoke
just release-checklist
```

Planned work is tracked in [ROADMAP.md](ROADMAP.md).

## Migrating from VoxLine

```bash
systemctl --user disable --now voxlined.service
mv ~/.config/voxline ~/.config/skald
mv ~/.local/share/voxline ~/.local/share/skald
sed -i 's|/voxline|/skald|g; s|voxline/|skald/|g' ~/.config/skald/config.toml
skald config validate
skald service install
systemctl --user start skaldd.service
```

Run `skald secrets set openrouter` again if cleanup used the VoxLine keyring.
Remove old VoxLine binaries and `voxlined.service` after validating Skald.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
