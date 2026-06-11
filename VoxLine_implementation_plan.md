# VoxLine Implementation Plan

## 0. Project Summary

VoxLine is a Linux-first, local-first dictation app inspired by the interaction model of Wispr Flow: trigger dictation, speak, stop, and safely insert the final text into the active application.

The first target user is a power user on Linux with an NVIDIA GPU-class workstation, specifically the initial development target of:

```text
CPU: Ryzen 5900X-class
GPU: RTX 3070 Ti-class, 8 GB VRAM
RAM: 32 GB
OS: Linux desktop session
```

The architecture must still be portable enough to support CPU-only Linux profiles later, followed by macOS and Windows ports.

Core product promise:

```text
fast local speech-to-text
safe clipboard-first insertion
optional opt-in cleanup
no transcript or audio storage by default
headless daemon with CLI control
```

Important privacy rule:

```text
Audio never leaves the machine.
Transcript text leaves the machine only if the user explicitly enables a cloud cleanup provider.
```

Cloud cleanup is important to the product, but it is opt-in. The app must not claim “fully local” when cleanup is enabled.

---

## 1. Product Definition

### 1.1 Primary v1 product flow

```text
User triggers dictation
  ↓
voxlined starts recording microphone audio
  ↓
User stops dictation
  ↓
voxlined finalizes a temporary WAV in runtime storage
  ↓
local Whisper backend transcribes the full recording
  ↓
final transcript is copied to clipboard
  ↓
if safe, VoxLine attempts paste injection
  ↓
previous clipboard is restored when configured and possible
  ↓
temporary audio is deleted
```

### 1.2 Cleanup-enabled flow

Cleanup is the first major feature after reliable local dictation and insertion.

```text
User triggers dictation
  ↓
record locally
  ↓
transcribe locally
  ↓
if cleanup is enabled and permitted for this job:
    send transcript text to configured cleanup provider
    receive cleaned output
    validate cleaned output
    fallback to raw transcript on failure
  ↓
copy final text to clipboard
  ↓
safely paste if supported
```

### 1.3 Streaming preview later

Realtime preview is useful, but it is not part of the first production path.

Preview must never paste draft text.

Correct:

```text
show provisional preview in watch/overlay client
paste final full transcription once at the end
```

Incorrect:

```text
stream draft text into the target app while the user is still speaking
```

---

## 2. V1 Scope

### 2.1 V1 must include

```text
headless user-session daemon
CLI
IPC request/response
IPC event stream
local microphone capture
local final transcription using whisper-rs
power-user keep-warm ASR profile
clipboard copy
conservative paste injection
active-window safety checks where available
clipboard save/restore where possible
systemd user service
trigger/keybinding documentation for toggle and push-to-talk
custom vocabulary
final-path silence handling
dictate doctor equivalent, named voxline doctor
privacy-safe defaults
```

### 2.2 V1.1 should follow quickly

```text
OpenRouter cleanup
BYO API key
cleanup disabled until explicitly enabled
default cleanup style
cleanup timeout and fallback
short-utterance cleanup skip
cost and latency warnings
```

### 2.3 V1.2 should add routing polish

```text
styles
app profiles
insert snippets
voice commands as experimental
```

### 2.4 Post-v1 features

```text
template snippets with structured LLM extraction
realtime preview
GUI overlay
native hotkey daemon integration
local LLM cleanup
model downloader UI
packaged installers
macOS port
Windows port
```

---

## 3. Naming and Paths

Product name:

```text
VoxLine
```

Primary binaries:

```text
voxlined    # headless daemon
voxline     # CLI
```

Optional later binaries:

```text
voxline-overlay
voxline-tray
voxline-mac-helper
```

Linux paths:

```text
Config:      ~/.config/voxline/config.toml
Styles:      ~/.config/voxline/styles/
Apps:        ~/.config/voxline/apps/
Snippets:    ~/.config/voxline/snippets/
Models:      ~/.local/share/voxline/models/
Runtime:     $XDG_RUNTIME_DIR/voxline/
Socket:      $XDG_RUNTIME_DIR/voxline/voxlined.sock
Service:     ~/.config/systemd/user/voxlined.service
```

Runtime directory requirements:

```text
must be per-user
must be mode 0700
should be tmpfs when XDG_RUNTIME_DIR is available
must be cleaned on logout by the OS where possible
```

If `$XDG_RUNTIME_DIR` is unavailable, fail by default unless the user explicitly configures an alternate runtime directory.

---

## 4. Architecture Principles

### 4.1 Headless daemon

`voxlined` owns:

```text
audio recording
ASR model lifecycle
job orchestration
cleanup calls
clipboard writes
paste attempts
notifications
IPC server
```

`voxlined` must not own:

```text
overlay windows
GUI settings
tray UI
desktop toolkit dependencies
```

Any preview overlay or tray should be a separate client subscribing to daemon events.

### 4.2 Clipboard-first insertion

All text insertion starts with clipboard copy.

```text
final text
  ↓
write clipboard
  ↓
optionally verify or wait for clipboard availability
  ↓
attempt paste only if configured and safe
  ↓
restore previous clipboard when possible
```

If paste fails:

```text
leave final text on clipboard
notify user
emit result event with paste_succeeded = false
```

### 4.3 Local-first with honest cloud boundaries

Default:

```toml
[cleanup]
enabled = false
provider = "none"
```

When cleanup is enabled, docs and UI must say:

```text
Audio remains local.
Transcript text is sent to the configured cleanup provider.
```

### 4.4 Power-user first, portable later

The first performance target is the developer’s Linux GPU workstation. Do not compromise the internal design for CPU-only defaults too early.

However, every feature must have a portability story:

```text
Linux power-user NVIDIA profile first
Linux CPU profile second
macOS adapter third
Windows adapter fourth
```

### 4.5 Small workspace first

Start with a small number of crates. Do not split every subsystem into a crate before code exists.

Initial workspace:

```text
voxline/
  Cargo.toml
  crates/
    voxline-core/
    voxlined/
    voxline-cli/
    voxline-platform/
```

Suggested responsibilities:

```text
voxline-core:
  config
  protocol types
  job/state types
  transcript types
  typed errors
  routing config types
  vocabulary types

voxlined:
  daemon binary
  job orchestration
  IPC server
  ASR manager
  audio manager
  cleanup manager
  injection manager

voxline-cli:
  CLI binary
  command parsing
  IPC client
  doctor output
  config/secrets helpers

voxline-platform:
  Linux platform adapters now
  macOS/Windows adapters later
  clipboard
  paste
  active app detection
  notification
  service install helpers
```

Use internal modules for:

```text
audio
asr
cleanup
inject
doctor
service
ipc
vocabulary
routing
```

Split into more crates only after boundaries stabilize.

---

