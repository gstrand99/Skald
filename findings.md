# VoxLine code review findings

**Branch reviewed:** main @ 46f074f
**Review date:** 2026-06-11
**Scope:** security, maintainability, performance

Baseline: `just check` (fmt, clippy `-D warnings`, full test suite) passes cleanly on this
commit. All findings below are static-analysis/code-reading results; nothing was validated
against live hardware (microphone, GPU, desktop session) or external credentials. Findings
that would need such validation are marked accordingly.

## Executive summary

- The privacy boundary leaks inside the machine: `Event::Result` broadcasts the full
  transcript text to every IPC subscriber, contradicting the implementation plan's
  "never include transcript text in events by default" rule (plan sections 10.6 and 14.2).
  The network-facing boundaries (transcript-only cleanup, rustls, keyring-first secrets)
  are otherwise implemented well.
- The audio capture path has the highest-risk runtime defects: `max_record_seconds` is
  documented and defaulted but never enforced, the capture buffer grows without bound, the
  preview tap re-processes the entire recording history every tick while contending on the
  audio callback's mutex, and CPAL stream errors are warn-logged and otherwise ignored.
- `crates/voxlined/src/main.rs` (~2,130 lines) concentrates command dispatch, the job state
  machine, the dictation pipeline, benchmarks, and event streaming with no tests, and its
  check-then-act busy guards race under concurrent IPC clients. This is the main
  structural risk for future work.

## Statistics

- Files reviewed: ~40 Rust source files (~12,400 lines across 5 crates), 21 docs pages,
  `config-example/linux/config.toml`, `justfile`, `README.md`,
  `VoxLine_implementation_plan.md`
- Findings by severity:
  - Critical: 0
  - High: 6
  - Medium: 27
  - Low: 14

## Critical and high findings

No critical findings. Nothing reviewed is remotely exploitable or destroys user data; the
items below are the highest-impact gaps against the plan's own requirements.

### 1. `Event::Result` broadcasts full transcript text to all IPC subscribers

- **Severity:** high
- **Category:** security
- **Location:** `crates/voxline-core/src/protocol.rs` — `DictationResult`, `Event::Result`;
  `crates/voxlined/src/main.rs` — `finish_dictation` and snippet/template delivery paths
  (`state.events.send(Event::Result { ... })` at ~1039, ~1379, ~1538, ~1630)
- **Status:** confirmed
- **Description:** `DictationResult` embeds the complete `Transcript` (full text plus all
  segments), and every `Event::Result` broadcast carries it to any client subscribed on the
  daemon socket. The plan (sections 10.6, 14.2) requires a redacted `PublicResultEvent`
  with metadata only, with text gated behind an explicit debug flag.
- **Risk or impact:** Any same-user process can passively harvest every dictation by
  subscribing to events, without triggering dictation itself. The plan accepts same-user
  *command* access as a v1 risk, but explicitly does not accept transcript text in events.
- **Evidence:** `protocol.rs` lines ~203–220 (`pub transcript: Transcript` in
  `DictationResult`), ~289–293 (`Event::Result { result: DictationResult }`). No redaction
  or privacy gate exists on the broadcast path in `main.rs`. `voxline watch` then prints
  `result.transcript.text` to stdout.
- **Remediation:** Emit a metadata-only result event by default (the plan's
  `PublicResultEvent` shape). Include transcript text only when an explicit debug/privacy
  config option is enabled, and have `voxline doctor` warn when it is. Note `Event::Preview`
  necessarily carries text for the overlay; document it as a deliberate exception.

### 2. `max_record_seconds` is documented but never enforced; capture buffer is unbounded

- **Severity:** high
- **Category:** performance / correctness
- **Location:** `crates/voxlined/src/audio.rs` — `start_recording`, `build_stream`;
  `crates/voxline-core/src/config.rs` — `AudioConfig.max_record_seconds` (default 300)
- **Status:** confirmed
- **Description:** `max_record_seconds` exists in the config schema, the example config,
  the plan (section 24), and the docs (`docs/src/content/docs/configuration/audio.md`
  calls it a "Safety cap on recording length"). No code in `voxlined` reads it. The CPAL
  callback extends a shared `Vec<f32>` indefinitely (`audio.rs` ~268–271, buffer created
  at ~241).
- **Risk or impact:** A forgotten toggle records device-native audio into RAM without
  limit (48 kHz stereo f32 is ~370 KiB/s; an hour is ~1.3 GiB) and then pays mixdown,
  resample, and WAV-write cost for the whole buffer at stop. The documented safety
  behavior simply does not exist.
- **Evidence:** `grep max_record_seconds` matches only `config.rs`, the example config,
  the plan, and docs — zero matches in `crates/voxlined/`.
- **Remediation:** Enforce the cap in the capture path (stop extending or auto-stop the
  job when `samples.len()` exceeds `max_record_seconds * native_rate * channels`), emit a
  notification/error event when triggered, and add a regression test.

### 3. Preview tap re-processes the entire recording every tick and contends with the audio callback

- **Severity:** high
- **Category:** performance
- **Location:** `crates/voxlined/src/audio.rs` — `RecordingTap::resampled_snapshot`
  (~64–73); `crates/voxlined/src/preview.rs` — `run_preview_loop` (~155)
- **Status:** confirmed
- **Description:** Every preview step (default 1 s), `resampled_snapshot()` locks the same
  mutex the real-time CPAL callback uses, clones the **entire** raw capture buffer, mixes
  it to mono, linearly resamples all of it, and only then trims to the ring-buffer window.
  Cost grows linearly with recording length, so total preview work is quadratic over a
  session.

```64:73:crates/voxlined/src/audio.rs
    pub fn resampled_snapshot(&self) -> Vec<f32> {
        let raw = self
            .samples
            .lock()
            .map(|samples| samples.clone())
            .unwrap_or_default();
        let mono = mix_to_mono(&raw, self.channels);
        let resampled = resample_linear(&mono, self.sample_rate, self.target_sample_rate);
        voxline_core::preview::trim_to_ring_buffer(resampled, self.max_samples)
    }
```

- **Risk or impact:** Long dictations with preview enabled burn CPU and allocate hundreds
  of MB; while the snapshot holds the lock for the full clone, the audio callback blocks,
  risking dropped input frames (xruns) exactly when the buffer is largest.
