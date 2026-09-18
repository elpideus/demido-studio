# demido-models

The models on this machine: the library, its folders, and what each file is.

Brief B55: "Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks."

## The one idea

**The directory is the registry.** Carried over from v2's decision 0007. A
file at its destination is the record that it was downloaded, so there is no
bookkeeping file: a model deleted by another tool disappears on the next scan
rather than lingering, and a file copied in by hand appears on the next scan
without being registered. A download still in flight is not a `.gguf` and is
not listed, because it is not there yet.

## The parts

| Part | What |
|---|---|
| `folders.rs` | The download folder and the scan folders, resolved from the ladder, and what moving the download folder does to them. Pure. |
| `sources.rs` | Where LM Studio, the Hugging Face cache, Jan and GPT4All keep models, what detection finds, and what a borrowed row is called. |
| `library.rs` | The scan, the removal that refuses, where a download lands, and the disk Demido spent. |
| `parts.rs` | Weights, projector, draft, and the pieces of a split model, by filename. |
| `gguf.rs` | The header: the facts a file states and whether the file is as long as it says. |

## Invariants

- **Borrowed folders are never written to, downloaded into or deleted from.**
  Kept by `Library::remove` refusing every path outside Demido's own root on
  the *resolved* path (so `models\..\elsewhere` and a junction out of the root
  are both outside), and refusing a borrowed folder even when it sits inside
  the root. `Library::destination` has no argument that lands outside the root:
  every segment is reduced to a plain name. `tests/library.rs` asserts both
  directly, and snapshots a borrowed folder before and after to prove nothing
  touched it.
- **A model is verified before it is offered.** Every weights file's header is
  read and the file checked to be as long as its tensor table says. A file cut
  short, a login page saved as `.gguf`, or a split model with a piece missing is
  a `Damaged` row with the reason, never a `Local`. The type sizes are ggml's
  block structs; a type the table does not know counts as one byte, which is a
  lower bound rather than a guess.
- **Borrowed bytes are never disk Demido spent.** `Scan::spent` walks the own
  root only, skipping any borrowed folder nested in it. `docs/rules/runtimes.md`'s
  managed-against-linked rule, applied to weights.
- **Capabilities on disk are read from the file** (#36's scoping of
  `design/shell.md`'s tags). Vision is a projector beside the weights that does
  not say it has no vision encoder; audio is a projector that says it has an
  audio encoder; tools and reasoning are what the GGUF's own chat template
  renders. No template is `Unknown`, which the window draws differently from
  `No`. The filename is never read for a capability.
- **Nothing here downloads and nothing here touches the network.** The index
  (#70) and the queue (#73) fetch. This is the part of the models surface a
  network failure must never take away.
- **A scan never fails.** A missing or unreadable folder contributes nothing;
  startup never blocks, and a drive that is not plugged in is not a broken
  library. Bounded at four folders deep and 500 models.

## Judgement calls, recorded rather than asked

- **Moving the download folder makes the old one borrowed.** Nothing moves,
  the old folder becomes a scan folder so every model in it is still offered,
  and from then on Demido will not delete from it. That is the honest reading
  of a setting change that did not take the files with it; a person who wants
  them to be Demido's again moves the download folder back.
- **A download folder on or inside a scan folder is refused**
  (`Folders::borrowed_at`). Downloading there would be writing into a borrowed
  folder. A household sharing one folder of weights (`docs/rules/profiles.md`)
  removes it from the scan folders first, which is saying out loud that Demido
  may write there.
- **The closer folder decides whose a file is.** A borrowed folder inside the
  download folder keeps its files borrowed; a borrowed folder that contains the
  download folder (a whole drive) does not claim Demido's downloads.
- **Moving the download folder takes the old folder's bytes out of the spent
  figure.** They become a scan folder's bytes, and the figure is what the
  current folder holds. Moving it back brings them back.
- **Tools is the template naming `tools`.** A substring, so a template that
  mentions the word without rendering a list would read `Yes`; none on the rig
  does, and the alternative is evaluating Jinja.
- **Unset scan folders are what detection finds, every time they are read**,
  until somebody edits the list. An empty list is an answer (they removed them
  all) and is never re-seeded.
- **Reasoning is a thinking block in the template.** `<think>`,
  `enable_thinking`, `reasoning_content` or `thinking`. Every thinking template
  on the rig matches; a template that thinks under some other word reads `No`,
  which is the cost of not guessing from the name.

## Tests

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-models
```

`tests/library.rs` writes real GGUF bytes into real temporary folders
(`tests/support/mod.rs`) and reads them back through the calls the window makes,
because borrowing, part classification and the directory-as-registry rule are
only interesting against real paths.
