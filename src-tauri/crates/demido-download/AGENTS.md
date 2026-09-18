# demido-download

The download queue: a model's files onto this machine, resumable, verified, and
a failure that says what happened.

Brief B22:

> UI should also have a download status indicator which also allows to pause, resume or cancel model downloads (especially useful when adding multiple models to the download queue).

Carried from v2's `demido-download` on
[#73](https://github.com/elpideus/demido-studio/issues/73), with what the queue
there did not have: an item is a model rather than a file, a failure is a cause
rather than a sentence, the queue outlives the process, a cancel deletes, the
disk is asked before the transfer, and a finished file is checked to be a GGUF.

## The parts

| Part | What |
|---|---|
| `lib.rs` | `Item` (one model: its pieces and projector), `Piece` (one file) and `Failure` (why an item stopped). |
| `queue.rs` | The pool, the rows, `pause`, `pause_all`, `resume` (which is also the retry) and `cancel`. |
| `transfer.rs` | One file: range negotiation, the write, the digest and the GGUF check, the rename. |
| `file.rs` | `downloads.json` in the profile: intent, never progress. |
| `room.rs` | Free space on the download folder's volume. |

## Invariants

- **The library never holds a partial file.** Bytes accumulate in
  `<name>.gguf.part`, which `demido-models` does not list. An item's files are
  renamed into place only once every one of them is whole and verified, and the
  weights' first piece goes last, so the file a backend is handed appears only
  when everything it needs is beside it.
- **Verified before offered.** A file is the length the index listed, hashes to
  the LFS digest where the index published one, and passes
  `demido_models::gguf::verify`. A file that fails is deleted: resuming from it
  would fail at the same byte forever.
- **The file records intent; the bytes on disk record progress.** An entry in
  `downloads.json` is an item and whether it was paused or failed. How far it
  got is the length of its partial files, read again by `Queue::open`. A
  finished item leaves the file, because the library is its record.
- **One failure is one failure.** A failed item gives its slot back and the
  others carry on. `resume` on it is the retry, and it asks for the bytes still
  missing.
- **A failure has a cause.** `Failure` tells a reset connection, a truncated
  body, a full disk, no room up front, a gated file, a missing one, a rate
  limit, a stall, a wrong length, a login page, a digest mismatch and a file
  that is not a model apart. The window writes the sentence.
- **A cancel deletes the partial files** and takes the row out, including a
  paused or failed item's. The task deletes them after it lets go of them,
  because Windows will not delete an open file. A finished item's row is
  dismissed and its model left alone: deleting a model is not the queue's.
- **Free space is asked before a byte is requested**, against what is still to
  come rather than the whole item, so a resume is not refused for bytes already
  on disk.
- **The index's size is enforced.** A `Content-Length` or `Content-Range` that
  disagrees with it is refused before anything is written, and a body that runs
  past it is stopped. A `200` to a ranged request is written from zero, never
  appended.
- **One target, one item.** Queueing a model whose first piece is already in
  the queue returns that row, and resumes it if it was held.
- **Opening a queue starts nothing and writes nothing.** `Queue::start`, called
  once inside the runtime, picks up whatever nobody paused.

## Judgement calls, recorded rather than asked

- **Two at a time.** v2 ran three archives of a few hundred megabytes; a model
  is gigabytes, and two keeps a home link full with bars that still move.
- **A pause survives a restart; so does a failure.** A row that failed says why
  on the next launch rather than retrying by itself, because a gated file or a
  full disk will fail the same way again.
- **`text/html` is refused on sight.** A login page is a success with a body,
  and it is the one body that is never a model. A page that hides its type is
  still caught, by the length or by the GGUF check.
- **A reset is an I/O `ConnectionReset` or `ConnectionAborted` somewhere under
  the body error.** Any other body error, and a body that ends short, is
  `EndedEarly`. Both keep their bytes.
- **Free space is Windows only.** Elsewhere nothing is refused up front, and a
  full disk is still named when the write fails.

## No new trait

There is one host and one way to fetch a file over HTTP, and no second
implementation is coming, so there is no seam and no contract suite
([`docs/rules/tiles.md`](../../../docs/rules/tiles.md)). The interesting
behaviour is at the HTTP boundary, and the tests meet it there.

## Tests

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-download
```

`tests/over_http.rs` runs the queue against a server on `127.0.0.1` that
misbehaves on purpose: a truncated body, a reset mid-body, a server ignoring
`Range`, a wrong `Content-Length`, a redirect to an HTML login page named as a
`.gguf`, a `401`, a `404`, a silent connection. Bodies are real GGUF bytes and
assertions are about files on disk and requests on the wire: what a retry asked
for, what a cancel left, what a restart picked up.

`tests/against_hugging_face.rs` is the real host, `--ignored`: the smallest
quantisation of `unsloth/Qwen3-0.6B-GGUF` (214 MB) through a pause, verified
against its published digest and then offered by the library, and a gated
repository failing as gated.

```text
cargo test --manifest-path src-tauri/Cargo.toml -p demido-download --test against_hugging_face -- --ignored --test-threads=1
```