## 5. Recommended Rust Dependencies

Use exact versions chosen by the implementing agent at build time, but start from these libraries.

| Concern | First implementation |
|---|---|
| CLI | `clap` |
| Config | `serde`, `toml` |
| Errors | `thiserror` for typed errors, `anyhow` only at binary edges |
| Logging | `tracing`, `tracing-subscriber` |
| Async runtime | `tokio` |
| HTTP | `reqwest` with `rustls-tls` |
| Paths | `directories` or equivalent |
| Secrets | `keyring` |
| Clipboard | `arboard`, plus Linux fallbacks where needed |
| Notifications | `notify-rust` or platform adapter |
| Audio capture | `cpal` |
| WAV writing | `hound` |
| Resampling | `rubato` or equivalent |
| ASR | `whisper-rs` |
| IPC | Tokio Unix domain sockets on Linux/macOS; named pipes later on Windows |
| JSON | `serde_json` |
| Runtime/temp files | `tempfile` plus explicit runtime dir |
| Process handling | `tokio::process` only for platform tools |

Important ASR packaging note:

```text
whisper-rs GPU support is feature-gated at build time.
Provide at least a CUDA build profile for the initial power-user target and a CPU build profile later.
```

Example feature strategy:

```toml
[features]
default = ["asr-whisper-rs"]
asr-whisper-rs = []
asr-whisper-rs-cuda = ["whisper-rs/cuda"]
asr-whisper-rs-cpu = []
```

Do not hard-code this exact feature naming if a cleaner packaging strategy emerges.

---

## 6. Latency Targets

Latency is a product requirement, not a tuning detail.

### 6.1 Initial power-user target

For the target workstation with warm ASR model:

```text
10-second utterance, local-only path:
  p50 stop-to-clipboard: < 1.5s
  p95 stop-to-clipboard: < 3.0s

10-second utterance, safe paste path:
  p50 stop-to-insert: < 2.0s
  p95 stop-to-insert: < 3.5s
```

For cleanup-enabled path:

```text
local ASR budget still applies
cleanup adds network/model latency
cleanup must have timeout and fallback
short utterances may bypass cleanup
```

### 6.2 Per-stage budget

```text
stop recording + finalize WAV: < 100ms
pre-ASR silence/energy gate: < 50ms
ASR model ready check: < 50ms when warm
ASR transcription: benchmarked per model/hardware
cleanup route selection: < 20ms
cloud cleanup timeout: configurable, default 8000–12000ms for opt-in cleanup
clipboard write: < 100ms
paste safety check: < 100ms where supported
paste command: < 250ms
notification: non-blocking
```

### 6.3 Fast paths

```text
cleanup disabled by default
--no-cleanup per job
skip cleanup below N words when enabled
keep_warm ASR lifecycle for target machine
model loaded at daemon start for power-user profile
clipboard-only fallback on paste uncertainty
```

### 6.4 Required benchmarking commands

```bash
voxline bench asr ./samples/10s.wav
voxline bench end-to-end ./samples/10s.wav --no-cleanup
voxline bench end-to-end ./samples/10s.wav --cleanup
voxline bench model-load
```

Bench output should include:

```text
recording duration
audio finalize ms
ASR load ms
ASR transcribe ms
cleanup ms
clipboard ms
paste ms
total stop-to-clipboard ms
total stop-to-insert ms
```

---

## 7. Trigger UX

VoxLine v1 does not implement native global hotkey capture inside the daemon.

Instead, v1 provides reliable CLI commands that users bind through their desktop environment or compositor.

### 7.1 Required trigger commands

```bash
voxline toggle
voxline start
voxline stop
voxline cancel
```

Optional aliases:

```bash
voxline ptt-start
voxline ptt-stop
```

These can simply call `start` and `stop`, but the names make compositor configs easier to read.

### 7.2 Toggle mode

Toggle is required in v1.

```text
first trigger: start recording
second trigger: stop recording and continue pipeline
```

If the daemon is busy and not recording:

```text
return busy
emit current job state
never cancel a transcribing/cleaning/injecting job from toggle
```

### 7.3 Push-to-talk mode

Push-to-talk should be supported through separate start and stop commands.

```text
key press -> voxline start
key release -> voxline stop
```

Not every desktop shortcut system supports release bindings. Where release binding is unavailable, docs must recommend toggle mode.

### 7.4 Example bindings

GNOME custom shortcuts:

```text
Name: VoxLine Toggle
Command: voxline toggle
Shortcut: user-chosen key
```

KDE custom shortcut:

```text
Command/URL: voxline toggle
Trigger: user-chosen key
```

Hyprland toggle example:

```text
bind = $mainMod, SPACE, exec, voxline toggle
```

Hyprland push-to-talk example:

```text
bind = $mainMod, V, exec, voxline start
bindr = $mainMod, V, exec, voxline stop
```

Sway toggle example:

```text
bindsym $mod+space exec voxline toggle
```

Sway push-to-talk example:

```text
bindsym $mod+v exec voxline start
bindsym --release $mod+v exec voxline stop
```

X11 example:

```text
Bind a desktop shortcut to: voxline toggle
```

### 7.5 Doctor trigger guidance

`voxline doctor` must print platform-specific guidance:

```text
Trigger mode: external shortcut
Recommended command: voxline toggle
Push-to-talk support: available if your compositor supports release bindings
```

`voxline service install` must also print shortcut setup instructions.

---

## 8. State Model

Do not model the daemon as one large enum that mixes job state, model state, and preview state.

Use orthogonal state dimensions.

### 8.1 Job state

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobState {
    Idle,
    Recording,
    Stopping,
    Transcribing,
    Cleaning,
    Copying,
    Injecting,
    Done,
    Cancelled,
    Failed { code: String, message: String },
}
```

### 8.2 Model state

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelState {
    Unloaded,
    Loading,
    Ready,
    Failed { code: String, message: String },
}
```

