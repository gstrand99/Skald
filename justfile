set dotenv-load := true

default:
    @just --list

# Build all workspace crates.
build:
    cargo build --workspace

# Run the daemon in the foreground.
daemon: build-cuda
    target/debug/skaldd --foreground

# Record from the default microphone for the given number of seconds.
mic seconds="5": build
    target/debug/skald test mic --seconds {{seconds}}

# Start a manual recording.
start: build
    target/debug/skald start

# Stop a manual recording and print the WAV path and metrics.
stop: build
    target/debug/skald stop

# Toggle recording; the stop toggle transcribes and copies the result.
toggle: build-cuda
    target/debug/skald toggle

# Cancel a manual recording without retaining a WAV.
cancel: build
    target/debug/skald cancel

# Show daemon status.
status: build
    target/debug/skald status

# Stream daemon events with live preview text when preview is enabled.
watch: build
    target/debug/skald watch

# Stream privacy-safe JSON updates for a Waybar custom module.
waybar: build
    target/debug/skald waybar

# Launch the preview overlay (requires preview.enabled and a graphical session).
overlay: build
    target/debug/skald-overlay

# Launch the optional StatusNotifier/AppIndicator tray client.
tray: build
    target/debug/skald-tray

# Verify clipboard write/read/restore through the daemon.
test-clipboard: build
    target/debug/skald test clipboard

# Paste a visible test string into the currently focused safe target.
test-paste: build
    target/debug/skald test paste

# Report session, clipboard, target detection, and paste capabilities.
doctor: build
    target/debug/skald doctor

# Install the systemd user service and print shortcut guidance.
service-install: build-cuda
    target/debug/skald service install

# Show systemd user service status.
service-status: build-cuda
    target/debug/skald service status

# Start the systemd user service.
service-start: build-cuda
    target/debug/skald service start

# Stop the systemd user service.
service-stop: build-cuda
    target/debug/skald service stop

# Preview OpenRouter cleanup for sample text.
cleanup-preview text style="": build
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "{{style}}" ]]; then
        target/debug/skald cleanup preview --style "{{style}}" "{{text}}"
    else
        target/debug/skald cleanup preview "{{text}}"
    fi

# List configured cleanup styles.
styles-list: build
    target/debug/skald styles list

# Show active target and matched application profile.
apps-detect: build
    target/debug/skald apps detect

# List configured insert snippets.
snippets-list: build
    target/debug/skald snippets list

# Preview template snippet rendering for sample dictated text.
snippets-preview name text: build
    target/debug/skald snippets preview {{name}} "{{text}}"

# Test voice command parsing for sample transcript text.
commands-test text: build
    target/debug/skald commands test "{{text}}"

# Test OpenRouter connectivity through the daemon.
test-openrouter: build
    target/debug/skald test openrouter

# Validate a generated WAV file.
inspect wav:
    file {{wav}}

# Play a generated WAV file through PipeWire.
play wav:
    pw-play {{wav}}

# Transcribe a 16 kHz mono WAV through the running daemon.
transcribe wav: build
    target/debug/skald transcribe {{wav}}

# Load the configured ASR model.
asr-load: build
    target/debug/skald asr load

# Show ASR model state.
asr-status: build
    target/debug/skald asr status

# Benchmark transcription for a WAV file.
bench-asr wav: build
    target/debug/skald bench asr {{wav}}

# Benchmark loading the configured ASR model.
bench-model-load: build
    target/debug/skald bench model-load

# Build the daemon with CUDA-enabled whisper-rs.
build-cuda:
    cargo build -p skaldd --no-default-features --features asr-whisper-rs-cuda

# Optimized release builds for local installation.
release:
    cargo build --workspace --release

# CUDA release build for the power-user profile (skaldd + CLI + overlay).
release-cuda:
    cargo build -p skaldd --release --no-default-features --features asr-whisper-rs-cuda
    cargo build -p skald-cli -p skald-overlay -p skald-tray --release

# Print transcribe-path benchmark timings for a WAV file.
bench-e2e wav: build
    target/debug/skald bench end-to-end {{wav}}

# Full dictation-path benchmark (ASR + optional cleanup + clipboard).
bench-dictation wav *flags="": build
    target/debug/skald bench dictation {{wav}} {{flags}}

# Install release binaries to ~/.local/bin (user-local).
install: release
    @just _install-release-binaries

# Install CUDA release binaries to ~/.local/bin (user-local).
install-cuda: release-cuda
    @just _install-release-binaries

_install-release-binaries:
    #!/usr/bin/env bash
    set -euo pipefail
    dest="${HOME}/.local/bin"
    mkdir -p "${dest}"
    install -m 0755 target/release/skald target/release/skaldd target/release/skald-overlay target/release/skald-tray "${dest}/"
    echo "Installed to ${dest}"
    if [[ "${SKALD_SKIP_SETUP:-}" != "1" ]]; then
        target/release/skald setup --if-missing || true
    fi

# Rebuild and reinstall local binaries, upgrade config, and restart the daemon.
dev-reload: release-cuda
    #!/usr/bin/env bash
    set -euo pipefail
    SKALD_SKIP_SETUP=1 just _install-release-binaries
    target/release/skald config upgrade
    target/release/skald service install
    target/release/skald service restart

# Run the interactive first-time setup wizard.
setup:
    cargo run -p skald-cli -- setup

# Create the default config tree and config.toml.
config-init:
    cargo run -p skald-cli -- config init

# Run the Starlight docs dev server (https://tryskald.dev).
docs-dev:
    cd docs && bun run dev

# Build the static docs site to docs/dist/.
docs-build:
    cd docs && bun run build

# Build and deploy docs to Cloudflare Workers (tryskald.dev).
docs-deploy:
    cd docs && bun run deploy

# Run formatting, linting, and tests.
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
