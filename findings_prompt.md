# VoxLine code smell and architecture review prompt

You are reviewing the VoxLine repository: a Linux-first, local-first dictation app written in Rust.

Your job is **read-only analysis**. Do **not** refactor, rewrite, or fix production code in this pass. Produce a structured findings document that will be used later to prioritize fixes.

## Deliverable

1. Check out the latest `main` branch, or merge/rebase if needed.
2. Create a branch named `code-review`.
3. Write `findings.md` at the repository root: `/findings.md`.
4. Commit only `findings.md` on `code-review` with this commit message:

   ```text
   Add code review findings document
   ```

5. Do **not** open a pull request unless explicitly asked.
6. Do **not** deploy docs or create Git releases.

## Review focus

Prioritize issues in these categories.

### Security

Review secrets handling, file permissions, IPC/socket trust, path traversal, unsafe subprocess/shell usage, network downloads, TLS, URL validation, integrity checks, cloud cleanup data leakage, clipboard/paste safety, logging of sensitive data, and keyring vs. file fallback behavior.

### Maintainability

Review crate boundaries, module size and complexity, duplication, error handling consistency, test gaps, config/protocol versioning, unclear abstractions, and docs vs. code drift.

### Performance

Review hot paths such as audio capture, resampling, ASR, preview generation, unnecessary clones or locks, blocking I/O on the async runtime, model lifecycle, memory growth in buffers or event broadcasts, and benchmark accuracy.

## Finding requirements

For each finding, include:

- **Severity:** `critical`, `high`, `medium`, or `low`
- **Category:** `security`, `maintainability`, `performance`, `documentation`, or another clearly named category
- **Location:** file path plus function, symbol, or area when known
- **Description:** what is wrong or concerning
- **Risk or impact:** why it matters
- **Evidence:** what you observed in the code, docs, tests, or runtime behavior
- **Concrete remediation suggestion:** what should change later

Distinguish confirmed bugs from speculative concerns. When uncertain, say what evidence would confirm or refute the finding.

When a code snippet is the clearest way to explain a problem or proposed remediation, include a concise snippet. Snippets should be illustrative and scoped to the finding. Do not use snippets as a substitute for explaining the issue, and do not modify repository files other than `findings.md`.

## Project context

VoxLine records microphone audio, transcribes locally with Whisper/GGML, optionally runs opt-in cloud text cleanup through OpenRouter, copies results to the clipboard, and pastes only when the active target is verified stable.

Privacy-safe defaults are a core product requirement. No audio should leave the machine unless cleanup is explicitly enabled, and cloud cleanup should send transcript text only.

Read these documents before judging implementation details:

- `AGENTS.md`
- `VoxLine_implementation_plan.md`

Treat the implementation plan as the source of truth when deciding whether something is a bug, an intentional deferral, or an acceptable v1 tradeoff.

## Repository layout

```text
/home/gstrand/github/voxline/                 # workspace root
├── Cargo.toml                               # workspace; unsafe_code forbidden workspace-wide
├── justfile                                 # dev recipes: check, release, install, bench, docs-*
├── findings.md                              # YOU CREATE THIS
├── AGENTS.md
├── VoxLine_implementation_plan.md
├── README.md
├── config-example/
│   └── linux/
│       └── config.toml                      # reference config with all sections
├── docs/                                    # Astro Starlight docs site; Bun; deploy deferred
│   ├── astro.config.ts
│   ├── src/
│   │   └── content/
│   │       └── docs/                        # user-facing docs, including configuration/*
│   └── wrangler.toml
└── crates/
    ├── voxline-core/                        # shared library: config, protocol, paths, secrets, setup
    ├── voxline-cli/                         # voxline binary; clap CLI
    ├── voxlined/                            # daemon: audio, ASR, IPC, injection, cleanup
    ├── voxline-platform/                    # Linux desktop/session/target detection
    └── voxline-overlay/                     # optional preview overlay UI
```

## Crate responsibilities