### 8.3 Daemon status

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub protocol_version: u32,
    pub active_job_id: Option<JobId>,
    pub job_state: JobState,
    pub final_model_state: ModelState,
    pub preview_model_state: Option<ModelState>,
    pub cleanup_enabled: bool,
    pub auto_paste_effective: AutoPasteEffectiveMode,
}
```

A job can be recording while the final model is loading or ready. Preview can be active while recording. These are not mutually exclusive.

---

## 9. Job Model

V1 supports one active dictation job.

```text
max_concurrent_jobs = 1
```

Every job has a unique `job_id`.

```rust
pub struct DictationJob {
    pub job_id: JobId,
    pub created_at: SystemTime,
    pub started_at: Option<SystemTime>,
    pub stopped_at: Option<SystemTime>,
    pub completed_at: Option<SystemTime>,
    pub trigger_mode: TriggerMode,
    pub requested_style: Option<String>,
    pub requested_snippet: Option<String>,
    pub cleanup_override: Option<CleanupOverride>,
    pub target_at_start: Option<TargetContext>,
    pub target_at_stop: Option<TargetContext>,
}
```

Busy behavior:

```text
toggle while Idle -> start new job
toggle while Recording -> stop and process active job
toggle while Transcribing/Cleaning/Copying/Injecting -> return busy
start while not Idle -> return busy
stop while Recording -> stop and process
stop while not Recording -> return no_active_recording
cancel while Recording -> stop and delete audio
cancel while later pipeline stage -> mark cancel requested if stage supports it, otherwise return cannot_cancel
```

V1 may choose not to cancel ASR/cleanup once started, but behavior must be explicit.

---

## 10. IPC Protocol

### 10.1 Transport

Linux/macOS:

```text
Unix domain socket at $XDG_RUNTIME_DIR/voxline/voxlined.sock
```

Windows later:

```text
Named pipe: \\.\pipe\voxline-voxlined
```

Do not put slashes inside the Windows pipe name.

### 10.2 Socket security

V1 security model:

```text
socket directory mode 0700
socket owned by current user
same-UID access only where peer credential checking is available
no network socket
```

Risk accepted for v1:

```text
A malicious local same-user process may command the daemon to record, paste, or spend cleanup credits.
```

Mitigations:

```text
same-user socket only
optional cleanup disabled by default
no transcript history
doctor reports socket permissions
future: per-client approval or token auth if needed
```

### 10.3 Request format

Use newline-delimited JSON for v1.

Every request includes:

```json
{
  "protocol_version": 1,
  "cmd": "toggle",
  "request_id": "client-generated-id"
}
```

### 10.4 Response format

Every response includes:

```json
{
  "protocol_version": 1,
  "request_id": "client-generated-id",
  "ok": true,
  "job_id": "01J...",
  "state": "Recording"
}
```

Busy response:

```json
{
  "protocol_version": 1,
  "request_id": "client-generated-id",
  "ok": false,
  "error": {
    "code": "busy",
    "message": "VoxLine is currently transcribing"
  },
  "job_id": "01J...",
  "state": "Transcribing"
}
```

### 10.5 Commands

Required v1 commands:

```json
{"cmd":"status"}
{"cmd":"toggle"}
{"cmd":"start"}
{"cmd":"stop"}
{"cmd":"cancel"}
{"cmd":"subscribe","events":["state","result","error"]}
```

Cleanup-related commands after V1.1:

```json
{"cmd":"toggle","cleanup":"force"}
{"cmd":"toggle","cleanup":"disable"}
{"cmd":"toggle","style":"professional"}
{"cmd":"toggle","snippet":"signature"}
```

### 10.6 Event format

Every event includes:

```json
{
  "protocol_version": 1,
  "event": "state",
  "job_id": "01J...",
  "timestamp_ms": 1730000000000
}
```

State event:

```json
{
  "protocol_version": 1,
  "event": "state",
  "job_id": "01J...",
  "job_state": "Recording",
  "final_model_state": "Ready"
}
```

Result event:

```json
{
  "protocol_version": 1,
  "event": "result",
  "job_id": "01J...",
  "copied_to_clipboard": true,
  "paste_attempted": true,
  "paste_succeeded": true,
  "cleanup_used": false,
  "clipboard_restored": true
}
```

Error event:

```json
{
  "protocol_version": 1,
  "event": "error",
  "job_id": "01J...",
  "error": {
    "code": "paste_unsafe_target_changed",
    "message": "Target app changed before paste; copied text to clipboard only"
  }
}
```

Never include transcript text in events by default.

Optional debug mode may include text only if explicitly enabled.

### 10.7 Slow consumer policy

Event streams must not backpressure dictation.

Policy:

```text
state/result/error events: small bounded buffer, disconnect very slow clients
preview events later: drop old preview events and keep latest only
```

---

## 11. Audio Capture

### 11.1 Goals

The audio layer must support:

```text
low-latency recording start
clean WAV output
runtime temp storage
future live preview tap
device-native capture
resampling to 16 kHz mono
final-path silence detection
```

### 11.2 Public trait

Do not expose backend stream ownership in the public trait.

```rust
#[async_trait]
pub trait AudioRecorder: Send + Sync {
    async fn start(&self, job_id: JobId) -> Result<AudioSession, AudioError>;
    async fn stop(&self, job_id: JobId) -> Result<AudioRecording, AudioError>;
    async fn cancel(&self, job_id: JobId) -> Result<(), AudioError>;
}
```

Implementation detail:

```text
Use an internal audio owner thread/task that creates, owns, and drops CPAL stream resources.
Communicate with it by channels.
Do not assume CPAL stream ownership can be casually moved through daemon state.
```

### 11.3 Audio recording output

```rust
pub struct AudioRecording {
    pub job_id: JobId,
    pub wav_path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub rms_energy: f32,
    pub peak_energy: f32,
    pub speech_detected: bool,
}
```

### 11.4 Capture flow

```text
open default input device
choose supported input config
capture device-native sample format
convert samples to f32
mix down to mono
resample to 16 kHz
write WAV under $XDG_RUNTIME_DIR/voxline/<job-id>.wav
track duration and energy
on stop, finalize WAV cleanly
on cancel/error, delete temp file
```

### 11.5 Resampling

Whisper expects 16 kHz mono PCM.

CPAL may provide:

```text
44.1 kHz
48 kHz
96 kHz
mono
stereo
multi-channel
integer or float sample formats
```

V1 must include real resampling. Do not fake this by setting a desired sample rate and assuming the device honors it.

### 11.6 Runtime storage

Temporary audio path:

```text
$XDG_RUNTIME_DIR/voxline/<job-id>.wav
```

Never default to:

```text
~/.cache/voxline/
```

Debug retention may be added:

```toml
[debug]
retain_audio = false
retain_transcripts = false
```

If debug retention is enabled, warn loudly in `voxline doctor`.

---

## 12. Final-Path Silence and Hallucination Handling

Silence handling is required on the final path, not just preview.

### 12.1 Pre-ASR gates

Config:

```toml
[audio.gates]
min_record_ms = 350
min_rms_energy = 0.003
min_peak_energy = 0.015
```

Behavior:

```text
if duration < min_record_ms:
  do not transcribe
  do not paste
  notify "No speech captured"

if energy below threshold:
  do not transcribe unless user disables gate
  do not paste
  notify "No speech detected"