- **Remediation:** Maintain a bounded ring buffer at capture time (or per-tick incremental
  resampling of only new samples) so the preview tap touches a fixed-size window. Keep the
  callback's critical section to an append only.

### 4. Job-state busy checks are check-then-act and race under concurrent clients

- **Severity:** high
- **Category:** correctness
- **Location:** `crates/voxlined/src/main.rs` — `toggle` (~715), `start` (~738),
  `bench_dictation` (~418), and similar guards in `transcribe`, `setup_record`,
  `bench_model_compare`, `insert_snippet`
- **Status:** confirmed (race window verified in code; not reproduced live)
- **Description:** Each handler reads `status.read().await.job_state != JobState::Idle`,
  releases the lock, then proceeds through async steps before any write-lock state update.
  Two concurrent IPC clients can both observe `Idle` and both start pipelines, violating
  the plan's `max_concurrent_jobs = 1` and the documented busy matrix (plan section 9).
- **Risk or impact:** Overlapping recordings/transcriptions/pastes with interleaved state
  events; benchmark commands can also interleave with live dictation. Single-user CLI
  usage rarely hits this, but compositor keybinds firing in bursts can.
- **Remediation:** Make the Idle-to-Recording (and other entry) transitions atomic — e.g.
  take the status write lock once, verify-and-transition inside one critical section, or
  serialize job entry through a dedicated orchestrator task/mutex.

### 5. Audio stream failures are warn-logged and otherwise ignored

- **Severity:** high
- **Category:** correctness
- **Location:** `crates/voxlined/src/audio.rs` — `build_stream` error callback (~262)
- **Status:** confirmed
- **Description:** The only handling for CPAL stream errors (device unplugged, stream
  died) is `tracing::warn!`. The job stays in `Recording`; nothing notifies the job layer
  or user.
- **Risk or impact:** The user dictates into a dead stream and discovers it only when the
  result is empty or partial; the "no speech detected" gate then masks the hardware
  failure as a user error.
- **Remediation:** Propagate stream errors to the owner thread (shared flag or channel),
  fail the active recording with a typed `AudioError` variant, emit `Event::Error`, and
  notify the user.

### 6. Non-interactive setup rewrites an existing config without `--force`

- **Severity:** high
- **Category:** correctness
- **Location:** `crates/voxline-cli/src/setup_cmd.rs` — `run` (~44–52 reconfigure prompt,
  ~198–204 unconditional save)
- **Status:** confirmed
- **Description:** The "Reconfigure?" confirmation only runs when interactive. With
  `--non-interactive` (and without `--force`), setup proceeds and ends with
  `final_config.save()`, replacing ASR model selection, lifecycle, preview, and cleanup
  settings derived from re-detection.
- **Risk or impact:** Scripted runs (CI, provisioning, `just install` wrappers) silently
  clobber a user's tuned configuration. `Config::from_setup_selection` preserves some
  fields, but model/lifecycle/preview selections are overwritten.
- **Remediation:** In non-interactive mode, require `--force` to touch an existing config
  and exit with a clear message otherwise (mirroring the `--if-missing` semantics).

## Medium findings

### 7. Model downloads have no cryptographic integrity verification

- **Severity:** medium
- **Category:** security
- **Location:** `crates/voxline-core/src/download.rs` — `download_model`;
  `crates/voxline-core/src/models.rs` — catalog
- **Status:** confirmed
- **Description:** Downloads are HTTPS-pinned to Hugging Face (good), but the only
  validation is size heuristics: skip re-download if an existing file is ≥ 50% of the
  approximate catalog size; accept a finished download if ≥ 25% of it. No SHA-256.
- **Risk or impact:** A truncated, corrupted, or substituted artifact (CDN fault or
  upstream compromise) passes validation and is loaded into the daemon. Whisper GGML files
  are parsed by native code in whisper.cpp.
- **Remediation:** Add expected SHA-256 per `ModelCatalogEntry`, verify after download and
  before the skip-if-exists early return; delete `.part` files on failure.

### 8. Insecure secrets-file fallback is invisible in human doctor output

- **Severity:** medium
- **Category:** security
- **Location:** `crates/voxline-cli/src/main.rs` — `print_doctor` (~912–921),
  `build_doctor_suggestions`
- **Status:** confirmed
- **Description:** Plan sections 21 and 25.8 require doctor to report and warn when the
  plaintext file fallback is enabled. The JSON report includes
  `secrets.insecure_file_enabled`, but the human output prints only keyring/env lines and
  no warning or suggestion is generated. (The 0600 mode enforcement on the file itself is
  correctly implemented in `secrets.rs`.)
- **Risk or impact:** The recommended pre-production check silently passes a plaintext
  API-key configuration.
- **Remediation:** Print the insecure-file status in `print_doctor` and add a prominent
  warning plus a suggestion to migrate to the keyring when enabled.

### 9. `[privacy]` storage flags parse but do nothing

- **Severity:** medium
- **Category:** security / maintainability
- **Location:** `crates/voxline-core/src/config.rs` — `PrivacyConfig`;
  `crates/voxlined/src/main.rs` (only `store_audio` and `log_transcripts` are read)
- **Status:** confirmed
- **Description:** `store_history`, `store_raw_transcript`, and `store_cleaned_transcript`
  exist in config, the example file, and doctor output, but have no runtime effect. The
  docs honestly mark them "reserved", but doctor reports them as if they were live
  controls and `sensitive_storage_or_logging_enabled()` factors them into warnings.
- **Risk or impact:** Setting them `true` warns the user but stores nothing — confusing in
  the benign direction today, and a trap when someone wires them up without revisiting
  every consumer.
- **Remediation:** Either reject `true` values in `Config::validate()` until implemented,
  or implement the storage paths with strict permissions and doctor warnings.

### 10. Runtime directory ownership is never verified

- **Severity:** medium
- **Category:** security
- **Location:** `crates/voxline-core/src/runtime.rs` — `ensure_runtime_dir_for`,
  `verify_mode`; `RuntimeError::NotOwned` (~15) is defined but never constructed
