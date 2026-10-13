// The AUR package's files, as scripts/aur.mjs points them at a release.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { pkgbuild, srcinfo } from './aur.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = path.join(root, 'packaging', 'aur', 'demido-studio-bin');
const read = (name) => readFileSync(path.join(dir, name), 'utf8');
const sha = 'ab'.repeat(32);

describe('aur.mjs', () => {
  test('sets the version, resets pkgrel and writes the checksum in the PKGBUILD', () => {
    const out = pkgbuild(read('PKGBUILD').replace(/^pkgrel=.*$/m, 'pkgrel=3'), '9.8.7', sha);
    assert.match(out, /^pkgver=9\.8\.7$/m);
    assert.match(out, /^pkgrel=1$/m);
    assert.match(out, new RegExp(`^sha256sums=\\('${sha}'\\)$`, 'm'));
    // The source names the release through $pkgver, so it follows.
    assert.match(out, /releases\/download\/v\$pkgver\/\$_pkgname-\$pkgver-linux-x86_64\.tar\.gz/);
  });

  test('changes .SRCINFO as makepkg would print it', () => {
    const out = srcinfo(read('.SRCINFO'), '9.8.7', sha);
    assert.match(out, /^\tpkgver = 9\.8\.7$/m);
    assert.match(out, /^\tpkgrel = 1$/m);
    assert.match(out, new RegExp(`^\\tsha256sums = ${sha}$`, 'm'));
    assert.match(
      out,
      /^\tsource = demido-studio-9\.8\.7-linux-x86_64\.tar\.gz::https:\/\/github\.com\/elpideus\/demido-studio\/releases\/download\/v9\.8\.7\/demido-studio-9\.8\.7-linux-x86_64\.tar\.gz$/m,
    );
  });

  test('.SRCINFO in the repository is what makepkg prints for the PKGBUILD', { skip: !hasMakepkg() }, () => {
    const printed = execFileSync('makepkg', ['--printsrcinfo'], { cwd: dir, encoding: 'utf8' });
    assert.equal(read('.SRCINFO'), printed);
  });

  test('refuses a pre-release and a malformed checksum', () => {
    const script = path.join(root, 'scripts', 'aur.mjs');
    for (const args of [
      ['0.6.0-beta.1', sha],
      ['0.6.0', 'nope'],
    ]) {
      assert.throws(() => execFileSync(process.execPath, [script, ...args], { stdio: 'pipe' }));
    }
  });
});

function hasMakepkg() {
  try {
    execFileSync('makepkg', ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}