```

Thresholds must be configurable because microphones vary.

### 12.2 Post-ASR gates

After transcription:

```text
trim whitespace
reject empty transcript
reject common silence hallucinations
reject repeated subtitle artifacts
```

Default known-hallucination filter:

```toml
[asr.hallucination_filter]
enabled = true
phrases = [
  "thank you.",
  "thanks for watching.",
  "subtitles by",
  "subtitle by",
  "captioned by"
]
```

Filtering rule:

```text
Use exact/near-exact matching only for very short transcripts.
Never remove legitimate phrases from longer dictated content.
```

---

## 13. ASR Backend

### 13.1 Primary backend

V1 primary backend:

```text
whisper-rs embedded whisper.cpp backend
```

Rationale:

```text
model can stay loaded in process
no subprocess JSON parsing for main path
no PATH probing for main path
lifecycle config is meaningful immediately
lower latency for repeated dictations
cleaner future preview path
```

### 13.2 Fallback backend

Keep a CLI backend only as fallback/debug:

```text
whisper.cpp CLI backend
```

Use cases:

```text
diagnose whisper-rs build problems
compare outputs
support environments where embedded backend fails
```

Do not build a required whisper-server middle layer unless benchmarking proves it is useful.

### 13.3 ASR manager

```rust
pub struct AsrManager {
    final_worker: FinalAsrWorker,
    lifecycle: AsrLifecycleConfig,
}
```

Responsibilities:

```text
load model
keep model warm
unload after idle timeout if configured
run transcription on blocking worker thread
track model state
emit model events
surface typed errors
```

### 13.4 Blocking isolation

Whisper transcription is CPU/GPU intensive and must not block the Tokio IPC/event loop.

Required implementation:

```text
run model load and transcription on dedicated blocking worker thread or spawn_blocking pool
keep IPC server responsive while ASR runs
emit state events before and after long operations
```

### 13.5 Typed errors

Library traits must not return `anyhow::Result`.

Use typed errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("model not found: {path}")]
    ModelNotFound { path: PathBuf },

    #[error("model load failed: {message}")]
    ModelLoadFailed { message: String },

    #[error("transcription failed: {message}")]
    TranscriptionFailed { message: String },

    #[error("unsupported backend feature: {feature}")]
    UnsupportedFeature { feature: String },
}
```

Binaries may wrap typed errors with `anyhow` at the outermost layer only.

### 13.6 Lifecycle modes

Start with two lifecycle modes:

```text
on_demand
keep_warm
```

`on_demand`:

```text
load model when needed
optionally unload after transcription
lowest idle resource usage
not optimized for the initial power-user target
```

`keep_warm`:

```text
load model at daemon start or first use
keep loaded for idle timeout
best initial profile for developer workstation
```

Config:

```toml
[asr.lifecycle]
mode = "keep_warm"
warm_on_daemon_start = true
idle_unload_seconds = 900
```

Do not add five lifecycle modes in v1.

### 13.7 Model naming

Avoid fake shorthand model names.

Config should point to exact files:

```toml
[asr]
model_path = "~/.local/share/voxline/models/ggml-large-v3-turbo-q5_0.bin"
```

Optional model resolver later:

```text
large-v3-turbo-q5 -> exact configured artifact path
small.en -> exact configured artifact path
```

The resolver must never silently download or guess in v1.

### 13.8 Power-user ASR profile

Initial target profile:

```toml
[asr]
backend = "whisper_rs"
model_path = "~/.local/share/voxline/models/ggml-large-v3-turbo-q5_0.bin"
language = "en"
threads = 8
gpu = true
gpu_backend = "cuda"

[asr.lifecycle]
mode = "keep_warm"
warm_on_daemon_start = true
idle_unload_seconds = 900
```

If the large turbo quantized model does not fit or perform well on the target GPU, benchmark alternatives and update the profile.

Required benchmark comparison before declaring v1 latency successful:

```text
base.en CPU
small.en CPU
small.en CUDA
large-v3-turbo quantized CUDA
```

---

## 14. Transcript and Result Types

### 14.1 Transcript

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
```

### 14.2 Dictation result

Do not include transcript text in public result events by default.

Internal result:

```rust
pub struct DictationResult {
    pub job_id: JobId,
    pub raw: Transcript,
    pub cleaned_text: Option<String>,
    pub final_text: String,
    pub cleanup_used: bool,
    pub cleanup_failed: bool,
    pub style_used: Option<String>,
    pub app_profile_used: Option<String>,
    pub snippet_used: Option<String>,
    pub copied_to_clipboard: bool,
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub clipboard_restored: bool,
    pub target_changed_before_paste: bool,
}
```

Public event result:

```rust
pub struct PublicResultEvent {
    pub job_id: JobId,
    pub cleanup_used: bool,
    pub cleanup_failed: bool,
    pub copied_to_clipboard: bool,
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub clipboard_restored: bool,
    pub warning_code: Option<String>,
}
```

---

## 15. Custom Vocabulary

Custom vocabulary is v1 scope.

Dictation apps live or die on proper nouns, project names, people names, acronyms, and product terms.

### 15.1 Config

```toml
[vocabulary]
enabled = true
initial_prompt_enabled = true
post_replace_enabled = true

[[vocabulary.phrases]]
text = "OpenRouter"

[[vocabulary.phrases]]
text = "Hyprland"

[[vocabulary.phrases]]
text = "VoxLine"

[[vocabulary.replacements]]
from = "hyper land"
to = "Hyprland"
case_sensitive = false

[[vocabulary.replacements]]
from = "open router"
to = "OpenRouter"
case_sensitive = false
```

### 15.2 Behavior

Use vocabulary in two ways:

```text
1. ASR biasing through initial prompt/context if backend supports it.
2. Deterministic post-ASR replacement before cleanup.
```

Order:

```text
raw transcript
  ↓
trim/normalize
  ↓
vocabulary replacements
  ↓
cleanup if enabled
  ↓
