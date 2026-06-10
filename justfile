set dotenv-load := true

default:
    @just --list

# Build all workspace crates.
build:
    cargo build --workspace

# Run the daemon in the foreground.
daemon: build
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

# Cancel a manual recording without retaining a WAV.
cancel: build
    target/debug/voxline cancel

# Show daemon status.
status: build
    target/debug/voxline status

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

# Run formatting, linting, and tests.
check:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
