# demido-setup

What the guided set-up was told, what that leaves outstanding, and what the
answers together name as the thing to talk to.
[`docs/rules/setup.md`](../../../docs/rules/setup.md) is the contract; this
file says where each of its rules lives in code.

## The one idea

**Answers are remembered. Everything else is derived from disk.**

`answers.rs` holds choices a person made and nothing else. Whether `llama.cpp`
is installed, whether the model is still there, whether set-up is finished:
none of those is stored anywhere, because the runtimes ledger and the file
system already answer them, and a second copy of an answer is a copy that goes
stale. Section 1 says it plainly: what is outstanding is "derived from disk
rather than remembered, so it cannot go stale against what is actually there".

That is why there is no `finished` flag in this crate. A profile whose runtimes
folder was deleted has set-up outstanding again, and nothing had to be
un-remembered for that to become true.

The one remembered thing that is not a choice about the machine is
`Answers::dismissed`, and it is about a **gesture**: leaving is never final,
and without it leaving the wizard would reopen the wizard.

## The parts

| Part | What |
|---|---|
| `answers.rs` | The accelerator, the model folders, the model, which manifest rows are ticked, and whether the wizard was left. |
| `plan.rs` | `Situation` observed from the ledger and the disk, and the `Plan` derived from it with the answers. |
| `target.rs` | The binary and the model a turn needs, checked against the disk rather than trusted from the ledger. |
| `discover.rs` | The model folders already on the machine, and the GGUFs under them. |
| `store.rs`, `file.rs`, `contract.rs` | The seam for where the answers live, `setup.json` per profile, and that seam's contract suite. |

## Invariants

- **The plan is computed, never stored.** `Plan::of` takes a `Situation` and
  the answers and returns a list. There is nowhere to write one to.
- **`Situation::observe` reads the ledger rather than a boolean.** "Is the
  backend installed" is a question `demido-runtimes` already answers, and
  answering it twice is how two answers come apart. A **linked** row settles
  the step exactly as a **managed** one does: both run.
- **`target` checks the disk.** A row managed at a pin whose directory somebody
  deleted is no target. Returning one would make the composer say a model is
  loading and fail seconds later with a path in the message.
- **A capability is data.** `Answers::unticked` is a list of ids, so the
  capability group of section 4 is more entries in `demido_catalog::MANIFEST`
  and never a field per capability here. A row nobody has touched is ticked,
  which is what makes a row added later arrive on rather than off.
- **The accelerator is not stored until it is chosen.** `None` means nobody
  overrode detection, not CPU. Storing the detected answer would make a machine
  that grew a card keep answering with the machine it used to be.
- **Detection reports, it does not decide.** `discover` returns folders and
  files for a person to confirm. Nothing here starts reading from a folder
  because it found one, and nothing here writes, creates or moves anything.
- **A scan is bounded.** Four levels, five hundred files. Unbounded recursion
  under a folder somebody pointed at a drive root is a wizard step that hangs.
- **This crate has no words in it.** Every step, standing and reason is a
  variant; the window writes the sentence, for the reason
  [`demido-hardware`](../demido-hardware/AGENTS.md) gives.

## Why Ollama is not a candidate folder

It keeps its weights as content-addressed blobs with no extension and a
manifest beside them, so a `.gguf` scan finds nothing there. A row pre-filled
with it would read as an empty folder rather than as the unsupported layout it
is. LM Studio, which the brief names, is the first candidate.

## What is out of this pass

No fetching, no process, no network: that is `demido-runtimes`. No Tauri: the
commands are `src-tauri/src/setup.rs`, which is also the only place that turns
these facts into sentences.

## Tests

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-setup
```

Everything touches only a scratch directory under the OS temp folder.
`contract.rs` is the seam's suite and `file.rs` runs it, so a second store
implementation has one function to pass.