final output
```

### 15.3 Safety

Replacement rules should be conservative.

```text
Prefer whole-word matching.
Allow regex only if explicitly enabled.
Make replacements testable with voxline vocab test.
```

Commands:

```bash
voxline vocab list
voxline vocab add phrase "OpenRouter"
voxline vocab add replace "open router" "OpenRouter"
voxline vocab test "I configured open router in hyper land"
```

---

## 16. Cleanup

Cleanup is opt-in but important.

### 16.1 Default cleanup config

```toml
[cleanup]
enabled = false
provider = "none"
```

### 16.2 OpenRouter cleanup config

```toml
[cleanup]
enabled = true
provider = "openrouter"
model = "openai/gpt-4.1-mini"
temperature = 0.2
timeout_ms = 10000
fallback_to_raw_on_error = true
skip_if_word_count_below = 5
```

The exact model can change; config must not hard-code an unchangeable provider/model.

### 16.3 Request behavior

If cleanup is enabled:

```text
send final transcript text, not audio
use user's API key
apply timeout
fallback to raw on timeout/error/invalid output
emit cleanup_used and cleanup_failed flags
```

### 16.4 Cost and latency warning

Docs and setup must say:

```text
Every cleanup call may cost money and adds latency.
Short utterances can bypass cleanup.
Use --no-cleanup for commands, code, passwords, terminals, or sensitive content.
```

### 16.5 Default cleanup prompt

System prompt:

```text
You are the cleanup engine for VoxLine, a local dictation app.
Rewrite dictated speech into clean text ready to paste.
Preserve the user's meaning.
Do not add facts.
Remove filler words when appropriate.
Fix punctuation and capitalization.
Return only the final text.
```

User message:

```text
<transcript after vocabulary replacement and command stripping>
```

### 16.6 Validation

Validation is a fallback safety net, not the main guarantee.

Initial checks:

```text
not empty
not only whitespace
not a prompt explanation
not obvious boilerplate such as "Here is..." for default style
not wildly longer than input unless route allows it
```

If validation fails:

```text
fallback to raw transcript
mark cleanup_failed = true
emit warning
```

Later, validation must become route-specific.

---

## 17. Routing Roadmap

Routing should come quickly after default cleanup, but it should not block the first reliable local dictation milestone.

### 17.1 Styles

Directory:

```text
~/.config/voxline/styles/
```

Style metadata:

```toml
name = "professional"
description = "Clear, polished, professional prose."
prompt_file = "professional.md"
```

Prompt file:

```md
Rewrite dictated speech into clear professional text.
Preserve the user's meaning.
Do not add facts.
Return only the final text.
```

Priority order once styles exist:

```text
1. CLI override
2. future voice command override
3. app profile default
4. global default style
```

### 17.2 App profiles

App detection must happen at recording start and recording stop, not only at cleanup time.

Directory:

```text
~/.config/voxline/apps/
```

Example:

```toml
name = "Slack"
default_style = "casual"
match_process = ["slack"]
match_app_id = ["Slack"]

prompt = """
The user is typing into Slack or a similar chat app.
Use natural chat-message formatting.
Avoid sounding like a formal email.
Do not use markdown tables.
"""
```

Terminal profile:

```toml
name = "Terminal"
default_style = "verbatim"
match_process = ["alacritty", "kitty", "wezterm", "gnome-terminal"]

[cleanup]
enabled = false

[injection]
prefer_clipboard_only = true
```

### 17.3 Insert snippets

Insert snippets are allowed before template snippets.

Directory:

```text
~/.config/voxline/snippets/
```

Example:

```toml
name = "signature"
type = "insert"
aliases = ["signature", "insert signature"]
content_file = "signature.md"
```

Insert snippet behavior:

```text
No LLM extraction required.
Snippet content can be inserted directly.
```

### 17.4 Template snippets later

Template snippets require structured extraction.

Do not pretend this is simple string templating.

Required design later:

```text
snippet declares fields
cleanup prompt asks for structured JSON
parser validates fields
missing fields have configured fallback
render template only after validation
route-specific length validation
```

Example later schema:

```toml
name = "standup"
type = "template"
fields = ["yesterday", "today", "blocked"]
template_file = "standup.md"
```

### 17.5 Voice commands later

Voice commands are useful but brittle. They must be experimental at first.

Safer command strategy:

```text
Require a trigger prefix such as "VoxLine professional" or "VoxLine signature".
Avoid matching ordinary dictated sentences like "professional mode is important here".
```

Initial parser:

```text
start-only
lowercase
trim punctuation
collapse whitespace
match configured aliases
strip matched command from transcript
```

Add fuzzy matching only after tests and telemetry prove deterministic matching is too weak.

---

## 18. Active Target Detection

### 18.1 Target context

```rust
pub struct TargetContext {
    pub captured_at: SystemTime,
    pub session_type: Option<String>,
    pub desktop: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub window_id: Option<String>,
    pub app_id: Option<String>,
    pub detection_confidence: DetectionConfidence,
}
```

### 18.2 Detection timing

Capture target context:

```text
at recording start
at recording stop
immediately before paste
```

Use this for:

```text
paste safety
app profile routing later
terminal paste behavior later
```

### 18.3 Linux detection backends

```text
X11:
  xdotool/wmctrl or X11 APIs

Hyprland:
  hyprctl activewindow -j

Sway:
  swaymsg -t get_tree

GNOME Wayland:
  limited; often unavailable

KDE Wayland:
  limited/mixed
```

App detection is best-effort. Failure must not block dictation.

---

## 19. Clipboard and Paste

Paste correctness is a v1 production risk.

### 19.1 Insertion pipeline

```text
recording starts
  ↓
capture target_at_start
  ↓
recording stops
  ↓
capture target_at_stop
  ↓
transcribe and optional cleanup
  ↓
capture target_before_paste
  ↓
if target changed or result is too old:
    copy only
    notify user
else:
    save previous clipboard if configured
    write final text to clipboard
    wait for clipboard availability
    paste
    optionally restore previous clipboard
