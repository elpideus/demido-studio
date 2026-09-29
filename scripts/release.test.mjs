// Tests for scripts/release.mjs and scripts/release-notes.mjs, run against throwaway git
// repositories in the temp folder, each with a bare repository as its origin. Git runs with a
// config of its own, so nothing here signs with, or reads, the person's git settings.
//
//   node --test scripts/release.test.mjs

import assert from 'node:assert/strict';
import { spawnSync, execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const temp = mkdtempSync(path.join(os.tmpdir(), 'demido-release-test-'));
after(() => rmSync(temp, { recursive: true, force: true }));

const gitconfig = path.join(temp, 'gitconfig');
writeFileSync(
  gitconfig,
  [
    '[user]',
    '  name = Release Test',
    '  email = release-test@example.com',
    '[commit]',
    '  gpgsign = false',
    '[tag]',
    '  gpgsign = false',
    '[init]',
    '  defaultBranch = main',
    '[core]',
    '  autocrlf = false',
    '',
  ].join('\n'),
);
const env = { ...process.env, GIT_CONFIG_GLOBAL: gitconfig, GIT_CONFIG_NOSYSTEM: '1' };

// The files scripts/version.mjs writes the version in.
const VERSION_FILES = [
  'package.json',
  'apps/studio/package.json',
  'apps/installer/package.json',
  'packages/ui/package.json',
  'sidecars/market/package.json',
  'apps/studio/src-tauri/tauri.conf.json',
  'apps/installer/src-tauri/tauri.conf.json',
  'Cargo.toml',
  'Cargo.lock',
];
const SCRIPTS = ['scripts/version.mjs', 'scripts/release.mjs', 'scripts/release-notes.mjs'];

let count = 0;

function git(cwd, ...args) {
  return execFileSync('git', args, { cwd, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

function commit(cwd, subject) {
  git(cwd, 'commit', '--allow-empty', '-m', subject);
}

/** Runs one of the copied scripts in `cwd`; returns its exit code and output. */
function script(cwd, name, ...args) {
  const r = spawnSync(process.execPath, [path.join(cwd, 'scripts', name), ...args], { cwd, env, encoding: 'utf8' });
  return { code: r.status, stdout: r.stdout, stderr: r.stderr };
}

/**
 * This repository's release scripts and version files, committed on main as "Start" and pushed to
 * a bare origin. Built once and copied for each test: starting git costs a noticeable moment on
 * Windows. The origin is a relative path, so a copy pushes to its own.
 */
function template() {
  const dir = path.join(temp, 'template');
  const work = path.join(dir, 'work');
  const origin = path.join(dir, 'origin.git');
  for (const file of [...VERSION_FILES, ...SCRIPTS]) {
    mkdirSync(path.dirname(path.join(work, file)), { recursive: true });
    copyFileSync(path.join(repo, file), path.join(work, file));
  }
  git(work, 'init', '--quiet', '-b', 'main');
  git(work, 'add', '-A');
  git(work, 'commit', '--quiet', '-m', 'Start');
  mkdirSync(origin);
  git(origin, 'init', '--quiet', '--bare', '-b', 'main');
  git(work, 'remote', 'add', 'origin', '../origin.git');
  git(work, 'push', '--quiet', '-u', 'origin', 'main');
  return dir;
}

let templateDir;

/** A fresh copy of the template: `work` on main, level with `origin`. */
function fixture() {
  templateDir ??= template();
  const dir = path.join(temp, `case-${++count}`);
  cpSync(templateDir, dir, { recursive: true });
  return { work: path.join(dir, 'work'), origin: path.join(dir, 'origin.git') };
}

const version = () => JSON.parse(readFileSync(path.join(repo, 'package.json'), 'utf8')).version;

describe('the version rule', () => {
  // Each script has its own copy of the rule; they must not drift apart.
  const rules = SCRIPTS.map(
    (file) => readFileSync(path.join(repo, file), 'utf8').match(/const SEMVER =\s*(\/.+\/);/)?.[1],
  );

  test('is the same in every script', () => {
    assert.ok(rules[0], 'scripts/version.mjs has no SEMVER');
    for (const [i, rule] of rules.entries()) assert.equal(rule, rules[0], `${SCRIPTS[i]} differs from ${SCRIPTS[0]}`);
  });

  test('takes semantic versions without build metadata or leading zeros', () => {
    const [, source, flags] = rules[0].match(/^\/(.*)\/([a-z]*)$/);
    const semver = new RegExp(source, flags);
    for (const v of [
      '0.0.0',
      '1.2.3',
      '10.20.30',
      '0.5.0-beta.0',
      '0.5.0-beta.1',
      '0.5.0-rc.10',
      '0.5.0-0a',
      '0.5.0-alpha-1.x',
      '0.5.0-01a',
    ]) {
      assert.ok(semver.test(v), `${v} is refused`);
    }
    for (const v of [
      '1.2',
      '01.2.3',
      '1.02.3',
      '1.2.03',
      '1.2.3+build.4',
      '1.2.3-',
      '1.2.3-beta.',
      '1.2.3-beta..1',
      '0.5.0-beta.01',
      '0.5.0-01',
      '0.5.0-00',
    ]) {
      assert.ok(!semver.test(v), `${v} is taken`);
    }
  });
});

describe('version.mjs', () => {
  test('refuses a numeric pre-release identifier with a leading zero and changes no file', () => {
    const { work } = fixture();
    for (const args of [['9.9.9-beta.01'], ['--check', '9.9.9-beta.01']]) {
      const r = script(work, 'version.mjs', ...args);
      assert.equal(r.code, 1, `exit code for ${JSON.stringify(args)}`);
      assert.match(r.stderr, /9\.9\.9-beta\.01 is not a version/);
    }
    assert.equal(git(work, 'status', '--porcelain'), '');
  });
});

describe('release.mjs', () => {
  test('rejects missing, malformed and extra arguments before touching git', () => {
    const { work } = fixture();
    const head = git(work, 'rev-parse', 'HEAD');
    for (const args of [[], ['1.2'], ['1.2.3+build.4'], ['1.2.3', '1.2.4'], ['1.2.3', '--push-later']]) {
      const r = script(work, 'release.mjs', ...args);
      assert.equal(r.code, 1, `exit code for ${JSON.stringify(args)}`);
      assert.match(r.stderr, /Usage|not a version|Unknown option/);
    }
    assert.equal(git(work, 'rev-parse', 'HEAD'), head);
    assert.equal(git(work, 'tag'), '');
  });

  test('refuses a numeric pre-release identifier with a leading zero, which Cargo cannot read', () => {
    const { work, origin } = fixture();
    const head = git(work, 'rev-parse', 'HEAD');
    const r = script(work, 'release.mjs', '9.9.9-beta.01');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /9\.9\.9-beta\.01 is not a version/);
    assert.equal(git(work, 'rev-parse', 'HEAD'), head);
    assert.equal(git(work, 'status', '--porcelain'), '');
    assert.equal(git(work, 'tag'), '');
    assert.equal(git(origin, 'tag'), '');
  });

  test('refuses a working tree with changes', () => {
    const { work } = fixture();
    writeFileSync(path.join(work, 'notes.txt'), 'unsaved');
    writeFileSync(path.join(work, 'package.json'), '{}');
    const r = script(work, 'release.mjs', '9.9.9', '--no-push');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /working tree has changes/);
    assert.match(r.stderr, /^ M package\.json$/m);
    assert.match(r.stderr, /^\?\? notes\.txt$/m);
    assert.equal(git(work, 'tag'), '');
  });

  test('refuses a branch other than main', () => {
    const { work } = fixture();
    git(work, 'checkout', '--quiet', '-b', 'feature');
    const r = script(work, 'release.mjs', '9.9.9', '--no-push');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /cut from main, and this is feature/);
  });

  test('refuses when main is behind origin/main', () => {
    const { work, origin } = fixture();
    const other = path.join(path.dirname(work), 'other');
    git(path.dirname(work), 'clone', '--quiet', origin, other);
    commit(other, 'Someone else pushed first');
    git(other, 'push', '--quiet', 'origin', 'main');
    const r = script(work, 'release.mjs', '9.9.9', '--no-push');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /1 commit\(s\) behind origin\/main/);
    assert.equal(git(work, 'tag'), '');
  });

  test('refuses a version whose tag exists here', () => {
    const { work } = fixture();
    git(work, 'tag', 'v9.9.9');
    const r = script(work, 'release.mjs', '9.9.9', '--no-push');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /tag v9\.9\.9 already exists/);
  });

  test('refuses a version whose tag exists only on origin', () => {
    const { work, origin } = fixture();
    // A tag on a commit no branch has: fetching main does not bring it along.
    const other = path.join(path.dirname(work), 'other');
    git(path.dirname(work), 'clone', '--quiet', origin, other);
    commit(other, 'Released from somewhere else');
    git(other, 'tag', 'v9.9.9');
    git(other, 'push', '--quiet', 'origin', 'v9.9.9');
    const r = script(work, 'release.mjs', '9.9.9', '--no-push');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /origin already has the tag v9\.9\.9/);
    assert.equal(git(work, 'log', '-1', '--format=%s'), 'Start');
  });

  test('--no-push commits the new version and tags it, and pushes nothing', () => {
    const { work, origin } = fixture();
    const originMain = git(origin, 'rev-parse', 'main');
    const r = script(work, 'release.mjs', 'v9.9.9-beta.1', '--no-push');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /git push --atomic origin main v9\.9\.9-beta\.1/);

    assert.equal(git(work, 'log', '-1', '--format=%s'), 'Release v9.9.9-beta.1');
    assert.equal(git(work, 'status', '--porcelain'), '');
    assert.equal(script(work, 'version.mjs', '--check', '9.9.9-beta.1').code, 0);
    assert.equal(git(work, 'cat-file', '-t', 'v9.9.9-beta.1'), 'tag');
    assert.equal(git(work, 'tag', '-l', '--format=%(contents:subject)', 'v9.9.9-beta.1'), 'Demido Studio 9.9.9-beta.1');
    assert.equal(git(work, 'rev-parse', 'v9.9.9-beta.1^{commit}'), git(work, 'rev-parse', 'HEAD'));

    assert.equal(git(origin, 'rev-parse', 'main'), originMain);
    assert.equal(git(origin, 'tag'), '');
  });

  test('pushes the release commit and its tag to origin together', () => {
    const { work, origin } = fixture();
    const r = script(work, 'release.mjs', '9.9.9');
    assert.equal(r.code, 0, r.stderr);
    assert.equal(git(origin, 'rev-parse', 'main'), git(work, 'rev-parse', 'HEAD'));
    assert.equal(git(origin, 'rev-parse', 'v9.9.9^{commit}'), git(work, 'rev-parse', 'HEAD'));
    assert.equal(git(origin, 'log', '-1', '--format=%s', 'main'), 'Release v9.9.9');
  });

  test('tags the current commit when the version is already the one asked for', () => {
    const { work } = fixture();
    const head = git(work, 'rev-parse', 'HEAD');
    const r = script(work, 'release.mjs', version(), '--no-push');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /already/);
    assert.equal(git(work, 'rev-parse', 'HEAD'), head);
    assert.equal(git(work, 'rev-parse', `v${version()}^{commit}`), head);
  });
});

