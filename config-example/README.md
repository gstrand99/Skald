# VoxLine configuration examples

Reference configurations for each platform. These files document the supported
`config.toml` shape; they are not loaded automatically.

| Path | Purpose |
|------|---------|
| [`linux/config.toml`](linux/config.toml) | Default Linux power-user profile |
| [`linux/styles/`](linux/styles/) | Default cleanup style metadata and prompt files |
| [`linux/apps/`](linux/apps/) | Application profile examples (terminal clipboard-only) |
| [`linux/snippets/`](linux/snippets/) | Insert snippet examples (static content, no LLM) |
| `mac/` | Reserved for a future macOS port |
| `windows/` | Reserved for a future Windows port |

On Linux, prefer generating a live config with:

```bash
voxline config init
```

Then validate with `voxline config validate` and `voxline doctor`.