| Crate | Binary | Role |
| --- | --- | --- |
| `voxline-core` | library | Config load/validate, NDJSON IPC protocol, Unix socket paths, secrets/keyring, systemd unit rendering, routing for apps/styles/snippets/commands, setup wizard logic, Hugging Face model catalog and HTTP download, system probe, privacy/runtime helpers |
| `voxlined` | `voxlined` | Tokio daemon: CPAL audio, Whisper ASR, preview ASR worker, job pipeline, OpenRouter cleanup, safe paste injection, benchmark commands |
| `voxline-cli` | `voxline` | User-facing CLI: toggle/start/stop, doctor, config, service, setup wizard UI, vocab/styles/apps/snippets/cleanup subcommands |
| `voxline-platform` | library | Wayland/X11 target detection, terminal/app ID heuristics, trigger guidance |
| `voxline-overlay` | `voxline-overlay` | Cursor-aware preview overlay |

## Key modules to review carefully

### Security-sensitive modules

- `crates/voxline-core/src/secrets.rs` — keyring, environment variable, insecure file fallback, `0600` enforcement
- `crates/voxline-core/src/download.rs` — Hugging Face model downloads, resume behavior, size checks
- `crates/voxline-core/src/runtime.rs` — runtime directory, socket `0600`
- `crates/voxline-core/src/protocol.rs` — v1 NDJSON IPC command/event schema
- `crates/voxline-core/src/client.rs` — CLI ↔ daemon client
- `crates/voxlined/src/injection.rs` — clipboard read/write/restore, paste safety triple-check
- `crates/voxlined/src/openrouter.rs` — outbound transcript-only HTTP
- `crates/voxlined/src/template_extract.rs` — structured LLM JSON for snippet templates
- `crates/voxline-platform/src/lib.rs` — focus/target detection, which paste safety depends on

### Large or complex modules

- `crates/voxlined/src/main.rs` — daemon event loop, command dispatch, job orchestration; approximately 2,000+ lines
- `crates/voxline-cli/src/main.rs` — CLI entry point, doctor, bench, many subcommands
- `crates/voxline-cli/src/setup_cmd.rs` — interactive setup wizard
- `crates/voxline-core/src/config.rs` — full config schema and validation
- `crates/voxline-core/src/setup.rs`
- `crates/voxline-core/src/system_probe.rs`
- `crates/voxline-core/src/models.rs`

### Performance-sensitive modules

- `crates/voxlined/src/audio.rs` — capture, resampling, gates, setup recording
- `crates/voxlined/src/asr.rs` — Whisper backend, hallucination filter, reload
- `crates/voxlined/src/preview.rs` — real-time preview path
- `crates/voxlined/src/preview_asr.rs` — preview ASR worker
- `crates/voxline-core/src/preview.rs` — preview text agreement logic

### Supporting features to review for consistency

- `crates/voxline-core/src/cleanup.rs`
- `crates/voxline-core/src/routing.rs`
- `crates/voxline-core/src/apps.rs`
- `crates/voxline-core/src/styles.rs`
- `crates/voxline-core/src/snippets.rs`
- `crates/voxline-core/src/snippet_templates.rs`
- `crates/voxline-core/src/commands.rs`
- `crates/voxline-core/src/service.rs`
- `crates/voxline-cli/src/service.rs`
- `crates/voxline-cli/src/*_cmd.rs`

## Main features implemented in v1

Use this checklist to ensure coverage. Flag gaps between docs, the implementation plan, and code.

1. **Daemon + IPC** — Unix socket, NDJSON protocol v1, job states, event broadcast through `watch`
2. **Audio** — CPAL capture, mono 16 kHz, energy/length gates, WAV retention under runtime directory
3. **ASR** — local Whisper/GGML, model lifecycle through `keep_warm` or `on_demand`, hallucination filter, vocabulary biasing/replacements, ASR reload for setup benchmarks
4. **Dictation workflow** — toggle/start/stop/cancel, transcribe path, clipboard copy, safe paste with target verification
5. **Preview** — optional real-time preview ASR and overlay through `voxline-overlay`
6. **Cleanup** — opt-in OpenRouter cleanup, styles from `~/.config/voxline/styles/`, voice-command style routing
7. **Routing** — app profiles, per-app cleanup/paste overrides, insert/template snippets, voice commands
8. **Secrets** — keyring-first OpenRouter API key, with doctor surfacing status
9. **Service** — systemd user unit install/start/stop
10. **Setup wizard** — `voxline setup`: probe dependencies, record fixture, download models, benchmark comparison, write config, optional service install; `just install` runs `setup --if-missing`
11. **Release hardening** — `just release`, benchmark recipes, doctor hints, privacy-gated transcript logs
12. **Docs** — Starlight site in `docs/`; content should align with `config.rs` and `config-example/linux/config.toml`

