# demido-catalog

The pinned manifest, and which archive a machine's accelerator asks for. It
fetches nothing.

## The one idea

**Pins ship inside the build, and the sizes beside them are measured.**

v2 read the release index and picked an archive out of whatever upstream had
that day. v3 does not: `docs/rules/setup.md` section 4 states what the first
set-up costs before a byte is spent, and a figure that came from a server
Demido asked at launch is a figure nobody measured. So the manifest is data, and
`tests/the_manifest.rs` reads section 4 and asserts every pin, download size,
size on disk and license against the row that measured it. A pin that moves
without a measurement moving with it fails the build.

That test is also why this crate has no seam for fetching: there is nothing here
that could be wrong in an interesting way. Getting the bytes onto disk and
owning them afterwards is `demido-runtimes`
([#46](https://github.com/elpideus/demido-studio/issues/46)).

## The three parts

| Part | What |
|---|---|
| `manifest.rs` | The pins, both sizes, and whose license each arrives under. Data. |
| `select.rs` | Pure: the archives one `Target` gets out of a list it is handed. |
| `selector.rs` | The accelerator row the wizard draws, over the manifest. |

`select` is handed its candidates rather than reading `MANIFEST`, which is how a
test can put the release's other CUDA builds in front of it without anything
being fetched. That seam is what proves the fix below.

## Invariants

- **The major version is the gate, and the minor version is not.** A 13.2 driver
  takes the 13.3 archive, not the 12.4 one. This is
  [#19](https://github.com/elpideus/demido-studio/issues/19)'s defect, fixed at
  the port: v2 compared the whole version and handed the rig 242 MiB instead of
  143 for a build it had measured loading every layer. The rule lives in
  `demido_hardware::CudaDriver::runs`, and
  `a_thirteen_two_driver_takes_the_thirteen_three_archive_not_the_twelve_four_one`
  is where it is asserted rather than described.
- **A companion is the runtime of the same toolkit, never of another.** The
  Windows CUDA build links against `cudart64_*.dll` and ships without it, and
  upstream's per-version bundles are not interchangeable.
- **No silent substitution in `select`.** A target whose accelerator has no
  loadable build is refused, with which of the two reasons it was. Falling back
  to CPU happens in `Selector`, in the open, on a row a person can see.
- **Every accelerator is a row, always.** `ACCELERATORS` is fixed, because a
  row's absence is a question nobody can ask about. What varies is
  `Availability`: offered with both sizes, a CUDA driver to install or update,
  or no build fetched yet.
- **The pre-selected row is still overridable.** `choose` returns `false` for a
  row carrying no build and changes nothing, so an override is never a broken
  install.
- **`Availability` and `Reason` carry facts, not sentences**, for the reason in
  [`demido-hardware`](../demido-hardware/AGENTS.md): the window owns the words.

## What is pinned, and what is deliberately not

CUDA 13.3 (with its `cudart` companion) and CPU, which is the whole of "CUDA and
CPU only in the first cut". ROCm and Vulkan are published upstream every release
and are not here: there is no AMD card on the rig, and shipping an untested path
is the built-but-not-working failure with a fresh coat on. They are rows saying
exactly that.

The CPU row's figures are this ticket's measurement (17.6 MiB down, 44.6 on
disk, 2026-09-09), taken the way #27 took the others: the download from the
response header, the size on disk from the archive's own index.

The capability group (uv, Python, SearXNG, Node, `agent-browser`, Chrome) is out
of S1. When it lands it is more rows in `MANIFEST`, not a second screen.

## Debugging

```text
cargo run --manifest-path src-tauri/Cargo.toml -p demido-catalog --example accelerator
```

Prints what was detected, what was pre-selected and why, every row with its
sizes, and the URLs the chosen row would fetch. On the rig that is CUDA at
515.5 MiB down and 671.6 on disk, which is section 4's required total.

## Tests

`cargo test --manifest-path src-tauri/Cargo.toml -p demido-catalog`. Nothing
here touches the network or the disk except `tests/the_manifest.rs`, which reads
one rules file.
