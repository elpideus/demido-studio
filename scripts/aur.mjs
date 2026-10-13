#!/usr/bin/env node
// Points the AUR package demido-studio-bin (packaging/aur/demido-studio-bin) at a release: sets
// pkgver, resets pkgrel and writes the tarball's checksum, in the PKGBUILD and in .SRCINFO, which
// the AUR reads without running makepkg. The release workflow runs it for every stable release;
// the package follows those only.
//
//   node scripts/aur.mjs 0.6.0 <sha256 of demido-studio-0.6.0-linux-x86_64.tar.gz>
//
// Then copy PKGBUILD, .SRCINFO and demido-studio.install into a clone of
// ssh://aur@aur.archlinux.org/demido-studio-bin.git, commit and push (docs/packaging.md).

import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

/** A stable release's version: no pre-release, which pacman would read as a pkgrel. */
const STABLE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** The PKGBUILD at `version`, with the tarball's `sha256`. */
export function pkgbuild(text, version, sha256) {
  let out = replaceOnce(text, /^pkgver=.*$/m, `pkgver=${version}`);
  out = replaceOnce(out, /^pkgrel=.*$/m, 'pkgrel=1');
  return replaceOnce(out, /^sha256sums=\(.*\)$/m, `sha256sums=('${sha256}')`);
}

/** .SRCINFO at `version`, as makepkg --printsrcinfo would print it for that PKGBUILD. */
export function srcinfo(text, version, sha256) {
  const old = text.match(/^\tpkgver = (.*)$/m)?.[1];
  if (!old) throw new Error('.SRCINFO has no pkgver');
  let out = replaceOnce(text, /^\tpkgver = .*$/m, `\tpkgver = ${version}`);
  out = replaceOnce(out, /^\tpkgrel = .*$/m, '\tpkgrel = 1');
  out = replaceOnce(out, /^\tsha256sums = .*$/m, `\tsha256sums = ${sha256}`);
  return replaceOnce(out, /^\tsource = .*$/m, (line) => line.split(old).join(version));
}

function replaceOnce(text, pattern, replacement) {
  if (!pattern.test(text)) throw new Error(`nothing matches ${pattern}`);
  return text.replace(pattern, replacement);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, sha256] = process.argv.slice(2).map((a) => a?.replace(/^v/, ''));
  if (!version || !STABLE.test(version) || !/^[0-9a-f]{64}$/.test(sha256 ?? '')) {
    console.error('Usage: node scripts/aur.mjs <stable version> <sha256 of the release tarball>');
    process.exit(1);
  }
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'packaging', 'aur', 'demido-studio-bin');
  for (const [name, update] of [
    ['PKGBUILD', pkgbuild],
    ['.SRCINFO', srcinfo],
  ]) {
    const file = path.join(dir, name);
    writeFileSync(file, update(readFileSync(file, 'utf8'), version, sha256));
  }
  console.log(`demido-studio-bin is at ${version}-1`);
}
