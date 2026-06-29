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

For Linux 1.0, the [Desktop matrix](/linux/desktop-matrix/) records Hyprland
Wayland as the only validated session. Complete the checklist on that target
before tagging.

## Publish manually

Releases are built and published manually from a maintainer workstation. There is
no GitHub Actions release workflow.

Issue branches merge into `dev`. `main` is release-only: the normal path into
`main` is a pull request from `dev` to `main`, and every merge to `main` is
released.

1. Confirm `dev` with `just release-ready`.
2. Open and merge the release pull request from `dev` to `main`.
3. Tag `vX.Y.Z` on `main`, matching `workspace.package.version`.
4. Run `just release-archives`, `just release-smoke`, and `just release-checksums`.
5. Sign with `just release-sign` when GPG keys are available.
6. Run `just release-checklist` and complete manual validation.
7. Upload archives, manifests, and signatures to GitHub Releases with
   `scripts/release-notes` output.
8. Deploy docs with `just docs-deploy` when site content changed.
9. If any release-only commits were made on `main`, merge `main` back into `dev`.

CUDA archives require a CUDA-enabled `skaldd` build on a suitable host. Build and
smoke-test the CUDA archive separately; do not publish a CPU build under a CUDA
artifact name.

Package-manager formats are deferred until archive releases are reliable.
