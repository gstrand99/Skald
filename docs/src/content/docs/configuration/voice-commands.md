---
title: "[voice_commands]"
description: Experimental spoken routing prefixes in transcripts.
---

**Experimental.** Disabled by default. Parses a spoken prefix at the **start** of
the transcript after ASR to select cleanup styles or insert snippets.

```toml
[voice_commands]
enabled = false
prefix = "voxline"
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | boolean | `false` | When true, parse voice commands from transcript text. |
| `prefix` | string | `"voxline"` | Required spoken prefix (case-insensitive matching). ASR may split this into two words (`Vox Line`); both are recognized. |

## Behavior

When enabled, a transcript starting with the prefix can:

- Select a cleanup style — e.g. `voxline professional thanks for the update`
- Trigger snippet-only insertion — e.g. `voxline signature` with an empty remainder
- Route to named snippet aliases configured in `snippets/` and command registry

The prefix and command word are stripped from text sent to cleanup and insertion.

## Testing

```bash
# Enable [voice_commands] in config.toml first
voxline commands test "voxline professional hey john thanks"
voxline commands conflicts
```

`voxline config validate` checks voice command aliases for conflicts with snippets.

## Notes

- Requires clear pronunciation of the prefix; noisy environments may mis-trigger.
- Does not replace compositor shortcuts; it routes **after** transcription.
- See snippet and style docs under [Related files](/configuration/related-files/).
