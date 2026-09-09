# demido-runtimes

Fetches what `demido-catalog` selects, verifies it, and owns it afterwards.
[`docs/rules/runtimes.md`](../../../docs/rules/runtimes.md) is the contract;
this file says where each of its rules lives in code.

## The one idea

**The state decides everything, never a path comparison.** A row is
`Managed`, `Linked` or `Absent` (`state.rs`), and the type makes the wrong
action inexpressible rather than merely refused: a `Linked` row carries a path
and nothing to sum, so no code path can count its bytes in the disk total or
delete them, and there is nothing to remember to filter at a call site.

## The parts

| Part | What |
|---|---|
| `fetch.rs` | Gets bytes onto disk. Resume is the `.part` file already there and nothing else: no marker file, no HTTP seam. |
| `unpack.rs` | Expands a zip, streaming. A build and its `cudart` companion merge into one directory, because that is how `llama-server` resolves its DLL. |
| `verify.rs` | The declared command, and the table of what each row declares. For the required group: start `llama.cpp`, load a model, generate one token, stop. |
| `manage.rs` | The state machine, and the only module that deletes anything. |
| `state.rs`, `store.rs`, `contract.rs`, `file.rs` | The ledger: three states, the seam for where it lives, that seam's contract suite, and `runtimes.json` per profile beside `settings.json`. |

## Invariants

- **The required group is data.** `RuntimeId` is a string and `verify::REQUIRED`
  is a table, so the capability group (uv, Python, SearXNG, Node,
  `agent-browser`, Chrome) is more entries, never a second `match` and never a
  second screen.
- **Verify then delete, in one call.** `Runtimes::fetch_row` deletes the
  superseded directory only after verification returns, and before the ledger
  write, so there is never a window where both pins or neither are on disk.
- **A refusal never costs a user what already worked.** An update that does not
  verify leaves the previous pin exactly where it was and the row still managed
  at it; only a **first** fetch with nothing behind it becomes absent with a
  reason. A link that does not verify never becomes linked, and never clears a
  managed row either. `Outcome::Refused` is how the caller hears about it: a
  refusal is an ordinary answer, not an `Err`, and returning `Ok(())` for it
  would make "it arrived and does not work" look like "it is ready".
- **A linked row is verified when it is pointed at**, by the same command a
  fetch runs, which is section 10 and section 2 being the same rule.
- **Everything deleted goes through `Runtimes::inside`.** `runtimes.json` is a
  file a person can open, so a row naming `../../.agent-browser/browsers` is
  refused rather than obeyed. That guard is what makes "Demido prunes no cache
  it did not fill" true of the code rather than true by luck.
- **`unused()` is computed every call**, by diffing the folder against what the
  ledger currently claims, never cached as a list. Files count as well as
  directories: a cancelled fetch's `.part` is bytes nothing names, and 373 MiB
  of half a `cudart` that no screen mentions is the same silence with a smaller
  number.
- **This crate contains no URL.** Every address comes through
  `Fetchable::url`, from a pin that shipped inside the build or a path the user
  pointed at, which is how "Demido never queries upstream for a version, on any
  trigger" is enforced rather than promised.
  `tests/no_upstream_lookup.rs` reads the crate's own source and fails on a
  literal or a release feed by name.
- **No seam for the network or for verification.** Both are real process, real
  socket and real card behaviour, and the ticket refuses a mock for them in as
  many words. `manage::VerifyFn` is a plain closure held on `Runtimes`, which
  is enough to test the rules above without a card, and is set once at the
  composition root.

## Why this depends on `demido-inference`

The declared verification for the required group is "loads the smallest model
already on disk and generates one token", and that is exactly what
`demido_inference::Backend` does. Writing a second `llama.cpp` client here to
keep the crate boundary tidy would mean verifying something other than the
thing Demido runs, which is the failure the rule exists to prevent.

## What is out of this pass

No Tauri wiring and no Runtimes settings page. `demido-catalog` (#45) landed
the same way, as a library with no caller in `src-tauri/src` yet. What that
page will need is already exposed: a `Serialize`-able `Ledger`, a progress
callback, `unused()`, `link()`, `remove()` and `remove_unused()`.

`docs/rules/setup.md` section 4's third required member, one model, has no pin
("the user's choice", "varies"), so nothing here fetches one yet.
`fetch_row` is generic over `fetch::Fetchable` rather than over
`demido_catalog::Archive`, so a model with its own URL goes down this same path
when the models surface exists, instead of a second one being built for it.

## Tests

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-runtimes
```

Everything above touches only a scratch directory under the OS temp folder.
`tests/resume_and_cancel.rs` and `tests/verify_then_delete.rs` run the real
fetcher against a real socket on `127.0.0.1`, which is how resume, cancel and
verify-then-delete are proved without a mock and without the network.

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-runtimes -- --ignored --nocapture
```

`tests/a_real_fetch.rs`: the pinned archive off GitHub, unpacked, and verified
by starting it against a model. This is the hand-run the ticket asks for at the
window gate, including the cancel-and-resume path. It fails rather than skips
when the rig is absent, matching `demido-inference/tests/rig.rs`.
