# macOS configuration

Skald stores configuration in `~/Library/Application Support/Skald` and models
in its `models` subdirectory. The native app uses Metal acceleration by default
and leaves previous-clipboard restoration disabled to avoid unnecessary
pasteboard reads.

Copy `config.toml` to the application-support directory or run
`skald config init`, then open Skald from Applications to grant microphone and
Accessibility access.
