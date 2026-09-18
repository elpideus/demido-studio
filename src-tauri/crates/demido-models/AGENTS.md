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
| `index.rs` | What Hugging Face publishes, keyless: the search, a repository's files, and the two pure parsers under them. |
| `quant.rs` | A quantisation label, read off a filename into a value ordered by fidelity, with llama.cpp's published bits per weight. |
| `choices.rs` | A repository's files, read into what a person can choose: one choice per model, its shards as pieces, its projector beside it. |

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
- **Only `index.rs` touches the network, and nothing here downloads.** The
  queue (#73) fetches weights. The library is the part of the models surface a
  network failure must never take away, and the index is built so it cannot:
  every call answers with an `Answer`: `Read`, or `Unreadable` with a `Cause`,
  never an `Err`, so a dead connection is a state the browser renders beside
  the installed models.
- **The index parses bytes, and only bytes** (#70). `parse_listing` and
  `parse_files` are pure; `Index` is the one place a request is made, and it
  hands them the body. A payload that confuses the browser becomes a fixture
  in `tests/fixtures/index/`, captured with `curl` and never edited.
- **Publisher fields are passed through verbatim.** `pipeline`, `library` and
  every tag in the order sent. `Repo` has no field a capability could be
  inferred into, and a test pins its keys.
- **Keyless, and it says who is asking.** No `Authorization`, no cookie, and a
  `User-Agent` naming the app, its version and this repository.
- **Gating is read at listing time.** The search asks for `gated` by name
  (without `expand` the listing leaves it out), and anything but `false` is a
  gate. A gated repository's tree still lists, with its digests masked, which
  reads as no digest rather than as sixty-four asterisks to verify against.
- **Capped and ordered at the parser**, not only in the query string: most
  downloaded first, at most `LISTING`, whatever the server honoured.
- **A file list is every page of it.** The tree is paged by a `Link` header;
  every `rel="next"` on the same host is followed, up to `PAGES`, and a tree
  longer than that is `Unreadable` rather than a short list passed off as the
  whole repository.
- **Nothing guesses a quantisation** (#71). A label llama.cpp publishes a
  number for carries that number and upstream's spelling; any other label
  keeps the file's spelling exactly, carries no number, and sorts after every
  label that has one. `tests/quantisations.rs` holds the table a second time,
  with its source, and fails if the two disagree.
- **Only weights are a choice, in the index as on disk.** `choices` offers one
  choice per model, a split model's shards as its pieces in order, and fetches
  the projector the weights need beside them. A projector or a draft is never
  a choice, and a split the listing lacks a piece of is not offered.
- **The size is the server's.** A choice's `bytes` is every piece and its
  projector as the tree states them, added up, never bits per weight times a
  parameter count.
- **A scan never fails.** A missing or unreadable folder contributes nothing;
  startup never blocks, and a drive that is not plugged in is not a broken
  library. Bounded at four folders deep and 500 models.

## Judgement calls, recorded rather than asked

- **A private repository and an absent one read the same.** Keyless, Hugging
  Face answers both with `401`, so both are `Cause::Missing`; telling them
  apart would need the account this index refuses to ask for.
- **A repository id is checked before it becomes a URL.** Anything that is not
  `owner/name` in Hugging Face's characters is `Missing` without a request, so
  an id can never become some other path.
- **Non-GGUF files are dropped by the file parser**, importance matrices
  included even though they sit in LFS. A file llama.cpp cannot load is not a
  choice.
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
- **Bits per weight are the README's measured row where it has one.**
  llama.cpp's `tools/quantize/README.md` measures one model, so those numbers
  compare with each other and include the tensors a recipe keeps wider. Where
  it has no row (`Q4_0`, `BF16`, the ternary types) the number is the one
  `quantize.cpp` states or ggml's block layout fixes, which is the type's width
  alone. Both are published; neither is estimated.
- **`UD` in front of a label makes the label the publisher's.** Unsloth
  Dynamic requantises layers the label does not describe, so `UD-IQ1_M` gets
  no number even though `IQ1_M` has one. `quant::RECIPES` is the list; a
  publisher's word nobody has added there reads as part of the model's name,
  and the label after it keeps its number.
- **A projector named `model` is the folder's.** `mmproj-model-f16` is what
  llama.cpp's converter writes when nobody names it, and `ggml-org` and
  `google` publish it so. Read that way on disk and in the index alike.
- **One projector per choice, `F16` first.** A repository publishing a
  projector at three precisions needs one of them fetched: `F16`, then `BF16`,
  then `F32`, then any other. A draft is not fetched at all; it makes a model
  faster, and nothing yet loads one.
- **Reasoning is a thinking block in the template.** `<think>`,
  `enable_thinking`, `reasoning_content` or `thinking`. Every thinking template
  on the rig matches; a template that thinks under some other word reads `No`,
  which is the cost of not guessing from the name.

## Tests

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-models
```

`tests/index.rs` parses the committed payloads and asks a server on
`127.0.0.1` that answers the way Hugging Face can, including the `401` it sends
for a repository that is not there. `tests/against_hugging_face.rs` asks the
real host and is `--ignored`:

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-models --test against_hugging_face -- --ignored
```

`tests/quantisations.rs` maps every published label to its bits and every
unpublished one to its spelling and last place. `tests/choices.rs` reads the
same committed payloads into choices: the ordered list, the split model, the
projector, and the sizes the server sent.

`tests/library.rs` writes real GGUF bytes into real temporary folders
(`tests/support/mod.rs`) and reads them back through the calls the window makes,
because borrowing, part classification and the directory-as-registry rule are
only interesting against real paths.