```

### 19.2 Config

```toml
[injection]
copy_to_clipboard = true
auto_paste = "safe"       # off | safe | always
max_paste_age_ms = 5000
restore_clipboard = true
paste_delay_ms = 120
fallback_to_clipboard_only = true
notify_on_clipboard_only = true
```

Default:

```toml
auto_paste = "safe"
```

Meaning:

```text
Paste only when the platform adapter believes paste is safe.
Otherwise copy to clipboard only.
```

### 19.3 Clipboard restore

Clipboard restore is required where possible.

Caveats to document:

```text
Wayland clipboard ownership is asynchronous.
Clipboard managers may race with restore.
Some clipboard contents cannot be faithfully restored through arboard.
If restore fails, final text remains on clipboard.
```

### 19.4 Linux paste commands

X11:

```bash
xdotool key ctrl+v
```

wlroots/Hyprland/Sway:

```bash
wtype -M ctrl -k v -m ctrl
```

Also acceptable:

```bash
wtype -M ctrl -P v -p v -m ctrl
```

Do not use:

```bash
wtype -M ctrl -P v -m ctrl
```

That presses `v` without releasing it.

### 19.5 Direct text typing option

On wlroots, `wtype` can type text directly. Add later as an insertion mode:

```toml
[injection]
mode = "clipboard_paste" # clipboard_paste | direct_type
```

Direct typing avoids clipboard destruction, but can have its own quoting, layout, and special-character risks.

### 19.6 GNOME Wayland

GNOME Wayland/Mutter should default to clipboard-only.

```text
Paste capability: clipboard-only
Reason: virtual keyboard paste tools are generally unsupported by default
```

Optional advanced override:

```toml
[injection.linux]
gnome_wayland_mode = "custom"
optional_paste_command = "ydotool key 29:1 47:1 47:0 29:0"
```

The app must not auto-configure `ydotool` because it requires uinput/daemon/permissions.

### 19.7 Terminal behavior

Many terminals use Ctrl+Shift+V instead of Ctrl+V.

V1 should document this.

Later, app profiles can specify paste commands:

```toml
[injection.profile.terminal]
paste_command = "ctrl+shift+v"
```

---

## 20. Linux Compatibility Matrix

| Session | Clipboard | Paste | App detection | Default v1 behavior |
|---|---:|---:|---:|---|
| X11 | yes | xdotool | good | safe paste if target stable |
| Hyprland | yes | wtype likely | hyprctl | safe paste if target stable |
| Sway | yes | wtype likely | swaymsg | safe paste if target stable |
| KDE Wayland | usually | mixed | limited | conservative, often clipboard-only |
| GNOME Wayland | yes | not by default | limited | clipboard-only |
| Unknown Wayland | usually | unknown | unknown | clipboard-only |
| Headless/no GUI | maybe no | no | no | record/transcribe only |

Do not promise universal Wayland paste.

---

## 21. Secrets

Use fallback chain:

```text
1. keyring
2. environment variable
3. explicitly enabled insecure file fallback
```

Config:

```toml
[secrets]
mode = "auto"
openrouter_env_var = "OPENROUTER_API_KEY"
allow_insecure_file_fallback = false
insecure_file_path = "~/.config/voxline/secrets.toml"
```

If file fallback is enabled:

```text
require file mode 0600
warn in doctor
never create it silently
```

Commands:

```bash
voxline secrets set openrouter
voxline secrets clear openrouter
voxline secrets status
```

Secret Service caveat:

```text
Linux keyring may fail if Secret Service is locked or unavailable during autostart.
Doctor must report this clearly.
```

---

## 22. Service Management

VoxLine runs as a user-session process, not a system service.

Install:

```text
~/.config/systemd/user/voxlined.service
```

Unit:

```ini
[Unit]
Description=VoxLine local dictation daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart=%h/.local/bin/voxlined
Restart=on-failure
RestartSec=2
Environment=RUST_LOG=info

[Install]
WantedBy=graphical-session.target
```

Caveat:

```text
Some WMs do not activate graphical-session.target or import display/session environment.
```

Hyprland example:

```text
exec-once = systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP DBUS_SESSION_BUS_ADDRESS
exec-once = systemctl --user start voxlined
```

`voxline service install` must print:

```text
service path
systemctl commands
shortcut binding recommendation
environment import guidance for Hyprland/Sway/custom WMs
```

Commands:

```bash
voxline service install
voxline service uninstall
voxline service start
voxline service stop
voxline service status
```

---

## 23. CLI Commands

### 23.1 Core commands

```bash
voxline status
voxline toggle
voxline start
voxline stop
voxline cancel
voxline watch
```

### 23.2 Recording and transcription

```bash
voxline test mic
voxline record --no-cleanup
voxline transcribe <audio-file> --no-cleanup
voxline bench asr <audio-file>
voxline bench end-to-end <audio-file> --no-cleanup
```

### 23.3 Config

```bash
voxline config path
voxline config init
voxline config validate
voxline config profile power-user-nvidia
voxline config profile cpu-safe
```

### 23.4 Doctor and tests

```bash
voxline doctor
voxline doctor --json
voxline test asr
voxline test clipboard
voxline test paste
voxline test app-detect
voxline test openrouter
```

### 23.5 Secrets

```bash
voxline secrets set openrouter
voxline secrets clear openrouter
voxline secrets status
```

### 23.6 ASR lifecycle

```bash
voxline asr status
voxline asr load
voxline asr unload
voxline asr restart
```

### 23.7 Vocabulary

```bash
voxline vocab list
voxline vocab add phrase "OpenRouter"
voxline vocab add replace "open router" "OpenRouter"
voxline vocab test "configure open router in hyper land"
```

### 23.8 Cleanup later

```bash
voxline cleanup enable openrouter
voxline cleanup disable
voxline cleanup preview "hey john thanks for catching that"
voxline toggle --cleanup
voxline toggle --no-cleanup
voxline toggle --style professional
```

### 23.9 Routing later

```bash
voxline styles list
voxline styles new professional
voxline styles edit professional
voxline apps detect
voxline apps list
voxline apps edit slack
voxline snippets list
voxline snippets new signature
voxline snippets insert signature
```

---

## 24. Main Config Example

Initial power-user config:

```toml
[daemon]
log_level = "info"
max_concurrent_jobs = 1
protocol_version = 1

[paths]
config_dir = "~/.config/voxline"
model_dir = "~/.local/share/voxline/models"
runtime_dir = "auto"

[audio]
backend = "cpal"
device = "default"
target_sample_rate = 16000
channels = 1
max_record_seconds = 300

[audio.gates]
min_record_ms = 350
min_rms_energy = 0.003
min_peak_energy = 0.015
notify_on_no_speech = true

[asr]
backend = "whisper_rs"
model_path = "~/.local/share/voxline/models/ggml-large-v3-turbo-q5_0.bin"
language = "en"
threads = 8
gpu = true
gpu_backend = "cuda"

[asr.lifecycle]
mode = "keep_warm"
warm_on_daemon_start = true
idle_unload_seconds = 900

[asr.hallucination_filter]
enabled = true
phrases = [
  "thank you.",
  "thanks for watching.",
  "subtitles by",
  "subtitle by",
  "captioned by"
]

[vocabulary]
enabled = true
initial_prompt_enabled = true
post_replace_enabled = true

[cleanup]
enabled = false
provider = "none"
model = ""
temperature = 0.2
timeout_ms = 10000
fallback_to_raw_on_error = true
skip_if_word_count_below = 5

[injection]
copy_to_clipboard = true
auto_paste = "safe"
max_paste_age_ms = 5000
restore_clipboard = true
paste_delay_ms = 120
fallback_to_clipboard_only = true
notify_on_clipboard_only = true

[injection.linux]
session = "auto"
wayland_paste_command = "wtype -M ctrl -k v -m ctrl"
x11_paste_command = "xdotool key ctrl+v"
gnome_wayland_mode = "clipboard_only"
optional_paste_command = ""

[secrets]
mode = "auto"
openrouter_env_var = "OPENROUTER_API_KEY"
allow_insecure_file_fallback = false
insecure_file_path = "~/.config/voxline/secrets.toml"

[notifications]
enabled = true

[privacy]
store_history = false
store_audio = false
store_raw_transcript = false
store_cleaned_transcript = false
log_transcripts = false
```

Future CPU-safe profile:

```toml
[asr]
backend = "whisper_rs"
model_path = "~/.local/share/voxline/models/ggml-small.en.bin"
language = "en"
threads = 4
gpu = false

