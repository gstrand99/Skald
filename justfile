set dotenv-load := true

default:
    @just --list

# Build all workspace crates.
build:
    cargo build --workspace

# Run the daemon in the foreground.
daemon: build-cuda
    target/debug/voxlined --foreground

# Record from the default microphone for the given number of seconds.
mic seconds="5": build
    target/debug/voxline test mic --seconds {{seconds}}

# Start a manual recording.
start: build
    target/debug/voxline start

# Stop a manual recording and print the WAV path and metrics.
stop: build
    target/debug/voxline stop

# Toggle recording; the stop toggle transcribes and copies the result.
toggle: build-cuda
    target/debug/voxline toggle

# Cancel a manual recording without retaining a WAV.
cancel: build
    target/debug/voxline cancel

# Show daemon status.
status: build
    target/debug/voxline status

# Stream daemon events with live preview text when preview is enabled.
watch: build
    target/debug/voxline watch

# Verify clipboard write/read/restore through the daemon.
test-clipboard: build
    target/debug/voxline test clipboard

# Paste a visible test string into the currently focused safe target.
test-paste: build
    target/debug/voxline test paste

# Report session, clipboard, target detection, and paste capabilities.
doctor: build
    target/debug/voxline doctor

# Install the systemd user service and print shortcut guidance.
service-install: build-cuda
    target/debug/voxline service install

# Show systemd user service status.
service-status: build-cuda
    target/debug/voxline service status

# Start the systemd user service.
service-start: build-cuda
    target/debug/voxline service start

# Stop the systemd user service.
service-stop: build-cuda
    target/debug/voxline service stop

# Preview OpenRouter cleanup for sample text.
cleanup-preview text style="": build
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "{{style}}" ]]; then
        target/debug/voxline cleanup preview --style "{{style}}" "{{text}}"
    else
        target/debug/voxline cleanup preview "{{text}}"
    fi

# List configured cleanup styles.
styles-list: build
    target/debug/voxline styles list

# Show active target and matched application profile.
apps-detect: build
    target/debug/voxline apps detect

# List configured insert snippets.
snippets-list: build
    target/debug/voxline snippets list

# Preview template snippet rendering for sample dictated text.
snippets-preview name text: build
    target/debug/voxline snippets preview {{name}} "{{text}}"

# Test voice command parsing for sample transcript text.
commands-test text: build
    target/debug/voxline commands test "{{text}}"

# Test OpenRouter connectivity through the daemon.
test-openrouter: build
    target/debug/voxline test openrouter

# Validate a generated WAV file.
inspect wav:
    file {{wav}}

# Play a generated WAV file through PipeWire.
play wav:
    pw-play {{wav}}

# Transcribe a 16 kHz mono WAV through the running daemon.
transcribe wav: build
    target/debug/voxline transcribe {{wav}} --no-cleanup

# Load the configured ASR model.
asr-load: build
    target/debug/voxline asr load

# Show ASR model state.
asr-status: build
    target/debug/voxline asr status

# Benchmark transcription for a WAV file.
bench-asr wav: build
    target/debug/voxline bench asr {{wav}}

# Benchmark loading the configured ASR model.
bench-model-load: build
    target/debug/voxline bench model-load

# Build the daemon with CUDA-enabled whisper-rs.
build-cuda:
    cargo build -p voxlined --no-default-features --features asr-whisper-rs-cuda

# Create the default config tree and config.toml.
config-init:
    cargo run -p voxline-cli -- config init

# Run formatting, linting, and tests.
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
