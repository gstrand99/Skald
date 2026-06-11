# VoxLine documentation site

[Astro Starlight](https://starlight.astro.build/) site for [docs.voxline.dev](https://docs.voxline.dev).

## Requirements

- [Bun](https://bun.sh/) (not Node)

## Commands

```bash
bun install
bun run dev       # local dev server
bun run build     # output to dist/
bun run deploy    # build + wrangler deploy to Cloudflare Workers
```

From the repository root:

```bash
just docs-dev
just docs-build
just docs-deploy
```

## Theme

[Gruvbox for Starlight](https://starlight-theme-gruvbox.otterlord.dev/) via
`starlight-theme-gruvbox`.

## Custom domain

`docs.voxline.dev` is set in `astro.config.ts` (`site`) and attached to the Workers
asset route in the Cloudflare dashboard.
