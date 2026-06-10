# VoxLine

VoxLine is a Linux-first, local-first dictation daemon and CLI.

This repository currently contains the M0-M1 foundation: configuration, typed
protocol messages, a Unix-domain socket daemon, CLI control, event watching,
and environment diagnostics.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo run -p voxlined -- --foreground
cargo run -p voxline-cli -- status
```

The product and architecture specification is in
[`VoxLine_implementation_plan.md`](VoxLine_implementation_plan.md).

## Privacy

VoxLine does not store transcripts or audio by default. Cloud cleanup is not
implemented in this milestone and will remain opt-in when added.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).

