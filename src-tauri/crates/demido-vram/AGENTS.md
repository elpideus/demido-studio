# demido-vram

What the card has free, and whether one more generation slot fits in it.

## The one idea

**Parallelism is a budget, not a preference.** A sub-agent runs the
conversation's own weights on a second `llama.cpp` slot, never a second model,
so what it costs is a KV reservation. `--ctx-size` is per slot
([#19](https://github.com/elpideus/demido-studio/issues/19)), so parallelism
multiplies that reservation, and a number the card cannot honour has to be
answered before the driver is asked rather than after.

**The budget refuses, so the driver never has to.** A driver asked for memory it
does not have rarely errors. It obliges, by paging tensors through host memory,
and the app becomes twenty times slower with nothing in any log to say why. That
failure is undiagnosable from the symptom, which is why the accounting happens
before the allocation rather than around it.

## Two functions that do not know about each other

`admit` is **pure**: free bytes, what a slot costs, the slots already open, the
slots asked for. No card, no driver, no clock. That is what makes a machine
dependent number testable without the machine, and the table in `lib.rs` asserts
the rig's own measurements on any machine that can run `cargo test`.

`free_now` is the **real reading**, and it is a plain function beside the
arithmetic rather than a field inside it, because *the card's free memory is not
what a settings page saw when it was drawn*. A browser window opened between the
two moves the answer by more than a slot costs, so anything that opens a slot
reads again at the moment it opens one.

## Invariants

- **A slot that does not fit queues, with the reason stated.** Never an
  eviction, never a failed load, and never a preference shown back as though it
  had been honoured.
- **Nothing here frees anything**, and nothing here can. It is a set of numbers
  that says yes or no.
- **An unmeasured slot is refused, not guessed at.** `per_slot` of zero is
  `Queued::Unmeasured` rather than a slot that costs nothing. A slot admitted on
  a number nobody measured is exactly the paging failure above.
- **A card that could not be read is not a card with nothing free.** `free_now`
  answers `None`, and what a caller does with that is its own decision:
  `demido_chat::Pool` treats it as nothing to spare, which opens no new slot and
  closes none.
- **Every arithmetic path saturates.** An absurd context length in a settings
  file must refuse a slot, never wrap around into a small number that admits
  one.
- **Device 0, and never a sum.** Adding two cards' free memory together would
  admit a slot that fits in neither.

## NVML, not CUDA and not DXGI

`cuMemGetInfo` needs a CUDA context, and creating one on the device costs VRAM:
a reading that allocates is a reading that changes its own answer. DXGI's
`QueryVideoMemoryInfo` reports *this process's* usage against a budget, and the
memory that matters here is held by `llama-server`, which is a different
process. NVML needs no context, reports the whole device, and is what
`nvidia-smi` itself reads. It ships with the NVIDIA driver, so it is present on
exactly the machines whose free memory can be read.

`demido-hardware` loads the CUDA driver library for `cuDriverGetVersion`. This
crate does not depend on it: two different libraries, two different questions,
and neither keeps a handle for the other.

## What v2's `Ledger` got right, and what it did not

v2's `demido-vram` is the port ledger's one candidate drop
(`docs/research/port-ledger.md`): built, tested, and with no caller in its whole
life. So it was reviewed rather than ported, and three of its decisions carried
and one reversed.

Carried: refuse before the driver does; never evict; label where a number came
from rather than presenting a prediction as a measurement.

**Reversed: work that does not fit falls back to the primary, it never queues.**
That was the right answer to v2's question, where an auxiliary was a *second
model* and the fallback was the chat model doing the work less well. Here a
sub-agent already runs the conversation's own weights, so there is nothing to
fall back *to*: the only difference between the slot it wanted and the slot it
would fall back to is that the second one is busy. That is a queue, and calling
it a fallback would have been a name for waiting.

Not carried: the `Ledger` itself, the `ResidentId` handles, the roles, the
overcommitment report and the GGUF parser. A resident-model ledger is the
tiered-VRAM scheduler v2 wrote it for and nothing has asked for one; when
something does, it is an addition rather than the thing that was missing.

## Where the numbers come from

`per_slot` is measured, and nothing in this build measures one yet, so the only
admission the window can reach today is the default: one slot, which is the
conversation's own and is never the pool's to refuse. The producer is the fit
verdict ([#74](https://github.com/elpideus/demido-studio/issues/74)), which
reads the attention geometry a KV cache is sized from. That is also what will
answer the one ordering question this crate leaves open: `free_now` is read
before the weights land, so it is the card as it stands rather than as it will
stand, and pricing the load whole is what makes a pre-load reading answerable.

The rig's figures are `docs/rules/done.md`'s, and the tests here use them
verbatim. That includes both readings of the development model's slot, 563
derived out of a total and 620 weighed directly: asserting both is how the table
says the rule is about the arithmetic rather than about which reading of one
slot it was handed.

## Tests

`cargo test --manifest-path src-tauri/Cargo.toml -p demido-vram`. The arithmetic
needs no GPU by construction. Two cases call the real NVML and assert only that
the answer is coherent, which is the part no fixture can cover; on a machine
with no NVIDIA card they assert that it says nothing, which is also correct.
