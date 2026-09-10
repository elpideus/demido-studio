# demido-hardware

What this machine can run: the adapters DXGI reports, what CUDA the installed
driver runs, and the accelerator those two facts indicate.

## The one idea

**Detection reports, it does not decide, and it does not speak.**

*Reports*: this crate says what is present and what that points at. Which
archive it turns into is [`demido-catalog`](../demido-catalog/AGENTS.md)'s, the
choice stays the user's, and it is settled by a model actually loading rather
than by anything asserted here. A detector that guesses right nine times and
lies confidently the tenth is worse than one that reports what it saw.

*Does not speak*: every reason and every note is a variant, never a sentence.
The window owns the words, `docs/rules/setup.md` section 3 owns what they say,
and a crate with no prose in it cannot drift from them. `Reason::CudaDriver`
carries the adapter and the version; the row that renders it writes
"NVIDIA GeForce RTX 3060, driver reports CUDA 13.2".

## Invariants

- **Nothing here fails startup.** A failed probe is a `Note`, not an error, and
  every one of them still leaves `preselection()` answering.
- **Software adapters are excluded.** Windows' Basic Render Driver is not a GPU,
  and offering it sends a user down a path that cannot work.
- **`primary()` is the largest card, not the first.** An integrated adapter at
  index 0 with a discrete card at index 1 is the common case. This machine is
  one.
- **The CUDA driver is asked, not inferred.** `cuDriverGetVersion` reports the
  highest CUDA a driver runs, which is the only thing that decides which build
  loads. An NVIDIA card is not evidence of a CUDA driver, and a driver version
  is not a CUDA version.
- **An NVIDIA card whose driver runs no CUDA pre-selects CPU**, and the reason
  says which. Still reporting: what is reported is that the driver said no.
- **What an AMD card indicates is ROCm**, whether or not a ROCm build exists.
  Availability is the manifest's answer, not detection's, and keeping the two
  apart is what lets the selector draw an honest empty row.
- **Dedicated and shared memory are reported separately, never summed.** Shared
  memory is host RAM the driver may lend the card: real, and far too slow to
  plan a model around.

## Windows, and DXGI only

`probe.rs` calls DXGI directly. No vendor tool has to be installed, every
adapter is enumerated including integrated ones, and dedicated and shared memory
come back separately. `nvidia-smi` fails on machines without it and sees nothing
that is not NVIDIA; `wmic` is deprecated. `docs/rules/setup.md` section 3 records
this as a rewrite of v2's probe rather than a port, for that reason.

Other platforms report an empty list and `Note::NotProbed`. Cross-platform
detection lands with cross-platform support at 1.0 (Brief B03), and the CPU row
works meanwhile, which is what keeps startup unblocked.

`Ecosystem` has four variants and no `Metal`: nobody on a supported platform
could pick it, and it is one line the day macOS is a target.

## Debugging

The probe is printed by `demido-catalog`'s example, beside the choice made from
it, because the two failures a report confuses are "the machine was read wrong"
and "the row chosen from it is wrong":

```text
cargo run --manifest-path src-tauri/Cargo.toml -p demido-catalog --example accelerator
```

## Tests

`cargo test --manifest-path src-tauri/Cargo.toml -p demido-hardware`. The logic
is asserted against constructed machines, so it needs no particular card. Two
tests call the real probe and the real driver and assert only that they answer,
which is the part no fixture can cover.