## Suggested review method

1. Skim `VoxLine_implementation_plan.md`, especially these sections:
   - Section 4: architecture
   - Section 10: IPC
   - Section 16: cleanup
   - Section 19: paste
   - Section 21: secrets
   - Section 28: logging/privacy
2. Map the data flow:
   - microphone → WAV → ASR → optional cleanup → clipboard → paste decision
3. Trace trust boundaries:
   - local socket: who can connect?
   - filesystem paths: tilde expansion, runtime directory, config paths, model paths
   - network: cleanup and model download only
   - secrets: keyring, environment variables, file fallback
4. Run `just check` once for baseline.
5. Note clippy/test blind spots, especially integration and security edges.
6. Compare `docs/src/content/docs/configuration/*` and `README.md` against `config.rs` for drift.
7. Look for god modules, duplicated protocol handling, and `unwrap`/`expect` on user-controlled input.

## Required `findings.md` structure

Use this outline:

````markdown
# VoxLine code review findings

**Branch reviewed:** main @ <short sha>  
**Review date:** <date>  
**Scope:** security, maintainability, performance

## Executive summary

- <Top risk or recommendation>
- <Top risk or recommendation>
- <Top risk or recommendation>

## Statistics

- Files reviewed: <approximate count>
- Findings by severity:
  - Critical: <count>
  - High: <count>
  - Medium: <count>
  - Low: <count>

## Critical and high findings

### 1. <Finding title>

- **Severity:** critical | high
- **Category:** security | maintainability | performance | documentation | other
- **Location:** `path/to/file.rs` — `<function or area>`
- **Status:** confirmed | speculative | needs hardware/GPU validation
- **Description:** <what is wrong or concerning>
- **Risk or impact:** <why it matters>
- **Evidence:** <what code, behavior, test result, or doc mismatch supports this>
- **Remediation:** <concrete suggestion>

```rust
// Optional: include a concise illustrative snippet only when it clarifies the issue
```

## Medium findings

### 2. <Finding title>

- **Severity:** medium
- **Category:** <category>
- **Location:** `<path>` — `<function or area>`
- **Status:** confirmed | speculative | needs validation
- **Description:** <description>
- **Risk or impact:** <impact>
- **Evidence:** <evidence>
- **Remediation:** <suggestion>

## Low findings and nitpicks

### 3. <Finding title>

- **Severity:** low
- **Category:** <category>
- **Location:** `<path>` — `<function or area>`
- **Status:** confirmed | speculative | needs validation
- **Description:** <description>
- **Risk or impact:** <impact>
- **Evidence:** <evidence>
- **Remediation:** <suggestion>

## Maintainability themes

Summarize cross-cutting maintainability issues, such as module size, testing strategy, error types, API boundaries, naming consistency, and duplication.

## Performance themes

Summarize cross-cutting performance concerns, such as ASR lifecycle, preview path behavior, allocations, blocking I/O, locks, and benchmark quality.

## Documentation and plan drift

Summarize mismatches between:

- `VoxLine_implementation_plan.md`
- `README.md`
- `config-example/linux/config.toml`
- `docs/src/content/docs/configuration/*`
- actual implementation in `config.rs` and related modules

## Positive patterns worth preserving

Briefly note design or implementation patterns that should not be broken during fixes.

## Suggested remediation roadmap

### Phase 1: Quick wins

- <fix>

### Phase 2: Structural fixes

- <fix>

### Phase 3: Optional performance work

- <fix>

## Out of scope and deferred

- macOS port
- docs deployment
- Git releases
- hardware-only validation
- any production code refactor in this pass
````

## Writing guidance

Write clearly and factually. Do not use emojis.

Cite paths like `crates/voxlined/src/main.rs`, and include a function, type, or area when known.

Prefer depth on security and maintainability over style nits.

If you cannot verify something without hardware, a GPU, a microphone, a running desktop session, or external credentials, say so explicitly.

If a concern is speculative, label it as speculative and explain what evidence would make it confirmed.

Avoid overstating risks. Focus on likely, actionable issues.

## Constraints

- Do not commit secrets.
- Do not commit `node_modules/`.
- Do not commit `target/`.
- Do not commit build artifacts.
- Do not change production code in this pass.
- Only create and commit `findings.md`.
- Do not open a pull request unless explicitly asked.
- Do not deploy docs.
- Do not create Git releases.
