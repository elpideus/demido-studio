# Contributing to Demido Studio

Thank you for taking the time to contribute! This document covers everything you need to know to get a change merged.

---

## Table of contents

- [Code of conduct](#code-of-conduct)
- [Ways to contribute](#ways-to-contribute)
- [Before you start](#before-you-start)
- [Development setup](#development-setup)
- [Branch and commit conventions](#branch-and-commit-conventions)
- [Pull request process](#pull-request-process)
- [Coding standards](#coding-standards)
- [Testing](#testing)
- [Reporting security vulnerabilities](#reporting-security-vulnerabilities)

---

## Code of conduct

This project follows the [Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) Code of Conduct. By participating you agree to abide by its terms. Report unacceptable behaviour to the maintainers via email (see your issue tracker contact details).

---

## Ways to contribute

| Type | How |
|------|-----|
| Bug report | [Open a bug report issue](../../issues/new?template=bug_report.yml) |
| Feature request | [Open a feature request issue](../../issues/new?template=feature_request.yml) |
| Code change | Fork → branch → PR (see below) |
| Documentation | Same flow as code — PRs welcome |
| Translation | Open an issue first to coordinate |

---

## Before you start

For any change beyond a trivial typo fix:

1. **Search existing issues** to see if it has already been reported or discussed.
2. **Open an issue** describing what you want to change and why. This avoids duplicate work and lets maintainers flag conflicts early.
3. Wait for a maintainer to label the issue `accepted` or comment that the direction is good before investing significant effort.

For small, obvious fixes (typos, broken links, one-liner bug fixes) you can skip the issue and open a PR directly.

---

## Development setup

### Prerequisites

- Node.js 20 LTS
- Rust stable (install via [rustup](https://rustup.rs))
- Tauri prerequisites for your platform — [Windows](https://tauri.app/start/prerequisites/#windows) | [macOS](https://tauri.app/start/prerequisites/#macos) | [Linux](https://tauri.app/start/prerequisites/#linux)

### Clone and install

```bash
git clone https://github.com/demido-studio/demido-studio.git
cd demido-studio
npm install
```

### Run in development

```bash
npm run tauri dev
```

Vite serves the frontend on `http://localhost:1420` with HMR. The Tauri shell recompiles Rust on file save (incremental, usually a few seconds).

### Run frontend tests

```bash
npm test
```

### Run Rust tests

```bash
cd src-tauri
cargo test
```

### Lint / type-check

```bash
# TypeScript
npx tsc --noEmit

# Rust
cd src-tauri && cargo clippy -- -D warnings
```

---

## Branch and commit conventions

### Branches

Branch off `main`. Name your branch using one of these prefixes:

| Prefix | Use for |
|--------|---------|
| `feat/` | New functionality |
| `fix/` | Bug fixes |
| `refactor/` | Code restructuring without behaviour change |
| `chore/` | Build, CI, dependency, or tooling changes |
| `docs/` | Documentation only |

Examples: `feat/keyboard-shortcuts`, `fix/fts5-backfill`, `refactor/split-commands`.

### Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `perf`.

Scopes (optional but encouraged): `db`, `providers`, `mcp`, `agent`, `ui`, `settings`, `streaming`, `fs`.

Examples:

```
feat(mcp): add per-server connection timeout
fix(db): backfill FTS5 index for pre-migration messages
chore(ci): add cargo clippy step to GitHub Actions
```

**Keep commits atomic** — one logical change per commit. Squash work-in-progress commits before opening a PR.

---

## Pull request process

1. **Target `main`** — all PRs go to `main`.
2. **Fill in the PR template** — description, motivation, and test plan are required.
3. **Ensure CI passes** — the automated checks must be green before review.
4. **Request a review** — tag a maintainer or wait; we aim to respond within 3 business days.
5. **Address feedback** — push follow-up commits to the same branch (do not force-push after review has started unless asked).
6. **Squash and merge** — maintainers will squash-merge once approved.

### PR checklist

Before marking your PR ready for review:

- [ ] CI checks pass (lint, type-check, tests)
- [ ] New behaviour is covered by tests (or existing tests updated)
- [ ] No unrelated changes are included
- [ ] `CHANGELOG.md` updated (if user-visible change)
- [ ] Documentation updated if you changed public-facing behaviour

---

## Coding standards

### Rust

- **Edition**: 2021
- Follow `rustfmt` defaults (run `cargo fmt` before committing)
- **No `unwrap()` or `expect()` in production paths** — propagate errors with `?` and return `Result`. Use `expect` only for invariants that are truly impossible to violate (document why in a comment).
- Keep `#[tauri::command]` handlers thin — delegate to functions in the appropriate `db/` or `providers/` module.
- New DB changes must go through a numbered migration in `db/mod.rs::MIGRATIONS`. Never edit existing migrations.

### TypeScript / React

- **Strict mode** enabled — no `any` without a comment explaining why.
- Prefer functional components and hooks.
- State lives in Zustand stores, not component state, unless it is purely local UI state.
- Derive values from store state rather than duplicating them.
- Components under `src/components/` should be presentational where possible; data fetching and side effects belong in stores.
- Use `DOMPurify.sanitize()` on any AI-generated content rendered as HTML.

### General

- No commented-out code in PRs.
- No debug `console.log` / `eprintln!` left in.
- Prefer deleting code over leaving it dead.

---

## Testing

| Layer | Tool | Location |
|-------|------|----------|
| Frontend unit | Vitest + Testing Library | `src/test/` |
| Backend unit | `cargo test` | `src-tauri/src/**/tests` |
| E2E (planned) | Playwright | `tests/` |

For bug fixes, add a test that fails before your fix and passes after. For new features, add tests that cover the happy path and at least one error case.

---

## Reporting security vulnerabilities

**Do not open a public issue for security vulnerabilities.**

Please report them privately by emailing the maintainer (see the GitHub Security Advisory tab or the email on the maintainer's profile). We aim to acknowledge reports within 48 hours and will coordinate a disclosure timeline with you.
