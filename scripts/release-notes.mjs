#!/usr/bin/env node
// The body of a GitHub release, in Markdown: what changed since the previous release, then how to
// install it. The release workflow publishes it next to the installer.
//
//   node scripts/release-notes.mjs v0.4.1
//
// "What's changed" is the subject of every commit since the previous release (every commit up to
// the tag, for the first release), leaving out the "Release v..." commits that only set the
// version. A pre-release counts from the previous v* tag of any kind. A stable release counts from
// the previous stable one, so it also lists what its pre-releases already shipped: people on the
// Release channel never saw those.

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The same rule as scripts/version.mjs: semantic versions without build metadata. */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?$/;

function git(...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trimEnd();
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

/**
 * The release before `tag`, or null when there is none: the previous v* tag for a pre-release,
 * the previous stable tag (one without a "-") for a stable release.
 */
function previousTag(tag) {
  const exclude = tag.includes('-') ? [] : ['--exclude', 'v*-*'];
  try {
    return git('describe', '--tags', '--abbrev=0', '--match', 'v*', ...exclude, `${tag}^`);
  } catch {
    // No earlier such tag, or the tag is on the very first commit, which has no parent.
    return null;
  }
}

const tag = process.argv[2];
if (!tag || !tag.startsWith('v') || !SEMVER.test(tag.slice(1))) {
  fail('Usage: node scripts/release-notes.mjs v<version>   (v1.2.3, or v1.2.3-beta.1 for a pre-release)');
}
const version = tag.slice(1);

try {
  git('rev-parse', '--verify', '--quiet', `refs/tags/${tag}`);
} catch {
  fail(`There is no tag ${tag}`);
}

const previous = previousTag(tag);
const subjects = git('log', '--format=%s', previous ? `${previous}..${tag}` : tag)
  .split('\n')
  .map((subject) => subject.trim())
  .filter((subject) => subject && !subject.startsWith('Release v'));

const changes = subjects.length ? subjects.map((subject) => `- ${subject}`) : ['- No changes besides the version.'];

// One line per paragraph: GitHub shows every line break in a release body.
const install = [
  `Download \`Demido-Studio-Setup-${version}.exe\` below and run it.`,
  'If Demido Studio is already installed, Setup updates it in place and keeps your chats, models and settings.',
  'Installations from 0.4.0 on also update themselves: see Settings, Updates.',
].join(' ');

process.stdout.write(["## What's changed", '', ...changes, '', '## Install', '', install, ''].join('\n'));
