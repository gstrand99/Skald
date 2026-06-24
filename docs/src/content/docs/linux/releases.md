---
title: Linux releases
description: Archive release, signing, validation, and rollback process.
---

Skald Linux releases are versioned archives built from a release tag. Archives
contain these binaries:

- `skald`
- `skaldd`
- `skald-overlay`
- `skald-tray`

Archives do not contain ASR model weights, API keys, user configuration, user
styles, snippets, app profiles, transcript text, audio files, or proprietary
CUDA libraries.

## Artifacts

Each release publishes:

- CPU-safe Linux archive
- CUDA Linux archive
- SHA-256 manifest for each archive
- detached GPG signatures for archives and manifests
- release notes
- source archive from GitHub Releases

The CUDA archive targets the current tested NVIDIA CUDA line. For the first
archive pipeline this is CUDA 13.3, with Linux NVIDIA driver `>=610.43.02` on
the release host. CUDA 13.x minor-version compatibility requires Linux driver
`>=580`. The exact driver, toolkit, Rust compiler, commit, tag, target triple,
and selected profile are recorded in `BUILD-METADATA.toml` inside each archive.

## Build

Release packaging must run from a clean worktree at a tag matching the workspace
version (`v0.1.0` for version `0.1.0`).

```bash
just release-archives
just release-smoke
just release-checksums
just release-sign
scripts/release-notes > dist/RELEASE_NOTES.md
```

For local packaging tests without a tag:

```bash
just release-archive-dry-run
```

## Validation

Print the release checklist:

```bash
just release-checklist
```

Run the checklist before publishing a release. It covers clean install, upgrade,
rollback, CPU and CUDA doctor output, service restart, microphone capture, final
transcription, clipboard and safe-paste fallback, overlay modes, tray behavior,
cleanup disabled by default, desktop matrix notes, and artifact privacy checks.

## CI

The release workflow runs on `v*` tags. It runs `just check`, builds the
CPU-safe archive, smoke-tests the extracted archive, generates checksums, signs
when the GPG release key is available, and creates a draft GitHub release.

CUDA publishing is a manual or dedicated-runner gate. The workflow has a CUDA
job for a self-hosted Linux CUDA runner and will not publish a CPU build under a
CUDA artifact name.

Package-manager formats are deferred until archive releases are reliable.