describe('release-notes.mjs', () => {
  test('lists every commit for the first release, without the Release commits', () => {
    const { work } = fixture();
    commit(work, 'Add the chat');
    commit(work, 'Release v0.1.0');
    commit(work, 'Fix the chart');
    git(work, 'tag', '-a', 'v0.1.0', '-m', 'Demido Studio 0.1.0');
    const r = script(work, 'release-notes.mjs', 'v0.1.0');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /^## What's changed\n\n- Fix the chart\n- Add the chat\n- Start\n\n## Install\n/);
    assert.doesNotMatch(r.stdout, /Release v/);
    assert.match(r.stdout, /`Demido-Studio-Setup-0\.1\.0\.exe`/);
    assert.match(r.stdout, /Settings, Updates/);
  });

  test('lists only the commits since the previous v* tag', () => {
    const { work } = fixture();
    commit(work, 'Old work');
    git(work, 'tag', 'v0.1.0');
    commit(work, 'Not a release tag');
    git(work, 'tag', 'nightly');
    commit(work, 'New work');
    commit(work, 'Release v0.2.0-beta.1');
    git(work, 'tag', '-a', 'v0.2.0-beta.1', '-m', 'Demido Studio 0.2.0-beta.1');
    const r = script(work, 'release-notes.mjs', 'v0.2.0-beta.1');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /- New work\n- Not a release tag\n\n/);
    assert.doesNotMatch(r.stdout, /Old work|Start/);
    assert.match(r.stdout, /`Demido-Studio-Setup-0\.2\.0-beta\.1\.exe`/);
  });

  test('a stable release lists everything since the previous stable one, pre-releases included', () => {
    const { work } = fixture();
    commit(work, 'Old work');
    git(work, 'tag', '-a', 'v0.4.0', '-m', 'Demido Studio 0.4.0');
    commit(work, 'Feature A');
    commit(work, 'Release v0.5.0-beta.1');
    git(work, 'tag', '-a', 'v0.5.0-beta.1', '-m', 'Demido Studio 0.5.0-beta.1');
    commit(work, 'Feature B');
    commit(work, 'Release v0.5.0-beta.2');
    git(work, 'tag', '-a', 'v0.5.0-beta.2', '-m', 'Demido Studio 0.5.0-beta.2');
    commit(work, 'Release v0.5.0');
    git(work, 'tag', '-a', 'v0.5.0', '-m', 'Demido Studio 0.5.0');

    const stable = script(work, 'release-notes.mjs', 'v0.5.0');
    assert.equal(stable.code, 0, stable.stderr);
    assert.match(stable.stdout, /^## What's changed\n\n- Feature B\n- Feature A\n\n/);
    assert.doesNotMatch(stable.stdout, /Old work|No changes/);

    // A pre-release still counts from the release just before it, pre-release or not.
    const beta = script(work, 'release-notes.mjs', 'v0.5.0-beta.2');
    assert.equal(beta.code, 0, beta.stderr);
    assert.match(beta.stdout, /^## What's changed\n\n- Feature B\n\n/);
  });

  test('a first stable release after only pre-releases lists every commit', () => {
    const { work } = fixture();
    commit(work, 'Feature A');
    git(work, 'tag', 'v0.1.0-beta.1');
    commit(work, 'Feature B');
    git(work, 'tag', 'v0.1.0');
    const r = script(work, 'release-notes.mjs', 'v0.1.0');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /- Feature B\n- Feature A\n- Start\n\n/);
  });

  test('handles a tag on the very first commit', () => {
    const { work } = fixture();
    git(work, 'tag', 'v0.1.0');
    const r = script(work, 'release-notes.mjs', 'v0.1.0');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /- Start\n/);
  });

  test('says so when the only commit is the version change', () => {
    const { work } = fixture();
    git(work, 'tag', 'v0.1.0');
    commit(work, 'Release v0.1.1');
    git(work, 'tag', 'v0.1.1');
    const r = script(work, 'release-notes.mjs', 'v0.1.1');
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /- No changes besides the version\./);
  });

  test('fails on a missing, malformed or unknown tag', () => {
    const { work } = fixture();
    for (const args of [[], ['0.1.0'], ['vnext'], ['v9.9.9']]) {
      const r = script(work, 'release-notes.mjs', ...args);
      assert.equal(r.code, 1, `exit code for ${JSON.stringify(args)}`);
    }
    // A tag that exists but is not a version the release scripts take.
    git(work, 'tag', 'v0.1.0-beta.01');
    const r = script(work, 'release-notes.mjs', 'v0.1.0-beta.01');
    assert.equal(r.code, 1);
    assert.match(r.stderr, /Usage/);
  });
});
