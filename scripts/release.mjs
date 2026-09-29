#!/usr/bin/env node
// Cuts a release: sets the version, commits it, tags it and pushes both. The pushed tag starts the
// release workflow (.github/workflows/release.yml), which builds, signs and publishes the
// installer.
//
//   pnpm release 0.4.1               a release
//   pnpm release 0.5.0-beta.1        a pre-release (a version with a "-")
//   pnpm release 0.4.1 --no-push     commit and tag only; it prints the push command for later
//
// It refuses unless the working tree is clean, the branch is main and main is not behind
// origin/main, so a release is always a commit of main with nothing left out of it. When the
// version is already the one asked for (the first release of a version set by hand), there is
// nothing to commit and the current commit is tagged.

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The same rule as scripts/version.mjs: semantic versions without build metadata. */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?$/;
const USAGE = 'Usage: pnpm release <version> [--no-push]   (1.2.3, or 1.2.3-beta.1 for a pre-release)';

function fail(message) {
  console.error(message);
  process.exit(1);
}

/** Runs git and returns what it printed (the end trimmed: `status --porcelain` lines start with a space). */
function git(...args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trimEnd();
}

/** Runs git in the terminal, so a signing prompt or the push's progress reaches the person. */
function gitLive(...args) {
  console.log(`$ git ${args.map((a) => (a.includes(' ') ? `"${a}"` : a)).join(' ')}`);
  execFileSync('git', args, { cwd: root, stdio: 'inherit' });
}

function succeeds(...args) {
  try {
    git(...args);
    return true;
  } catch {
    return false;
  }
}

// Arguments.
const args = process.argv.slice(2);
if (args.includes('--help') || args.includes('-h')) {
  console.log(USAGE);
  process.exit(0);
}
const unknown = args.find((a) => a.startsWith('-') && a !== '--no-push');
if (unknown) fail(`Unknown option ${unknown}\n${USAGE}`);
const positional = args.filter((a) => !a.startsWith('-'));
if (positional.length !== 1) fail(USAGE);
const version = positional[0].replace(/^v/, '');
if (!SEMVER.test(version)) fail(`${positional[0]} is not a version like 1.2.3 or 1.2.3-beta.1`);
const push = !args.includes('--no-push');
const tag = `v${version}`;

// Refusals, before anything changes.
const changes = git('status', '--porcelain');
if (changes) fail(`The working tree has changes. Commit or stash them first:\n${changes}`);

let branch = '';
try {
  branch = git('symbolic-ref', '--quiet', '--short', 'HEAD');
} catch {
  // A detached HEAD: no branch at all.
}
if (branch !== 'main') fail(`Releases are cut from main, and this is ${branch || 'a detached HEAD'}.`);

try {
  git('fetch', '--quiet', 'origin');
} catch (e) {
  fail(`Could not fetch origin, so there is no telling whether main is up to date:\n${e.stderr ?? e.message}`);
}
if (!succeeds('rev-parse', '--verify', '--quiet', 'refs/remotes/origin/main')) fail('origin has no main branch.');
const behind = Number(git('rev-list', '--count', 'HEAD..origin/main'));
if (behind > 0) fail(`main is ${behind} commit(s) behind origin/main. Pull first.`);

if (succeeds('rev-parse', '--verify', '--quiet', `refs/tags/${tag}`)) fail(`The tag ${tag} already exists.`);
if (git('ls-remote', '--tags', 'origin', `refs/tags/${tag}`)) fail(`origin already has the tag ${tag}.`);

// The release.
let committed = false;
try {
  execFileSync(process.execPath, [path.join(root, 'scripts', 'version.mjs'), version], { cwd: root, stdio: 'inherit' });
  if (git('status', '--porcelain')) {
    // The tree was clean, so every change is the version and `-a` takes exactly those files.
    gitLive('commit', '-a', '-m', `Release ${tag}`);
    committed = true;
  } else {
    console.log(`The version is already ${version}: tagging the current commit.`);
  }
} catch {
  fail('Stopped before committing. `git restore .` puts the version files back.');
}

try {
  gitLive('tag', '-a', tag, '-m', `Demido Studio ${version}`);
} catch {
  fail(
    committed
      ? `The release commit is made but not tagged. \`git reset --hard HEAD~1\` undoes it.`
      : `Could not tag ${tag}.`,
  );
}

const pushCommand = `git push --atomic origin main ${tag}`;
if (!push) {
  console.log(`\nCommitted and tagged ${tag}, not pushed. To publish it: ${pushCommand}`);
  process.exit(0);
}
try {
  gitLive('push', '--atomic', 'origin', 'main', tag);
} catch {
  fail(`\n${tag} is committed and tagged but not pushed. Once the problem is fixed: ${pushCommand}`);
}
console.log(`\nPushed ${tag}. GitHub Actions now builds, signs and publishes it.`);