- **Status:** confirmed (gap); speculative (exploitability — requires a misconfigured
  `paths.runtime_dir` pointing at another user's writable directory)
- **Description:** Only permission bits are checked (0700 dir, 0600 socket); UID ownership
  is not, despite the plan's "socket owned by current user" requirement and the dead
  `NotOwned` variant signaling original intent.
- **Remediation:** After `create_dir_all`, compare `metadata.uid()` against `geteuid()`
  and return `NotOwned` on mismatch; surface the same check in doctor.

### 11. Paste-target stability comparison includes window titles

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxlined/src/injection.rs` — `targets_are_stable` (~167–173);
  `crates/voxline-platform/src/lib.rs` — `TargetContext`
- **Status:** confirmed (fails safe)
- **Description:** Stability requires full struct equality (`start == stop ==
  before_paste`) including `title`. Titles change constantly (editor dirty markers,
  browser tab updates, terminal output, media players), so paste falls back to
  clipboard-only even when the user never left the window.
- **Risk or impact:** Failure direction is safe (clipboard-only), but frequent false
  fallbacks erode trust in `auto_paste = "safe"` and push users toward `"always"`, which
  weakens real safety.
- **Remediation:** Compare stable identity only (`backend`, `id`, `app_id`); keep `title`
  for display/diagnostics.

### 12. X11 target `app_id` holds a PID, not an application class

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxline-platform/src/lib.rs` — `capture_x11_target` (~571–580)
- **Status:** confirmed
- **Description:** `app_id` is populated from `xdotool getwindowpid`. App profile matching
  (`match_app_id`) and terminal heuristics compare against names like `"kitty"`, which
  never match a numeric PID. PIDs also change per process, adding noise to the stability
  comparison.
- **Risk or impact:** App-profile routing and terminal detection are effectively broken on
  X11; `voxline apps detect` output is misleading.
- **Remediation:** Use `xdotool getactivewindow getwindowclassname` (or WM_CLASS via
  `wmctrl -lx`) for `app_id`; keep the PID in a separate field if needed.

### 13. Hallucination filter is exact-match only and leaves filtered text in segments

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxlined/src/asr.rs` — `filter_hallucination` (~436–450),
  `Worker::transcribe` (~308–322)
- **Status:** confirmed
- **Description:** Two issues: (a) matching is full-string equality after lowercasing, so
  the shipped defaults like `"subtitles by"` never match real artifacts such as
  "Subtitles by the Amara.org community", and `"thank you"` without a period slips
  through; plan section 12.2 calls for exact/near-exact matching on short transcripts.
  (b) When the filter empties `text`, `Transcript.segments` still contains the
  hallucinated content, which flows into responses and events.
- **Risk or impact:** Silence hallucinations reach the clipboard; consumers reading
  segments see text the filter rejected.
- **Remediation:** Normalize punctuation and support prefix matching for the subtitle-class
  phrases (still gated to ≤5 words); clear or filter segments when the text is rejected.

### 14. Resampling is naive linear interpolation with no anti-aliasing

- **Severity:** medium
- **Category:** performance / quality
- **Location:** `crates/voxlined/src/audio.rs` — `resample_linear` (~355–368)
- **Status:** confirmed (code); speculative (accuracy impact — needs A/B WER measurement)
- **Description:** The plan (section 5) specifies `rubato` or equivalent; downsampling
  48 kHz to 16 kHz by linear interpolation without a low-pass filter folds energy above
  8 kHz into the audible band as aliasing. No `rubato` dependency exists.
- **Risk or impact:** Potentially degraded Whisper accuracy versus properly filtered
  input, particularly with sibilants and noisy mics. Both the final path and the preview
  path are affected.
- **Remediation:** Use `rubato` (or a polyphase FIR) on the audio owner/preview threads.
  Benchmark WER before/after on the setup fixture to confirm the impact.

### 15. Blocking clipboard/paste subprocess I/O and sleeps run on the async runtime

- **Severity:** medium
- **Category:** performance
- **Location:** `crates/voxlined/src/main.rs` — `copy_final_text` (~1727),
  `deliver_text_to_target` (~1682–1704); `crates/voxline-platform/src/lib.rs` —
  `copy_to_clipboard`, `read_clipboard`, `wait_for_clipboard`, `paste`
- **Status:** confirmed
- **Description:** `wl-copy`/`xclip`/`wtype`/`hyprctl` subprocess waits and
  `thread::sleep` paste delays execute inside async handlers without `spawn_blocking`.
- **Risk or impact:** Tokio worker threads stall during the inject phase; IPC
  responsiveness suffers exactly when status queries are most likely (plan section 13.4's
  responsiveness requirement extends in spirit to the whole pipeline).
- **Remediation:** Wrap clipboard save/copy/paste/restore and the paste delay in
  `tokio::task::spawn_blocking` (or a dedicated injection thread mirroring the audio/ASR
  owner pattern).

### 16. Preview events are published twice and a lagging client loses its whole stream

- **Severity:** medium
- **Category:** performance / correctness
- **Location:** `crates/voxlined/src/preview.rs` — `publish_preview` (~196–210);
  `crates/voxlined/src/main.rs` — `stream_subscribe` (~2019–2055), `stream_events`
  (~2094–2111), `broadcast::channel(32)` (~83)
- **Status:** confirmed
- **Description:** Preview snapshots are sent to both the coalescing `watch` channel and
  the `broadcast` channel, so subscribers requesting `Preview` plus other kinds receive
  duplicates. On `RecvError::Lagged`, the subscription task bails entirely, disconnecting
  the client rather than skipping ahead. The plan's slow-consumer policy is
  "drop old preview events, keep latest" and "disconnect very slow clients" only for the
  small control events.
- **Risk or impact:** Wasted IPC traffic, duplicate UI updates, and overlay/watch
  disconnects during bursty state activity; high-rate preview events flowing through the
  size-32 broadcast buffer make lag likely.
- **Remediation:** Serve preview exclusively from the `watch` channel (drop
  `EventKind::Preview` from broadcast matching), and on `Lagged` resubscribe/skip rather
  than disconnect.

### 17. `voxlined/src/main.rs` is a ~2,130-line god module with no tests

- **Severity:** medium
- **Category:** maintainability
- **Location:** `crates/voxlined/src/main.rs` (entire file)
- **Status:** confirmed
- **Description:** IPC dispatch, the job state machine, the dictation pipeline, snippet
  and template delivery, benchmarks, clipboard delivery, subscriptions, and notification
  logic share one file. `main.rs`, `preview.rs`, `preview_asr.rs`, `cleanup.rs`,
  `openrouter.rs`, and `template_extract.rs` have zero tests; the busy matrix, event
  filtering, and delivery decisions are only exercised manually.
- **Risk or impact:** The race in finding 4 is hard to fix confidently without extractable,
  testable units; future features keep accreting here.
- **Remediation:** Extract `ipc`, `jobs` (state machine as pure functions), `bench`, and
  `delivery` modules; add table-driven state-transition tests covering the plan section 9
  busy matrix.

### 18. Config loading: no validation on load, `paths.config_dir` ignored, no schema version

- **Severity:** medium
- **Category:** maintainability / correctness
- **Location:** `crates/voxline-core/src/config.rs` — `Config::path` (~473–477),
  `load_or_default` (~479–489); `crates/voxline-core/src/setup.rs` — `is_setup_complete`
- **Status:** confirmed
- **Description:** Three related gaps: (a) `load_or_default()` parses but never calls
  `validate()`; daemon startup validates explicitly, but many CLI paths and the repeated
  mid-job reloads (finding 25) do not. (b) `Config::path()` hardcodes
  `dirs::config_dir()/voxline/config.toml`, so the `paths.config_dir` setting relocates
  styles/apps/snippets but not the config file itself, and `is_setup_complete` checks the
  hardcoded path too. (c) There is no schema version or migration story; renames silently
  revert affected fields to defaults via `#[serde(default)]`.
- **Risk or impact:** Invalid or relocated configs behave inconsistently across commands;
  future schema evolution risks silent misconfiguration.
- **Remediation:** Validate inside `load_or_default` (or add a validating `load()` and
  migrate callers); document that `config.toml` location is fixed (or honor an env
  override); add a config version field with a migration hook.

### 19. Config validation has wide gaps and reports only the first issue

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxline-core/src/config.rs` — `validate()` (~563–714)
- **Status:** confirmed
- **Description:** Unvalidated: `cleanup.provider` (any string passes when enabled, except
  the `"none"` check), `secrets.mode`, `daemon.log_level`, `audio.backend`,
  `asr.backend`, `asr.gpu_backend`, `asr.threads`, gate numeric sanity,
  `cleanup.temperature`/`timeout_ms` ranges, and `asr.model_path` existence (the preview
  model path *is* checked when preview is enabled). Layout/style issue collectors return
  `Vec`s, but callers take only `.into_iter().next()`, so users fix problems one at a
  time.
- **Risk or impact:** Typos surface as runtime failures far from the config; iterative
  validate-fix loops are slow.
- **Remediation:** Add allowlists/range checks (or typed enums) for the fields above; warn
  (not hard-fail) on missing ASR model; aggregate all validation issues into one report.

### 20. One malformed TOML file breaks all styles/apps/snippets listings

- **Severity:** medium
- **Category:** maintainability
- **Location:** `crates/voxline-core/src/styles.rs` — `list_styles`;
  `crates/voxline-core/src/apps.rs` — `list_app_profiles`;
  `crates/voxline-core/src/snippets.rs` — `list_snippets`
- **Status:** confirmed
- **Description:** Each listing loop propagates per-file read/parse errors with `?`,
  aborting the entire listing. The voice-command registry and validation flows build on
  these listings.
- **Risk or impact:** A single corrupt user file disables whole subsystems (list commands,
  validation, voice-command routing) rather than being skipped with a warning.
- **Remediation:** Collect per-file failures as `ValidationIssue`s and return the
  successful entries, matching the existing `validate_installed_*` pattern.

### 21. Style `prompt_file` is joined into the styles dir without filename validation

- **Severity:** medium
- **Category:** security / consistency
- **Location:** `crates/voxline-core/src/styles.rs` — `load_style_prompt` (~221),
  `prompt_path_for_style` (~261)
- **Status:** confirmed
- **Description:** Snippets validate `content_file`/`template_file` against `/`, `\`, and
  `..`; styles join `metadata.prompt_file` directly, so `prompt_file = "../../x"` reads
  outside the styles directory. Exploitation requires writing the user's own config dir
  (already a high-trust location), so this is primarily an inconsistency and
  defense-in-depth gap rather than a standalone vulnerability.
- **Remediation:** Reuse the snippet filename validation for `prompt_file`.

### 22. CLI reimplements the IPC client and `watch` mishandles the subscribe response

- **Severity:** medium
- **Category:** maintainability / correctness
- **Location:** `crates/voxline-cli/src/main.rs` — `send`, `watch` (~551–624),
  `write_request`; `crates/voxline-core/src/client.rs`
- **Status:** confirmed
- **Description:** `voxline-core::client` implements connect/subscribe correctly (read
  `Response`, then stream events) and the overlay uses it. The CLI duplicates the logic;
  `watch` deserializes every line as `Event`, so the subscribe ack `Response` fails to
  parse and is dumped raw to stdout, and `response.ok` is never checked.
- **Risk or impact:** Garbage first line in `watch` output; a rejected subscription would
  go unnoticed; duplicated protocol code drifts.
- **Remediation:** Use `client::subscribe()` in `watch` and route `send` through shared
  client helpers.

### 23. Doctor coverage falls short of plan section 25

- **Severity:** medium
- **Category:** maintainability
- **Location:** `crates/voxline-cli/src/main.rs` — `build_doctor_report`, `print_doctor`,
  `doctor` (~741–753)
- **Status:** confirmed
- **Description:** Doctor checks config/socket/secrets/paste/privacy, but: no CPAL input
  device or sample-format probe (plan 25.4), no live model-load/transcribe probe — only
  `model_exists` (plan 25.5), CLI-side `WAYLAND_DISPLAY`/`DISPLAY`/`DBUS` presence is in
  JSON but not human output (plan 25.1), `--json` exits 0 even when
  `config_valid = false` (human mode bails), and tool listings do not explain
  session relevance (plan 25.7).
- **Risk or impact:** "Doctor passed" does not establish that the microphone or ASR path
  works; JSON automation treats broken configs as healthy.
- **Remediation:** Add an audio probe and a daemon-backed `AsrStatus`/load check, make
  JSON mode exit non-zero on failures, and print CLI env detail in human mode.

### 24. Setup never starts (or restarts) the daemon on the newly written config

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxline-cli/src/setup_cmd.rs` — `ensure_daemon`,
  `DaemonGuard::drop` (~268–301), end of `run` (~198–235);
  `crates/voxline-cli/src/service.rs` — `install` (enables, does not start)
- **Status:** confirmed
- **Description:** If setup spawned a temporary `voxlined --foreground` for benchmarking,
  `DaemonGuard` kills it on exit (reasonable — it ran with old config). But the service
  install step only `enable`s the unit and prints start instructions, and if a daemon was
  already running it keeps serving the *old* ASR settings with no restart or warning.
- **Risk or impact:** Immediately after a "Setup complete." message, dictation either
  fails (no daemon) or silently uses the previous model/lifecycle configuration.
- **Remediation:** Offer to `systemctl --user restart voxlined` (or print an explicit
  "restart required" warning) whenever setup wrote a config and a daemon is running or
  the service was installed.

### 25. Stale per-job overrides after failed start; config re-read repeatedly mid-job

- **Severity:** medium
- **Category:** correctness / performance
- **Location:** `crates/voxlined/src/main.rs` — `start` (~742–791),
  `audio_error_response` (~1972–1979), `reload_job_config` (~382–408)
- **Status:** confirmed
- **Description:** `cleanup_override`, `style_override`, and `target_at_start` are stored
  before `audio.start()`; if audio fails, the error path resets state to Idle but leaves
  the overrides set, so they apply to the *next* dictation. Separately, the pipeline calls
  `Config::load_or_default()` from disk multiple times per job (profile lookup, preview
  seconds, cleanup paths), silently falling back to defaults on parse errors mid-job.
- **Risk or impact:** A failed start can cause the next job to use the wrong style or
  cleanup mode; disk reads on the stop-to-clipboard path add latency and can diverge from
  the config the daemon validated at startup.
- **Remediation:** Clear overrides on every start-failure path; snapshot job-relevant
  config once at job start (or daemon start with explicit reload).

### 26. Benchmark reporting inaccuracies

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxlined/src/main.rs` — `bench_model_compare` (~541–579),
  `bench_dictation` (~411–454); `crates/voxlined/src/cleanup.rs` —
  `failed_fallback_outcome` (~87–93)
- **Status:** confirmed
- **Description:** `cold_load_ms` always reflects `reload()` latency even when
  `include_cold_load` is false; `bench_dictation` checks Idle once and is subject to the
  finding-4 race; cleanup failures report `cleanup_ms: 0`, hiding timeout latency from the
  end-to-end totals the plan (section 6.4) requires.
- **Risk or impact:** Model-selection decisions in setup and latency-target verification
  rest on misleading numbers.
- **Remediation:** Report load/reload semantics explicitly, hold the job slot during
  benches, and record elapsed cleanup time on failure.

### 27. Preview keeps a second Whisper context warm alongside the final model

- **Severity:** medium
- **Category:** performance
- **Location:** `crates/voxlined/src/preview_asr.rs` (~20–34);
  `crates/voxline-core/src/config.rs` — `PreviewConfig::to_asr_config` (forces
  `keep_warm`)
- **Status:** confirmed (code); needs hardware/GPU validation for actual pressure
- **Description:** With preview enabled, a dedicated preview engine loads its own model
  with forced `keep_warm`, so two GGML contexts can occupy GPU/RAM simultaneously with the
  power-user large-v3-turbo final model.
- **Risk or impact:** VRAM pressure on 8 GB-class GPUs (the stated initial target) can slow
  or fail final transcription.
- **Remediation:** Document the memory budget; consider unloading the preview model at
  stop before final transcription, or sharing a single worker with priority.

### 28. `auto_paste = "always"` skips staleness and target-stability checks

- **Severity:** medium
- **Category:** security
- **Location:** `crates/voxlined/src/injection.rs` — `check_paste_safety` (~103–110)
- **Status:** confirmed (arguably by design)
- **Description:** In `Always` mode only the session/terminal guards run;
  `max_paste_age_ms` and the triple-target check are bypassed. The injection docs describe
  this loosely ("without the full safe-target check"), and the plan defines the mode but
  also describes the paste pipeline's target checks unconditionally (section 19.1).
- **Risk or impact:** Text pastes into whatever is focused, however much later — the exact
  failure the product narrative centers on avoiding. It is opt-in, which bounds severity.
- **Remediation:** Keep the staleness check even in `Always` (a minutes-old paste is rarely
  intended), and have doctor warn when `always` is configured.

### 29. `voxline transcribe --no-cleanup` is a no-op

- **Severity:** medium
- **Category:** correctness
- **Location:** `crates/voxline-cli/src/main.rs` — `Commands::Transcribe` (~337–344,
  `no_cleanup: _`); `crates/voxline-core/src/protocol.rs` — `Command::Transcribe`
- **Status:** confirmed
- **Description:** The flag is accepted and discarded; the protocol command has no cleanup
  field. The transcribe path never runs cleanup today, so behavior happens to be what the
  flag implies — but the flag advertises control that does not exist, and the justfile and
  plan reference it.
- **Risk or impact:** If cleanup is ever added to the transcribe path, the flag silently
  fails open. Misleading UX today.
- **Remediation:** Remove the flag or wire it through the protocol.

### 30. `just install` overwrites a CUDA `voxlined` with a CPU build

- **Severity:** medium
- **Category:** build / documentation
- **Location:** `justfile` — `install` (depends on `release`, not `release-cuda`);
  `README.md` and `docs/src/content/docs/index.mdx` quick start
- **Status:** confirmed
- **Description:** Docs recommend `just release-cuda` then `just install`, but `install`
  triggers a fresh default-features (CPU) `cargo build --release`, replacing the CUDA
  binary that was just built.
- **Risk or impact:** GPU users silently end up on a CPU build; `asr.gpu = true` then
  fails at model load with `UnsupportedFeature`.
- **Remediation:** Add `install-cuda` (depending on `release-cuda`) or make `install`
  accept the build flavor; update the README sequence.

### 31. Five linked docs pages do not exist

- **Severity:** medium
- **Category:** documentation
- **Location:** `docs/astro.config.ts` sidebar; `docs/src/content/docs/index.mdx`;
  missing: `install`, `setup`, `cli`, `service`, `troubleshooting`
  (note: files exist for some titles but `guides/` and `reference/` directories are empty;
  several sidebar slugs resolve to nothing)
- **Status:** confirmed
- **Description:** The Starlight sidebar and intro page link to pages that are absent,
  producing broken navigation if the site is built/deployed as-is.
- **Risk or impact:** Broken first-run experience for the documented site; install/setup
  guidance only exists in `README.md`.
- **Remediation:** Add the missing pages or trim the sidebar until they exist. (Docs
  deployment itself is deferred, so this blocks nothing today.)

### 32. Overlay spawns a placement subprocess every 50 ms on the GTK main thread

- **Severity:** medium
- **Category:** performance
- **Location:** `crates/voxline-overlay/src/main.rs` — `glib::timeout_add_local`
  (~208–238); `crates/voxline-platform/src/lib.rs` — `capture_overlay_placement_hint`
- **Status:** confirmed
- **Description:** While recording with cursor placement enabled, the overlay calls
  `hyprctl`/`xdotool` synchronously on the UI thread 20 times per second.
- **Risk or impact:** UI jank and constant subprocess churn during dictation — the exact
  window where the system is already busiest.
- **Remediation:** Throttle to a few hundred ms, cache the last hint, or move capture to a
  worker thread feeding the UI via a channel.

### 33. `voxline secrets set` reads the API key without masking

- **Severity:** medium
- **Category:** security
- **Location:** `crates/voxline-cli/src/secrets_cmd.rs` — `set_secret` (~28–32)
- **Status:** confirmed
- **Description:** The OpenRouter key is read via plain `stdin().read_line`, echoing it to
  the terminal and leaving it in scrollback.
- **Risk or impact:** Shoulder-surfing and terminal-logging exposure of a paid API
  credential.
- **Remediation:** Use `dialoguer::Password` (already a dependency) for hidden input.

## Low findings and nitpicks

### 34. Failed temp-audio deletion logs the WAV path

- **Severity:** low
- **Category:** security / privacy
- **Location:** `crates/voxlined/src/main.rs` — `TemporaryAudio::drop` (~1900–1909)
- **Status:** confirmed
- **Description:** On deletion failure the path is logged at `warn`; plan section 28
  forbids audio paths in default logs after deletion.
- **Remediation:** Log job id and error kind only.

### 35. Same-user IPC can transcribe arbitrary readable files and spend cleanup credits

- **Severity:** low (accepted v1 risk per plan section 10.2)
- **Category:** security
- **Location:** `crates/voxlined/src/main.rs` — `transcribe`, `test_openrouter`,
  `cleanup_preview`, `template_preview`; no `SO_PEERCRED` check on connections
- **Status:** confirmed
- **Description:** Any same-user client can point `Command::Transcribe` at any readable
  WAV and receive the text, or invoke OpenRouter-spending commands. The plan accepts this
  class of risk; peer-UID checking ("where available") is unimplemented defense-in-depth.
- **Remediation:** Optional: verify peer UID on accept; rate-limit cloud-spend commands.

### 36. Transcript text surfaces in benches, setup tables, and `watch` output

- **Severity:** low
- **Category:** privacy
- **Location:** `crates/voxline-core/src/protocol.rs` — `ModelBenchResult.transcript_text`;
  `crates/voxline-cli/src/setup_cmd.rs` — `print_bench_table` (48-char snippet);
  `crates/voxline-cli/src/main.rs` — `watch` prints `result.transcript.text`
- **Status:** confirmed
- **Description:** Reasonable UX choices individually, but together they normalize
  transcript text appearing in scrollback/CI logs. Follows from finding 1 for `watch`.
- **Remediation:** Truncate/gate behind flags once finding 1's redaction lands.

### 37. Dead/incorrect job-state surface: `Failed` never used, `cancel` code misleading

- **Severity:** low
- **Category:** maintainability
- **Location:** `crates/voxline-core/src/protocol.rs` — `JobState::Failed`;
  `crates/voxlined/src/main.rs` — `cancel` (~1913–1924)
- **Status:** confirmed
- **Description:** Errors reset to `Idle`; `Failed` is never set. `cancel` during
  Transcribing/Cleaning returns `no_active_recording` instead of the plan's
  `cannot_cancel`.
- **Remediation:** Implement the plan's cancel matrix; use or remove `Failed`.

### 38. Protocol error codes are free-form strings; clients never check `protocol_version`

- **Severity:** low
- **Category:** maintainability
- **Location:** `crates/voxline-core/src/protocol.rs` — `ProtocolError`;
  `crates/voxline-core/src/client.rs`
- **Status:** confirmed
- **Description:** The daemon validates request versions, but responses/events are
  consumed without version checks, and error codes (`busy`, `asr_error`, ...) have no
  typed registry.
- **Remediation:** Add a code enum/const list and a version check helper in `client.rs`.

### 39. Unknown config/protocol fields are silently ignored

- **Severity:** low
- **Category:** maintainability
- **Location:** config and protocol structs (no `deny_unknown_fields`)
- **Status:** confirmed
- **Description:** `enabeld = true` style typos vanish silently.
- **Remediation:** `deny_unknown_fields` on `Config` (at least), with a clear parse error.

### 40. Path-handling duplication and `$HOME` non-expansion

- **Severity:** low
- **Category:** maintainability
- **Location:** `crates/voxline-core/src/paths.rs` — `expand_home`;
  `setup.rs` — `path_to_tilde`; `models.rs` — `tilde_model_path`
- **Status:** confirmed
- **Description:** Only `~/` is expanded (`$HOME/...` is literal); three modules carry
  their own tilde formatting helpers.
- **Remediation:** Centralize in `paths.rs`; document supported forms.

### 41. System probe shells out redundantly and fragilely

- **Severity:** low
- **Category:** maintainability
- **Location:** `crates/voxline-core/src/system_probe.rs` — `probe_system` (3×
  `nvidia_gpu_info()` / 3× `nvidia-smi` spawns), `free_space_mib` (parses `df -Pm`)
- **Status:** confirmed
- **Remediation:** Bind the GPU info once; use `statvfs` instead of parsing `df`.

### 42. OpenRouter client rebuilt per request; no retry

- **Severity:** low
- **Category:** performance
- **Location:** `crates/voxlined/src/openrouter.rs` — `complete_chat` (~25–28)
- **Status:** confirmed
- **Description:** A fresh `reqwest::Client` (new TLS handshake, no pooling) per cleanup
  call; transient 429/5xx immediately falls back to raw.
- **Remediation:** Share a client in `AppState`; consider one bounded retry within the
  timeout budget.

### 43. Unbounded worker queues and a fixed 20 ms stop sleep

- **Severity:** low
- **Category:** performance
- **Location:** `crates/voxlined/src/asr.rs`, `audio.rs`, `preview_asr.rs` — unbounded
  `mpsc::channel()`; `audio.rs` — `finish_recording` `thread::sleep(20ms)` (~318)
- **Status:** confirmed
- **Description:** Command queues have no backpressure (low risk with busy gating); every
  stop pays a blind 20 ms drain sleep inside the per-stage <100 ms finalize budget.
- **Remediation:** Bounded channels with `try_send`→busy; replace the sleep with a
  callback-drain signal.

### 44. Hyprland paste diverges from plan/docs and skips the terminal guard

- **Severity:** low
- **Category:** correctness / documentation
- **Location:** `crates/voxline-platform/src/lib.rs` — `paste` (Hyprland uses
  `hyprctl dispatch sendshortcut SHIFT,Insert,activewindow`);
  `crates/voxlined/src/injection.rs` — terminal check exempts `PasteBackend::Hyprland`
  (~96–101)
- **Status:** confirmed (likely deliberate: Shift+Insert works in most terminals)
- **Description:** Plan and docs describe wtype Ctrl+V for Hyprland; the implementation
  uses a Hyprland-specific Shift+Insert shortcut and consequently allows pasting into
  terminals on Hyprland only. The rationale is sound but undocumented, and Shift+Insert
  pastes the primary selection rather than the clipboard in some X11-protocol apps.
- **Remediation:** Document the Hyprland behavior and its terminal exemption; verify
  Shift+Insert semantics across common apps (needs desktop-session validation).

### 45. Vocabulary UX inconsistencies

- **Severity:** low
- **Category:** correctness / documentation
- **Location:** `crates/voxline-cli/src/main.rs` — `vocab` (saves without `validate()`,
  `vocab test` uses plain `str::replace`); `crates/voxlined/src/asr.rs` — whole-word
  `replace_whole_words`; vocabulary is captured at `AsrManager::spawn` and not refreshed
  by `reload()`
- **Status:** confirmed
- **Description:** `vocab test` results can differ from real transcription; `vocab add`
  takes effect only after daemon restart, while the docs imply otherwise.
- **Remediation:** Share the replacement function in core; document or implement hot
  reload.

### 46. Overlay polish issues

- **Severity:** low
- **Category:** correctness / privacy
- **Location:** `crates/voxline-overlay/src/main.rs` — `apply_event` (~291–318);
  `crates/voxline-platform/src/lib.rs` — `capture_x11_placement` (~520–544)
- **Status:** confirmed
- **Description:** Stable preview text lingers on screen after `Event::Result` until an
  Idle state arrives; X11 placement assumes a single monitor at origin (0,0).
- **Remediation:** Clear stable text on terminal states; use per-monitor geometry.

### 47. Miscellaneous drift and nits

- **Severity:** low
- **Category:** maintainability / documentation
- **Status:** confirmed
- **Description (grouped):**
  - Plan section 23 lists `voxline test asr`, `voxline test app-detect`, and
    `bench end-to-end --cleanup/--no-cleanup`; none exist (closest: `apps detect`,
    `bench dictation --cleanup`).
  - Docs CLI syntax errors: `secrets status openrouter` (no provider arg),
    `vocab add replace --from/--to` (positional in reality).
  - `privacy.md` documents the setup fixture at `~/.local/share/voxline/samples/setup.wav`;
    code writes `<model_dir>/samples/setup.wav`.
  - Interrupted downloads leave `.part` files; nothing cleans or reports them.
  - `print_response` uses `expect("response serializes")` — a panic on schema drift.
  - `bench dictation` accepts `--cleanup --no-cleanup` together (cleanup silently wins);
    `cleanup_override()` validates this elsewhere.
  - `crates/voxlined/Cargo.toml` re-declares `reqwest` with different features instead of
    `reqwest.workspace = true`.
  - `DEFAULT_OPENROUTER_MODEL = "~openai/gpt-mini-latest"` uses `~` as an OpenRouter
    routing prefix; easily confused with path tilde-expansion conventions elsewhere.
  - `wmctrl` is probed by doctor but never used by any backend.
  - `apply_profile("power-user-nvidia")` resets nearly the whole config (preserving only
    secrets/cleanup); docs describe it as an ASR profile switch.
- **Remediation:** Batch these into a docs/CLI cleanup pass.

## Maintainability themes

- **Module size and layering.** `voxlined/src/main.rs` (~2,130 lines) and
  `voxline-cli/src/main.rs` (~1,225 lines) are the two god modules. Core is well factored
  by contrast. The daemon needs `ipc`/`jobs`/`delivery`/`bench` extraction before the next
  feature lands (findings 4, 17).
- **Testing strategy.** Core has good unit coverage (routing, commands, templates, config
  defaults, paste-safety logic). The daemon's orchestration layer, the preview pipeline,
  the OpenRouter client, `core::client`, and `core::download` have zero tests. There are
  no CLI↔daemon integration tests (plan section 27.2) — the highest-value gap given
  findings 4 and 22.
- **Duplication.** Styles/apps/snippets repeat loader/validator/editor boilerplate with
  diverging behavior (findings 20, 21); the CLI re-implements the IPC client that core
  already provides (finding 22); tilde/path helpers exist in triplicate (finding 40);
  `find_voxlined` exists in both `setup_cmd.rs` and `service.rs`.
- **Error-type discipline** is good (typed `thiserror` per module, `anyhow` at binary
  edges) but protocol error codes are stringly-typed and `core::client` leaks `anyhow`
  (findings 38, 35-core).
- **Config lifecycle** is the weakest core area: no validation on load, no schema version,
  hardcoded config path, repeated mid-job disk reloads (findings 18, 19, 25).

## Performance themes

- **Audio capture is the hot-path risk cluster:** unbounded buffer growth, full-history
  preview reprocessing under the callback mutex, linear-interpolation resampling, and a
  fixed stop-drain sleep (findings 2, 3, 14, 43). These directly threaten the plan's
  stop-to-clipboard latency budget for long utterances.
- **ASR lifecycle is implemented correctly** (blocking worker thread, keep_warm with idle
  unload, on_demand unload), but preview adds a second always-warm context that the 8 GB
  GPU target may not absorb (finding 27).
- **The async runtime is blocked** by clipboard/paste subprocesses and sleeps during the
  inject phase (finding 15), and event fan-out duplicates preview traffic through a small
  broadcast buffer whose lag policy disconnects clients (finding 16).
- **Benchmarks need accuracy fixes** (cold-load semantics, cleanup-failure timing, race
  exposure) before they can validate the plan's latency targets (finding 26). Actual
  latency/WER verification requires hardware and is out of scope here.

## Documentation and plan drift

- **Docs vs code (config):** `config-example/linux/config.toml` matches `config.rs`
  defaults faithfully — the strongest doc artifact, backed by a test. The configuration
  pages are accurate with three exceptions: `[injection.linux]` paste-command keys are
  presented as functional but are validated-only dead config (paste backends are
  hardcoded in `voxline-platform`); `max_record_seconds` documents nonexistent behavior
  (finding 2); vocabulary docs say "whole-phrase" and imply hot reload (finding 45).
- **Docs vs code (CLI):** wrong syntax for `secrets status` and `vocab add replace`;
  `transcribe --no-cleanup` is a no-op; `cleanup disable` exists but is undocumented;
  five linked pages missing (finding 31).
- **Plan vs code:** the plan trails the implementation in scope (preview, overlay, voice
  commands, routing, setup all shipped beyond plan section 24's example) but leads it in
  several promises: doctor audio/ASR live probes (25.4–25.5), `test asr`/`test
  app-detect`, `bench end-to-end --cleanup`, the `[debug]` retention section (11.6/28),
  rubato resampling, and the no-transcript-in-events rule (10.6) — the last being
  finding 1.
- **README:** paste-safety and service descriptions match the implementation; the CUDA
  install sequence is broken by `just install` rebuilding CPU (finding 30).

## Positive patterns worth preserving

- Privacy-safe defaults are real and tested: cleanup off, all `[privacy]` storage/logging
  flags false, `auto_paste = "safe"`, and an example-config-validates test tying
  `config-example` to the schema.
- Secrets chain matches the plan: keyring → env → opt-in file with enforced 0600, no
  silent file creation.
- Runtime hygiene: 0700 runtime dir / 0600 socket enforced and verified; WAVs live under
  the runtime dir and are deleted via the `TemporaryAudio` RAII guard; refusal to run
  without `XDG_RUNTIME_DIR` unless explicitly configured.
- Thread architecture: audio owner thread and ASR worker threads communicating over
  channels keep CPAL stream ownership and Whisper blocking work off the Tokio runtime —
  exactly the plan's design.
- No shell interpolation anywhere on untrusted data: clipboard text goes via stdin; paste
  and detection tools use fixed argv arrays; `command_exists` passes the probe name as a
  positional `"$1"` parameter.
- Workspace-wide `unsafe_code = "forbid"`, clippy pedantic at `-D warnings`, rustls-only
  reqwest.
- Paste safety decomposed into pure, well-tested functions (`injection.rs`); fail-closed
  to clipboard-only.
- Overlay reconnects with backoff, escapes markup in preview text, and never logs
  transcripts.
- `voxline-core` routing/styles/commands/template modules have focused unit tests and
  typed errors; the voice-command parser follows the plan's conservative start-only,
  prefix-gated design.

## Suggested remediation roadmap

### Phase 1: Quick wins

- Redact transcript text from `Event::Result` by default (finding 1).
- Enforce `max_record_seconds` in the capture callback (finding 2).
- Surface audio stream errors to the job layer (finding 5).
- Require `--force` for non-interactive setup over an existing config (finding 6);
  print/offer a daemon restart at the end of setup (finding 24).
- Doctor: show insecure-file fallback, non-zero `--json` exit on invalid config
  (findings 8, 23 partial).
- Fix `watch` to use `core::client::subscribe` (finding 22); mask `secrets set` input
  (finding 33); clear stale overrides on failed start (finding 25 partial).
- Add `install-cuda` to the justfile and fix the README sequence (finding 30).
- Docs corrections batch: CLI syntax, fixture path, `[injection.linux]` status, missing
  pages or trimmed sidebar (findings 31, 47).

### Phase 2: Structural fixes

- Make job-entry transitions atomic and extract the job state machine into a testable
  module with busy-matrix tests (findings 4, 17).
- Ring-buffer the preview tap; move resampling off the callback mutex (finding 3).
- Validate on config load, aggregate validation issues, close the enum/range gaps, add a
  config schema version (findings 18, 19).
- Per-file error tolerance in styles/apps/snippets listings; share loader helpers; apply
  snippet filename validation to style `prompt_file` (findings 20, 21).
- SHA-256 verification for model downloads (finding 7).
- Runtime dir UID ownership check (finding 10).
- Stability comparison on `(backend, id, app_id)`; fix X11 `app_id` to WM_CLASS
  (findings 11, 12).
- Split preview off the broadcast channel; soften the lag policy (finding 16).
- `spawn_blocking` for clipboard/paste subprocess work (finding 15).
- Doctor audio + live-ASR probes (finding 23).

### Phase 3: Optional performance work

- Replace linear resampling with rubato and A/B the WER impact (finding 14) — needs
  hardware validation.
- Preview/final model memory budgeting on 8 GB GPUs (finding 27) — needs GPU validation.
- Shared OpenRouter client with bounded retry (finding 42).
- Benchmark accuracy fixes and a recorded latency report against plan section 6 targets
  (finding 26) — needs hardware validation.
- Near-exact hallucination matching with curated patterns (finding 13).

## Out of scope and deferred

- macOS and Windows ports (plan sections 29–30)
- docs site deployment (deferred per repo layout notes)
- Git releases
- hardware-only validation: microphone capture quality, GPU/CUDA behavior, real paste
  testing across the desktop matrix, latency/WER benchmarks, OpenRouter live calls
- any production code refactor in this pass — this document is the only change
