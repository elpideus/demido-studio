# Attribution

**Enforced now.** `scripts/check-rules.mjs`, rule `attribution`, and the
`.githooks/commit-msg` hook.

Decided on wayfinder ticket
[#16](https://github.com/elpideus/demido-studio/issues/16). This is hard rule 1
of [`AGENTS.md`](../../AGENTS.md).

## The rule

1. Every commit is authored **and** committed by
   `Stefan Cucoranu <elpideus@gmail.com>`.
2. Every commit is signed, with the SSH key in
   [`.github/allowed_signers`](../../.github/allowed_signers).
3. No commit message carries a `Co-Authored-By` trailer.
4. No assistant is named in a commit message, a pull request body, an issue, or
   any other repo metadata.

## Why it is a rule at all

Demido Studio is sole-authored, released under GPL-3.0-or-later, and worked on
across many agent sessions. Authorship that drifts is a licensing problem, not a
vanity one: the copyright holder has to be one identifiable person for the
license to be the thing it says it is. Signing is what makes that claim
checkable by somebody who does not trust the forge.

The fourth point is the one an agent breaks by default, because most harnesses
append a co-author trailer or a "generated with" line unless told otherwise. It
is not a judgement about how the code was written. It is that repo metadata
records **who is responsible for this software**, and that is one person.

## What is checked, and where

| Check | `commit-msg` hook | CI |
|---|---|---|
| Author and committer identity | yes | yes |
| Signature present | configuration only | yes, verified against the key |
| `Co-Authored-By` trailer | yes | yes |
| An assistant named | yes | yes |
| Em dash in the message ([rule 6](text.md)) | yes | yes |

The hook is the cheap gate and can be skipped; CI is the one that cannot. Both
run the same rules, so a message the hook accepts is one CI accepts.

Install the hook once per clone:

```bash
git config core.hooksPath .githooks
```

CI checks the commits in the push or pull-request range, passed in
`RULES_RANGE`. A local run with no range checks whatever is not yet on
`origin/main`, which is the set the hook already saw.

## Naming a project is not naming an author

`openclaude`, `open-webui`, `gemini-cli`, `claude-code` and this repo's own
`.claude` directory are names it has to be able to write down: two of them are
on the ruled-out list in [`provenance.md`](provenance.md), and the reason has to
be writable. Those names
are blanked before the check runs, so a commit message may say why code was not
taken from one of them. What it may not do is credit one for the work.

## What this does not check

- **Issue and pull request bodies.** The rule covers them; CI cannot see them
  without a network call, and a check that needs the forge to be up is a check
  that fails for the wrong reason. This half stays a discipline, and the wayfinder
  map's own tickets are where it is visible.
- **Whether the signing key still belongs to Stefan.** Key rotation updates
  `.github/allowed_signers`, and old commits keep verifying only if the old key
  stays listed.
