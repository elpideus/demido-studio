# vendored: tauri-plugin-mcp (frontend api)

Prebuilt frontend bridge for the **dev-only** MCP test-automation plugin
(`initMcpBridge`, used in `src/main.tsx` under `import.meta.env.DEV`).

## Why vendored

Upstream `github:DaveDev42/tauri-plugin-mcp` is a **pnpm monorepo**. Its install
runs `pnpm -r build` (needs `tsup`) → `npm install` cannot build it, which forced
pnpm on the whole project. Vendoring the prebuilt api dist keeps the repo **npm-only**:
plain `npm install` consumes this folder via `file:` with no build step, no pnpm.

The Rust side stays a normal cargo git dep (`src-tauri/Cargo.toml`) — cargo builds
git deps natively, no pnpm involved.

## Provenance

- Source: https://github.com/DaveDev42/tauri-plugin-mcp `packages/tauri-plugin-mcp-api`
- Commit: `bedc4acb3ad995ca42a070149a2dad6ae219389a` (matches the Cargo.lock pin)
- Built with: `pnpm --filter tauri-plugin-mcp-api build` (`tsc`)

## Refresh on upstream change

```
git clone https://github.com/DaveDev42/tauri-plugin-mcp /tmp/tpm
cd /tmp/tpm && pnpm install; pnpm --filter tauri-plugin-mcp-api build
cp packages/tauri-plugin-mcp-api/dist/{index.js,index.d.ts} \
   <repo>/vendor/tauri-plugin-mcp/dist/
```

Bump the cargo pin (`src-tauri/Cargo.toml` / `Cargo.lock`) to the same commit.