[asr.lifecycle]
mode = "on_demand"
warm_on_daemon_start = false
idle_unload_seconds = 0

[cleanup]
enabled = false
```

Cleanup-enabled profile:

```toml
[cleanup]
enabled = true
provider = "openrouter"
model = "openai/gpt-4.1-mini"
temperature = 0.2
timeout_ms = 10000
fallback_to_raw_on_error = true
skip_if_word_count_below = 5
```

---

## 25. Doctor Requirements

`voxline doctor` is required before production use.

### 25.1 Session checks

Report:

```text
XDG_SESSION_TYPE
XDG_CURRENT_DESKTOP
WAYLAND_DISPLAY
DISPLAY
DBUS_SESSION_BUS_ADDRESS
XDG_RUNTIME_DIR
```

### 25.2 Daemon environment

CLI must ask daemon for its environment/capabilities.

If CLI has display variables but daemon does not:

```text
Likely systemd user environment import problem.
```

### 25.3 Runtime dir

Check:

```text
XDG_RUNTIME_DIR exists
$XDG_RUNTIME_DIR/voxline can be created
mode is 0700
socket owned by current user
```

### 25.4 Audio

Check:

```text
CPAL default input device exists
supported input configs
sample format
record test possible
energy gate calibrated enough to detect speech
```

### 25.5 ASR

Check:

```text
backend available
whisper-rs build profile includes expected GPU support
model file exists
model can load
short sample can transcribe
lifecycle mode valid
```

### 25.6 Paste capability

Examples:

```text
Session: Wayland
Desktop: GNOME
Paste capability: clipboard-only
Reason: GNOME Wayland does not expose a default virtual keyboard path for wtype-style paste
```

```text
Session: Wayland
Desktop: Hyprland
Paste capability: wtype likely supported
Active app detection: hyprctl available
```

```text
Session: X11
Paste capability: xdotool supported if installed
Active app detection: available
```

### 25.7 External tools

Probe:

```text
wtype
xdotool
ydotool
hyprctl
swaymsg
wmctrl
notify-send
wl-copy
wl-paste
xclip
```

Not every tool is required. Doctor must explain which are relevant for the current session.

### 25.8 Secret store

Report:

```text
keyring available/unavailable
env fallback present/absent
insecure file fallback disabled/enabled
OpenRouter key configured yes/no
```

### 25.9 Cleanup routing later

When routing exists, check:

```text
default style exists
style prompt files parse
app profiles parse
snippet files parse
voice command aliases conflict or not
```

### 25.10 Privacy audit

Report:

```text
store_audio
store_raw_transcript
store_cleaned_transcript
log_transcripts
cleanup provider
cleanup enabled
```

If cleanup enabled:

```text
Warning: transcript text is sent to configured provider.
```

---

## 26. Milestones

### M0: Repo skeleton

Deliver:

```text
workspace
voxlined binary
voxline CLI
config loading
tracing
typed errors
basic doctor shell
```

Acceptance:

```bash
voxline config init
voxline config validate
voxlined --foreground
voxline status
voxline doctor
```

### M1: IPC and state

Deliver:

```text
Unix socket server
request/response protocol
protocol_version
request_id
job_id
status command
event stream
voxline watch
orthogonal job/model state
```

Acceptance:

```bash
voxline watch
voxline status
voxline toggle
```

Before audio exists, toggle may emit a placeholder error, but IPC and events must work.

### M2: Audio recorder

Deliver:

```text
CPAL capture
internal audio owner thread/task
sample conversion
resampling to 16 kHz mono
WAV writing under XDG_RUNTIME_DIR
energy/duration tracking
clean stop/cancel/delete
```

Acceptance:

```bash
voxline test mic
voxline record --no-cleanup
```

Inspect the WAV and verify valid duration/header.

### M3: whisper-rs ASR

Deliver:

```text
whisper-rs backend
CUDA build profile for power-user target
model loading
keep_warm lifecycle
blocking worker isolation
typed ASR errors
transcribe command
vocabulary initial prompt/replacement
hallucination filter
ASR benchmarks
```

Acceptance:

```bash
voxline asr load
voxline asr status
voxline transcribe ./sample.wav --no-cleanup
voxline bench asr ./sample.wav
voxline vocab test "open router in hyper land"
```

### M4: Full local dictation to clipboard

Deliver:

```text
toggle start/stop
record
silence gate
transcribe
vocabulary replacements
copy final text to clipboard
notify success
no cleanup
no auto-paste required yet
```

Acceptance:

```bash
voxline toggle
# speak
voxline toggle
# transcript is on clipboard
```

This is the first true usable local dictation milestone.

### M5: Safe paste injection

Deliver:

```text
active target capture at start/stop/before paste
X11 paste adapter
wlroots wtype paste adapter
GNOME clipboard-only behavior
max paste age
clipboard save/restore
paste doctor report
clipboard-only fallback notification
```

Acceptance:

```bash
voxline test clipboard
voxline test paste
voxline doctor
voxline toggle
# speak
voxline toggle
# text appears in target app where safe, else remains on clipboard
```

### M6: Service and trigger docs

Deliver:

```text
systemd user service install/uninstall/start/stop/status
Hyprland environment import guidance
GNOME/KDE shortcut docs
Sway/Hyprland toggle and PTT docs
service install prints binding instructions
```

Acceptance:

```bash
voxline service install
systemctl --user start voxlined
voxline doctor
# bind key to voxline toggle
# dictate without terminal focus
```

This is the v1 baseline.

### M7.5: Config layout and documented shape

Deliver:

```text
full config.toml scaffold matching section 24
styles/, apps/, snippets/, and model directories on config init
[injection.linux] config section
paths.runtime_dir resolution (auto → XDG runtime)
voxline config profile power-user-nvidia
voxline config profile cpu-safe
doctor check for routing directory scaffold
```

Acceptance:

```bash
voxline config init
voxline config profile cpu-safe
voxline config validate
voxline doctor
```

Blocks M8a/M8b/M8c routing work.

### M7: OpenRouter cleanup

Deliver:

```text
secrets set/status
OpenRouter client
cleanup disabled by default
explicit cleanup enable command
default style prompt
cleanup timeout
fallback to raw
short utterance cleanup skip
cleanup cost/latency warning
```

Acceptance:

```bash
voxline secrets set openrouter
voxline cleanup enable openrouter
voxline test openrouter
voxline cleanup preview "hey john thanks for catching that"
voxline toggle --cleanup
```

### M8a: Styles

Deliver:

```text
style config directory
style prompt files
CLI style override
style validation
cleanup preview by style
```

Acceptance:

```bash
voxline styles new professional
voxline cleanup preview --style professional "hey john thanks"
voxline toggle --style professional
```

### M8b: App profiles

Deliver:

```text
active app profile matching
app default style
terminal cleanup-off profile
app prompt layer
app detection doctor
```

Acceptance:

```bash
voxline apps detect
voxline apps edit slack
voxline toggle
```

### M8c: Insert snippets

Deliver:

```text
insert snippet config
snippet content files
CLI snippet insertion
snippet command path without LLM extraction
```

Acceptance:

```bash
voxline snippets new signature
voxline snippets insert signature
voxline toggle --snippet signature
```

### M8d: Voice commands experimental

Deliver:

```text
start-only command parser
optional trigger prefix
alias conflict detection
command stripping
voice command test CLI
feature flag or config marked experimental
```

Acceptance:

```bash
voxline commands test "voxline professional hey john thanks"
voxline toggle
```

### M9: Template snippets

Deliver only after structured extraction design is complete.

```text
template field schema
LLM structured JSON extraction
field validation
missing field behavior
route-specific validation
rendered template output
```

### M10: Realtime preview

Deliver:

```text
AudioTap/ring buffer
preview ASR worker
CPU preview default unless benchmarked otherwise
chunk/step/overlap config
local agreement stable/provisional output
energy VAD gating
preview events
slow-consumer drop policy
voxline watch preview display
```

Preview must never paste.

### M11: Optional overlay

Deliver:

```text
separate overlay process
subscribes to event stream
renders stable/provisional preview
GNOME limitation docs
terminal watch fallback
```

### M12: Hardening and packaging

Deliver:

```text
privacy audit
no transcript logs by default
socket permission checks
doctor suggestions
benchmark report
manual desktop matrix
install docs
release builds for power-user Linux
CPU-safe profile
```

---

## 27. Testing Plan

### 27.1 Unit tests

```text
config parsing
config validation
path expansion
state transitions
busy behavior
IPC serialization
job_id propagation
silence gate
hallucination filter
vocabulary replacements
cleanup fallback
output validation
clipboard restore decision logic
paste target mismatch logic
style priority later
snippet parsing later
voice command parser later
```

### 27.2 Integration tests

```text
CLI <-> daemon IPC
subscribe event stream
slow subscriber disconnect/drop behavior
record generated or fixture audio
transcribe fixture audio
mock ASR backend
mock cleanup backend
mock clipboard
mock paste command
mock active app detector
service file generation
```

### 27.3 Manual Linux matrix

Test manually on:

```text
X11
GNOME Wayland
KDE Wayland
Hyprland
Sway
headless/no graphical env
no Secret Service
bad model path
ASR model too large
cleanup timeout
OpenRouter key missing
clipboard manager installed
terminal paste target
focus changes during transcription
accidental short toggle
silent recording
```

### 27.4 Latency tests

For each candidate model/profile:

```text
model load time
warm transcription time
cold transcription time
stop-to-clipboard
stop-to-insert
cleanup latency
fallback latency on cleanup timeout
```

Do not accept v1 until power-user profile meets the latency target or the target is explicitly revised.

---

## 28. Logging and Privacy

Default logging must never include:

```text
raw transcript
cleaned transcript
audio path after deletion
API keys
clipboard contents
window titles if privacy mode disables them
```

Allowed logs:

```text
job_id
state transitions
durations
backend names
error codes
capability results
```

Debug logging that includes sensitive data must require explicit config:

```toml
[debug]
log_transcripts = false
retain_audio = false
retain_cleanup_payloads = false
```

Doctor must warn if any sensitive debug option is enabled.

---

## 29. macOS Port Plan

Do not build first, but preserve architecture.

Shared Rust:

```text
voxline-core
voxlined orchestration
voxline-cli
cleanup
routing
vocabulary
ASR manager
```

Likely macOS-specific work:

```text
CoreAudio through CPAL if sufficient
Keychain through keyring
pasteboard through arboard or native adapter
Accessibility permission for paste
microphone permission onboarding
launchd LaunchAgent
optional Swift helper
optional menu bar app
```

Do not rewrite core in Swift.

Use Swift only for:

```text
permissions UX
native menu bar/settings
accessibility/paste helper if Rust adapter is insufficient
launch-at-login UX
```

---

## 30. Windows Port Plan

Do not build first, but preserve architecture.

Shared Rust:

```text
voxline-core
voxlined orchestration
voxline-cli
cleanup
routing
vocabulary
ASR manager
```

Windows-specific work:

```text
WASAPI through CPAL
Credential Manager through keyring
Windows Clipboard through arboard/native adapter
SendInput paste
RegisterHotKey later
named pipe IPC
startup task or Startup folder shortcut
tray app later
```

Do not build as a Windows Service.

Reason:

```text
Windows Services run outside the normal interactive user session and are poor for microphone, clipboard, hotkeys, and active-window paste.
```

---

## 31. Agent Build Instructions

An implementation agent should follow these rules:

```text
1. Do not skip paste safety.
2. Do not enable cloud cleanup by default.
3. Do not log transcripts by default.
4. Do not add more crates until the 4-crate workspace becomes painful.
5. Do not block the IPC server on ASR or cleanup.
6. Do not assume Wayland paste works everywhere.
7. Do not add template snippets before structured extraction exists.
8. Do not add voice commands to the critical path.
9. Do not claim local-first without explaining cleanup boundaries.
10. Benchmark latency before declaring milestones complete.
```

The most important build sequence is:

```text
IPC -> audio -> whisper-rs ASR -> clipboard -> safe paste -> service/trigger docs -> cleanup -> routing
```

The first product win is:

```text
bind a key to voxline toggle, speak, stop, and see local transcript inserted safely
```

The second product win is:

```text
enable cleanup explicitly and get polished text with fallback and clear privacy/cost boundaries
```

---

## 32. First Development Checklist

Start here:

```text
[ ] Create workspace with voxline-core, voxlined, voxline-cli, voxline-platform.
[ ] Define config structs and default power-user config.
[ ] Define protocol_version, request_id, job_id, request/response/event types.
[ ] Implement UDS IPC server and CLI status command.
[ ] Implement event subscription and voxline watch.
[ ] Implement JobState and ModelState separately.
[ ] Implement CPAL audio owner and record-to-WAV.
[ ] Add resampling to 16 kHz mono.
[ ] Add duration/energy gates.
[ ] Integrate whisper-rs backend on blocking worker.
[ ] Add CUDA build profile for initial machine.
[ ] Add keep_warm model lifecycle.
[ ] Add vocabulary replacement.
[ ] Add hallucination filter.
[ ] Implement local dictation to clipboard.
[ ] Implement active target detection for current Linux session.
[ ] Implement conservative safe paste.
[ ] Implement clipboard restore.
[ ] Implement doctor capability report.
[ ] Implement systemd user service install.
[ ] Write trigger docs for toggle and PTT.
[ ] Add OpenRouter cleanup as opt-in.
```
