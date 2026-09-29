#!/usr/bin/env node
// The version is written in several files that must agree: the npm packages, both Tauri configs,
// the Cargo workspace and the workspace crates' entries in Cargo.lock. The root package.json is
// the one the others follow.
//
//   node scripts/version.mjs                  print the version
//   node scripts/version.mjs 0.5.0            set it everywhere (0.5.0-beta.1 for a pre-release)
//   node scripts/version.mjs --check [0.5.0]  fail when a file disagrees, or differs from 0.5.0

import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Semantic versions without build metadata, which Windows and the updater would both drop. A
 * numeric pre-release identifier has no leading zero (beta.1, never beta.01): Cargo refuses to
 * read such a version, and the updater skips it.
 */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?$/;

const JSON_FILES = [
  'package.json',
  'apps/studio/package.json',
  'apps/installer/package.json',
  'packages/ui/package.json',
  'sidecars/market/package.json',
  'apps/studio/src-tauri/tauri.conf.json',
  'apps/installer/src-tauri/tauri.conf.json',
];

// The first "version" key of a JSON file is its own: the value is replaced in place, so the
// file's formatting is left as it was.
const JSON_VERSION = /("version"\s*:\s*")([^"]*)(")/;
// `version = "..."` inside the `[workspace.package]` table.
const CARGO_VERSION = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]*)(")/;
// A workspace crate in Cargo.lock: every crate named demido-* is a path dependency of this repo.
const LOCK_ENTRY = /(\[\[package\]\]\r?\nname = "demido-[a-z-]+"\r?\nversion = ")([^"]*)(")/g;

const read = (file) => readFileSync(path.join(root, file), 'utf8');

/** Every place the version is written, with the version found there. */
function versions() {
  const found = [];
  for (const file of JSON_FILES) {
    found.push({ file, version: read(file).match(JSON_VERSION)?.[2] ?? null });
  }
  found.push({ file: 'Cargo.toml', version: read('Cargo.toml').match(CARGO_VERSION)?.[2] ?? null });
  for (const m of read('Cargo.lock').matchAll(LOCK_ENTRY)) {
    const name = m[1].match(/name = "([^"]+)"/)[1];
    found.push({ file: `Cargo.lock (${name})`, version: m[2] });
  }
  return found;
}

function replaceIn(file, pattern, version) {
  const text = read(file);
  if (!pattern.test(text)) throw new Error(`${file}: no version found`);
  pattern.lastIndex = 0;
  writeFileSync(
    path.join(root, file),
    text.replace(pattern, (_, before, _old, after) => `${before}${version}${after}`),
  );
}

function set(version) {
  for (const file of JSON_FILES) replaceIn(file, JSON_VERSION, version);
  replaceIn('Cargo.toml', CARGO_VERSION, version);
  replaceIn('Cargo.lock', LOCK_ENTRY, version);
}

function check(expected) {
  const found = versions();
  const want = expected ?? found[0].version;
  const wrong = found.filter((f) => f.version !== want);
  if (wrong.length) {
    for (const f of wrong) console.error(`${f.file}: ${f.version ?? 'missing'} (expected ${want})`);
    process.exit(1);
  }
  console.log(`${want} in ${found.length} places`);
}

const args = process.argv.slice(2);
if (args[0] === '--check') {
  const expected = args[1]?.replace(/^v/, '');
  if (expected !== undefined && !SEMVER.test(expected)) {
    console.error(`${expected} is not a version like 1.2.3 or 1.2.3-beta.1`);
    process.exit(1);
  }
  check(expected);
} else if (args[0]) {
  const version = args[0].replace(/^v/, '');
  if (!SEMVER.test(version)) {
    console.error(`${version} is not a version like 1.2.3 or 1.2.3-beta.1`);
    process.exit(1);
  }
  set(version);
  check(version);
} else {
  console.log(versions()[0].version);
}
