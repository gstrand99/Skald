---
title: Install
description: Build and install Skald on Linux.
---

Skald ships as Linux archives and Rust source builds. There is no distro package
yet.

## Dependencies

System packages (Arch-oriented names):

- PipeWire or PulseAudio for microphone capture
- `wl-clipboard` or `xclip` for clipboard integration
- Session tools as needed: `hyprctl`, `xdotool`, `wtype`, `notify-send`
- GTK 4 development libraries to build `skald-overlay`
- Optional: CUDA toolkit for the GPU ASR build profile
- `sha256sum` and `gpg` for archive verification

## Install from an archive

Download the archive, checksum manifest, and detached signatures from the GitHub
release. Verify before extracting:

```bash
sha256sum --check skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu.tar.gz.sha256
gpg --verify skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu.tar.gz.asc
gpg --verify skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu.tar.gz.sha256.asc
```

Extract and install user-local binaries:

```bash
tar -xzf skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu.tar.gz
install -m 0755 skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu/bin/* ~/.local/bin/
skald version --json
skaldd --build-info-json
skald setup --if-missing
skald doctor
```

Use the `cuda` archive only on hosts matching the documented CUDA driver target.
The archive records the tested driver/toolkit in `BUILD-METADATA.toml`.

## Build from source

CPU-only workspace:

```bash
just release
```

Power-user CUDA daemon (RTX-class GPU):

```bash
just release-cuda
```

Binaries install to `target/release/` (`skald`, `skaldd`, `skald-overlay`,
`skald-tray`).

User-local install (runs the setup wizard when no config exists):

```bash
just install              # after just release (CPU)
just install-cuda         # after just release-cuda (CUDA skaldd)
```

Skip the wizard (CI or manual setup):

```bash
SKALD_SKIP_SETUP=1 just install
SKALD_SKIP_SETUP=1 just install-cuda
```

## Migrate from VoxLine

Skald deliberately uses separate binary, service, XDG path, socket, and keyring
names. Stop VoxLine before moving an existing installation:

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

After validation, remove the old `voxlined.service` file and VoxLine binaries.
Keyring entries do not migrate between application names; run
`skald secrets set openrouter` again when needed.

## First-time setup

Recommended: run the [Setup wizard](/setup/) (`skald setup`).

Manual path:

```bash
skald config init
skald config profile power-user-nvidia   # or cpu-safe
```

Download Whisper GGML models into `~/.local/share/skald/models/`:

- Power-user: a large quantized model (for example `ggml-large-v3-turbo-q5_0.bin`)
- CPU-safe: `ggml-small.en.bin`
- Preview (optional): `ggml-small.en.bin` when `preview.enabled = true`

Validate:

```bash
skald config validate
skald doctor
```

## Profiles

| Profile | ASR model | Lifecycle | Use case |
|---------|-----------|-----------|----------|
| `power-user-nvidia` | Large CUDA model | `keep_warm` | Primary workstation target |
| `cpu-safe` | `small.en` on CPU | `on_demand` | Laptops and CPU-only hosts |

Restart `skaldd` after changing profiles.

## Upgrade

Archives replace binaries only. They do not overwrite user configuration,
styles, snippets, app profiles, managed models, keyring entries, or the systemd
user service state.

```bash
systemctl --user stop skaldd.service || true
install -m 0755 skald-0.1.0-linux-x86_64-unknown-linux-gnu-cpu/bin/* ~/.local/bin/
skald config upgrade
skald service install
systemctl --user restart skaldd.service
skald doctor
```

Config migrations are one-way unless release notes explicitly say otherwise.
Back up `~/.config/skald` before upgrading across config schema versions.

## Rollback

Keep the previous archive and checksum manifest. To roll back:

```bash
systemctl --user stop skaldd.service || true
install -m 0755 skald-previous-linux-x86_64-unknown-linux-gnu-cpu/bin/* ~/.local/bin/
skald service install
systemctl --user restart skaldd.service
skald doctor
```

If the newer version performed an irreversible config migration, restore the
backed-up `~/.config/skald` before restarting the daemon.

## Benchmarks

```bash
skald bench model-load
skald bench end-to-end ./sample.wav
skald bench dictation ./sample.wav --no-cleanup
skald bench dictation ./sample.wav --cleanup
skald bench dictation ./sample.wav --paste
```

See [Benchmark results](/linux/benchmarks/) for recorded numbers on the reference
workstation.
