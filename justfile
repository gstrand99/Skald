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

# Measure ambient microphone noise and recommend gate settings.
calibrate-mic seconds="5" *flags="": build
    target/debug/skald calibrate mic --seconds {{seconds}} {{flags}}

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

# Preview overlay styles without starting a dictation job.
overlay-preview *flags="": build
    target/debug/skald overlay preview {{flags}}

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

# Import personal vocabulary from a plain text or CSV file.
vocab-import file *flags="": build
    target/debug/skald vocab import {{flags}} {{file}}

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

# List managed ASR model candidates and installed state.
models-list: build
    target/debug/skald models list

# Verify installed catalog models without loading them.
models-verify: build
    target/debug/skald models verify

# Install a catalog model by stable ID.
models-install model: build
    target/debug/skald models install {{model}}

# Select a catalog model for final ASR.
models-select model: build
    target/debug/skald models select {{model}}

# Select a catalog model for text preview.
models-select-preview model: build
    target/debug/skald models select-preview {{model}}

# Review unused managed models for removal.
models-prune: build
    target/debug/skald models prune

# Manual managed-model validation without downloading model weights.
models-check: build
    target/debug/skald models list
    target/debug/skald models verify || true

# Noninteractive CPU-safe onboarding.
setup-cpu: build
    target/debug/skald models install small.en --select

# NVIDIA onboarding; requires a CUDA-enabled daemon build.
setup-nvidia: build-cuda
    target/debug/skald models install large-v3-turbo-q5 --select
    target/debug/skald models install small.en-q5 --select-preview

# Validate the CPU-safe managed-model path after setup.
validate-models-cpu: build
    target/debug/skald models verify small.en
    target/debug/skald doctor

# Validate NVIDIA model files and daemon build state after setup.
validate-models-nvidia: build-cuda
    target/debug/skald models verify large-v3-turbo-q5
    target/debug/skald models verify small.en-q5
    target/debug/skald doctor

# Generate shell completion output.
completions shell:
    cargo run -p skald-cli -- completions {{shell}}

# Benchmark transcription for a WAV file.
bench-asr wav: build
    target/debug/skald bench asr {{wav}}

# Run a redacted diagnostics benchmark for a WAV file.
diagnostics-benchmark wav: build
    target/debug/skald diagnostics benchmark --json {{wav}}

# Benchmark loading the configured ASR model.
bench-model-load: build
    target/debug/skald bench model-load

# Build the daemon with CUDA-enabled whisper-rs.
build-cuda:
    cargo build -p skaldd --no-default-features --features asr-whisper-rs-cuda

# Optimized release builds for local installation.
release:
    cargo build --workspace --release --locked

# CUDA release build for the power-user profile (skaldd + CLI + overlay + tray).
release-cuda:
    cargo build -p skaldd --release --locked --no-default-features --features asr-whisper-rs-cuda
    cargo build -p skald-cli -p skald-overlay -p skald-tray --release --locked

# Build CPU-safe and CUDA release archives in dist/.
release-archives:
    scripts/release-package cpu
    scripts/release-package cuda

# Build a CPU-safe release archive without requiring a tag or clean worktree.
release-archive-dry-run:
    SKALD_RELEASE_DRY_RUN=1 scripts/release-package cpu

# Smoke-test release archives from dist/ after extraction.
release-smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    for archive in dist/*-cpu.tar.gz; do
        scripts/release-smoke "${archive}" cpu
    done
    for archive in dist/*-cuda.tar.gz; do
        scripts/release-smoke "${archive}" cuda
    done

# Verify SHA-256 manifests for all release archives.
release-checksums:
    #!/usr/bin/env bash
    set -euo pipefail
    for manifest in dist/*.tar.gz.sha256; do
        sha256sum --check "${manifest}"
    done

# GPG-sign release archives and checksum manifests.
release-sign:
    #!/usr/bin/env bash
    set -euo pipefail
    for artifact in dist/*.tar.gz dist/*.tar.gz.sha256; do
        gpg --batch --yes --armor --detach-sign ${SKALD_SIGNING_KEY:+--local-user "$SKALD_SIGNING_KEY"} "${artifact}"
    done

# Generate draft release notes.
release-notes previous="":
    scripts/release-notes "v$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "skald-core") | .version')" "{{previous}}"

# Print the manual Linux release validation checklist.
release-checklist:
    scripts/release-checklist

# Verify the dev integration branch before opening a dev-to-main release PR.
release-ready: check
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git branch --show-current)"
    if [[ "${branch}" != "dev" ]]; then
        echo "release-ready must run on dev, not ${branch}" >&2
        exit 1
    fi

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

# Run first-time setup without prompts and install or refresh the user service.
setup-noninteractive:
    cargo run -p skald-cli -- setup --non-interactive --install-service

# Print the machine-readable first-time setup report.
setup-json:
    cargo run -p skald-cli -- setup --json

# Create the default config tree and config.toml.
config-init:
    cargo run -p skald-cli -- config init

# Run the Starlight docs dev server (https://tryskald.dev).
docs-dev:
    cd docs && bun run dev

# Build the static docs site to docs/dist/.
docs-build:
    cd docs && bun run build

# Validate docs typecheck and build.
docs-check:
    cd docs && bun run check
    cd docs && bun run build

# Build and deploy docs to Cloudflare Workers (tryskald.dev).
docs-deploy:
    cd docs && bun run deploy

# Run formatting, linting, and tests.
check: docs-check
    cargo fmt --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
