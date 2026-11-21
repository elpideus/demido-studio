#!/usr/bin/env node
// Builds everything in brand/: design tokens and glyph masks, then the brand kit page.
// Run: node brand/build.mjs
await import('./tokens/build.mjs')
await import('./kit/build.mjs')
