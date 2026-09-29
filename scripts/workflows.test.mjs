// Tests for the GitHub workflows in .github/workflows: the rules that keep the release signing key
// safe, which nothing else would notice being broken until a release. Read as text: the files
// keep to one indentation style, and a YAML parser would be a dependency for this alone.
//
//   node --test scripts/workflows.test.mjs

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = path.join(repo, '.github', 'workflows');
const workflows = readdirSync(dir)
  .filter((file) => /\.ya?ml$/.test(file))
  .map((file) => ({ file, text: readFileSync(path.join(dir, file), 'utf8').replace(/\r\n/g, '\n') }));
const release = workflows.find((w) => w.file === 'release.yml').text;

/** Each job of a workflow, by id: the lines from `  <id>:` to the next job, comments left out. */
function jobs(text) {
  const body = text.slice(text.indexOf('\njobs:\n') + '\njobs:\n'.length);
  const found = {};
  let current = null;
  for (const line of body.split('\n')) {
    if (/^\s*#/.test(line)) continue;
    const start = line.match(/^ {2}([\w-]+):$/);
    if (start) found[(current = start[1])] = '';
    else if (/^\S/.test(line)) break;
    else if (current) found[current] += `${line}\n`;
  }
  return found;
}

describe('workflows', () => {
  test('pin every action to a commit, with its tag in a comment', () => {
    let count = 0;
    for (const { file, text } of workflows) {
      for (const m of text.matchAll(/^\s*(?:- )?uses: (.+)$/gm)) {
        const use = m[1];
        if (use.startsWith('./')) continue;
        count++;
        assert.match(use, /^[\w.-]+\/[\w.-]+(?:\/[\w./-]+)?@[0-9a-f]{40} # \S+$/, `${file}: ${use}`);
      }
    }
    assert.ok(count > 0, 'no actions found');
  });
});

describe('release.yml', () => {
  const all = jobs(release);

  test('has the jobs it is expected to have', () => {
    assert.deepEqual(Object.keys(all), ['checks', 'build', 'sign', 'publish']);
  });

  test('gives the signing secrets to the sign job alone', () => {
    for (const [id, job] of Object.entries(all)) {
      if (id !== 'sign') assert.doesNotMatch(job, /secrets\./, `the ${id} job reads a secret`);
    }
    assert.match(all.sign, /secrets\.TAURI_SIGNING_PRIVATE_KEY\b/);
    assert.match(all.sign, /secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD\b/);
  });

  test('signs on a runner that checks out, installs and caches nothing', () => {
    const sign = all.sign;
    assert.match(sign, /^ {4}needs: \[checks, build\]$/m);
    assert.match(sign, /^ {4}if: github\.event_name == 'push' && startsWith\(github\.ref, 'refs\/tags\/v'\)$/m);
    assert.match(sign, /^ {4}permissions: \{\}$/m);
    assert.match(sign, /^ {4}environment: release$/m);
    const uses = [...sign.matchAll(/uses: ([\w-]+\/[\w-]+)@/g)].map((m) => m[1]);
    assert.deepEqual(uses, ['actions/setup-node', 'actions/download-artifact', 'actions/upload-artifact']);
    assert.match(sign, /package-manager-cache: false/);
    assert.doesNotMatch(sign, /pnpm|cargo|cache: true/);
  });

  test('signs with the tauri CLI version in pnpm-lock.yaml, from npm, in an empty directory', () => {
    const lock = readFileSync(path.join(repo, 'pnpm-lock.yaml'), 'utf8').replace(/\r\n/g, '\n');
    const locked = new Set(
      [...lock.matchAll(/^ {6}'@tauri-apps\/cli':\n {8}specifier: .+\n {8}version: (\S+)$/gm)].map((m) => m[1]),
    );
    assert.equal(locked.size, 1, `pnpm-lock.yaml has @tauri-apps/cli at ${[...locked].join(', ') || 'no version'}`);
    const pinned = all.sign.match(/^ {6}TAURI_CLI_VERSION: (\S+)$/m)?.[1];
    assert.equal(pinned, [...locked][0], 'TAURI_CLI_VERSION in release.yml differs from pnpm-lock.yaml');
    assert.match(
      all.sign,
      /cd "\$\(mktemp -d\)"\n\s+npx --yes "@tauri-apps\/cli@\$TAURI_CLI_VERSION" signer sign --app-version "\$VERSION" "\$setup"/,
    );
  });

  test('uploads the installer unsigned from build, signed from sign, and publishes the signed one', () => {
    assert.match(all.build, /name: Demido-Studio-Setup-\$\{\{ steps\.version\.outputs\.version \}\}-unsigned\n/);
    assert.doesNotMatch(all.build, /\.sig\b/);
    assert.match(all.sign, /name: Demido-Studio-Setup-\$\{\{ needs\.build\.outputs\.version \}\}-unsigned\n/);
    assert.match(all.sign, /\.exe\.sig\n/);
    assert.match(all.publish, /^ {4}needs: \[checks, build, sign\]$/m);
    assert.match(all.publish, /name: Demido-Studio-Setup-\$\{\{ needs\.build\.outputs\.version \}\}\n/);
  });
});
