# Driven live, or it isn't done

Decided on wayfinder ticket [#11](https://github.com/elpideus/demido-studio/issues/11).
`docs/rules/tiles.md` says how a piece of the codebase comes out; this says what
has to be true before any piece goes in, and what v0.1 is made of.

It exists because of the second of the three reasons v2 was set aside: it felt
built-but-not-working. v2's roadmap carries a formal ledger of features that were
built, unit tested and never once operated by a human. The Navigator had never
been opened. No model had ever called `create_artifact`. No permission prompt had
ever been read by somebody deciding. The code was there; the confidence was not.
v2 tried to fix that with an audit milestone late, and the debt outran the audit.
This file fixes it at the definition of done instead.

The bar is the brief's own:

> By properly "guiding" the llms, in a smart way that also does not consume too
> much context, it is possible to make even smaller models (like Qwen3.5 9B,
> Gemma 4 26B A4B and even Gemma 4 E4B it, all models which I will personally
> use during testing phase) behave "properly".

A feature nobody ever pointed a model at has not been tested against that
sentence. It has been tested against its author's idea of it.

## Two gates, and they fail differently

A ticket closes when **both** gates pass. Neither substitutes for the other,
because each is blind exactly where the other sees.

**The model gate.** A scenario in the live-model suite proves a real model,
shown Demido's tools in Demido's words, picked the right one and did something
useful with what came back. It runs from a terminal, with no window and nobody
at the keyboard, and it is **re-run every slice** thereafter. This is v2's
`demido-chat/tests/a_real_model.rs`: 1840 lines, twenty eight scenarios, all
driven, a process-wide permit so only one model is ever resident, and a ruleset
paid for in red runs. It ports.

**The window gate.** A person ran the feature once, in a running Demido window,
and attached a screenshot. It is **one-shot and never re-run**. It exists
because v2's own M12 note records four defects that driving the app turned up
and reading the code had not, among them an `@3xl:flex-row` sitting on the
wrong container, which meant the two-pane model browser **had never once laid
out in two panes** while every test was green.

A scenario without a screenshot is an unproven UI. A screenshot without a
scenario is a demo. Neither is done.

### The window gate has one known trap

Driving a Tauri window from outside needs `withGlobalTauri`, which Demido does
not set in anything shipped, and the way it fails is designed to waste a day.
Reproduced on [#19](https://github.com/elpideus/demido-studio/issues/19) against
v2's build, over CDP:

    typeof window.__TAURI__       = undefined
    typeof window.__TAURI_INTERNALS__ = object
    Object.keys(window) matching TAURI = []
    invoke through the global     = Cannot read properties of undefined (reading 'core')

The internals object is there and the public global is not, so the IPC channel
is fine and only the handle a driver reaches for is missing. Worse, the
internals are non-enumerable, so listing `window` reports no Tauri at all and a
healthy app looks like a broken one. v2 saw this as "Request timeout after
2000ms" from every webview tool while its Rust-side tools worked, which is the
same fault one layer up, and a missing `mcp-bridge:default` capability produces
the identical timeout. Two causes, one symptom, neither visible.

So the driver asserts the global at connect time and says which of the two is
missing, rather than timing out. A build made for evidence gets the relaxation
through a config merged only by the dev command, never through
`tauri.conf.json` or `capabilities/`, so a shipped build has neither the code
nor the permission.

A second trap sits beside it: a debug build loads `devUrl`, so running the
binary without the frontend dev server serves a blank page with the right window
title. Blank window, no error.

## What closes a ticket

The closing comment carries six fields and nothing else:

1. **Slice and ticket**: which slice this belongs to.
2. **Model, quant and `llama.cpp` SHA**: the exact three, because a red that
   turns out to be an upstream fix you did not have is a day spent on nothing.
3. **Scenario name**: the function in the live suite that proves it.
4. **Screenshot**: dragged into the comment, so GitHub hosts it and the clone
   stays small.
5. **Trace fixture path**: see below.
6. **`Bar:`**, either `chose` or `used`, matching the ticket's own line.

**The trace goes in the repo; the screenshot does not.** A screenshot is a
moment: it proves the window laid out once and it is never diffed. A trace is
the opposite. `design/windows.md` already requires the session log to be
sufficient to *rebuild* a prompt assembly rather than describe one, so a pruned,
deterministic trace is exactly a replayable fixture. It is committed beside the
scenario that produced it and becomes that scenario's input. Evidence with a
second job is evidence worth keeping; a folder of raw JSON exports is weight.

## The rig

The three primaries are already on this machine, in the layout
`demido-models/library.rs` already reads. The card is a **RTX 3060 with 12 GB**
and there is 32 GB of system RAM, so only one model is resident at a time and
the harness enforces that rather than the person typing the command.

They are **not interchangeable**, and pretending they are is how a quantisation
artifact gets recorded as a product defect:

| Role | Model | Quant | When it runs |
|---|---|---|---|
| Development | `gemma-4-E4B-it` | Q8_0, 7.6 GB | Every scenario, every iteration. Fully resident. |
| Reference | `gemma-4-26B-A4B-it-UD` | IQ2_M, 9.3 GB | Must be green before a slice closes. |
| Breadth | `Qwen3.8-27B-Uncensored` | IQ2_M, 9.9 GB | Once per slice. |

`Qwen3.5-9B` at Q4_K_M is secondary and runs when something is suspected to be
model-specific rather than product-specific.

**A red on the breadth model alone is a note, not a defect.** IQ2_M on a *dense*
27B is roughly 2.9 bits, and the first capability that degrades there is exactly
the one being measured: emitting a well-formed tool call. Green on the
development and reference models with a red on breadth is recorded as a model
note and nothing is changed, unless it reproduces at Q4_K_M. The reference model
tolerates the same quant far better because only about four billion parameters
are active per token.

That rule stands, but the failure it anticipates has not happened yet. On
[#19](https://github.com/elpideus/demido-studio/issues/19) all four models,
breadth included, emitted a well-formed call on every one of three probes, and
picked the right tool out of three offered. The rule is therefore a standing
allowance and not an expectation, and a breadth red is now surprising enough to
be worth a second look before it is written off as the quant.

## The pin

**`llama.cpp` is pinned and fetched, never built.** Hard rule 3 keeps a
third-party binary out of the installer, so the backend is the upstream release
archive: no compiler, no CUDA toolkit and no build on the user's machine. That
also makes the SHA in a closing comment mean something, because it names an
artifact anyone can download rather than one machine's build of it.

| | |
|---|---|
| Release | `b10816` |
| Commit | `427291b5b34cd914a31b3fd3b61a68f6184f4b9f` |
| Dated | 2026-09-05 |
| Archive | `llama-b10816-bin-win-cuda-13.3-x64.zip`, 143 MB |
| Runtime | `cudart-llama-bin-win-cuda-13.3-x64.zip`, 373 MB |

Two archives, not one: the build links against `cudart64_*.dll` and ships
without it. Half a gigabyte is what the guided set-up has to state before it
fetches anything, per Brief B11: "Guided set-up on first launch".

**A CUDA build one minor version above the driver still loads.** This driver
reports 13.2 and the 13.3 archive initialises CUDA and offloads normally. v2's
`demido-catalog` selects the newest build whose toolkit version is at or below
the driver's, which would hand this exact card the 12.4 archive: a 254 MB
download instead of 143 MB, for no reason. Minor-version compatibility is the
rule and the selector should follow it, with the major version as the real
gate. Carried into the port as a defect, not a preference.

## What the card holds

Measured on the pinned build, one slot, `-ngl 99`, prompt of 2048 and 128
generated. Idle desktop use of about 1.2 GB is included in the totals, because
it is real and the card does not get it back.

| Role | Layers | Weights | Total at 4k | Total at 32k | pp2048 | tg128 |
|---|---|---|---|---|---|---|
| Development, `gemma-4-E4B-it` Q8_0 | 43/43 | 4942 MiB | 6440 MiB | 6705 MiB | 2818 t/s | 48.0 t/s |
| Reference, `gemma-4-26B-A4B-it-UD` IQ2_M | 31/31 | 9536 MiB | 11368 MiB | 11865 MiB | 1749 t/s | 81.7 t/s |
| Breadth, `Qwen3.8-27B-Uncensored` IQ2_M | 66/66 | 9010 MiB | 10942 MiB | 12017 MiB | 485 t/s | 21.1 t/s |
| Secondary, `Qwen3.5-9B` Q4_K_M | 33/33 | 4861 MiB | 6474 MiB | not measured | 1735 t/s | 48.2 t/s |

Three things in that table are worth saying out loud.

**Context is nearly free and the weights are not.** Going from 4k to 32k costs
the reference model under 500 MiB. Every one of the three fits at 32k, so the
32k default this rig can afford is not the constraint anyone expected. What is
tight is the top of the table: breadth at 32k leaves 271 MiB on a 12 GB card,
which is less than one browser window, so the harness holding one model resident
is not tidiness, it is the only way that row loads at all.

**The reference model generates faster than the development model**, 81.7
against 48.0, because roughly four billion of its parameters are active per
token. The development model keeps its role on prompt processing, where it is
1.6 times faster, and on being the one with headroom to spare.

**`--ctx-size` is per slot, not per server.** With the default four slots,
`-c 4096` reserves 16k of KV and the reference model reaches 11787 MiB before
anyone has typed anything. Demido passes both `--parallel` and `--ctx-size`
already, so the number the user sees has to be the one they get.

## The bar: `chose` or `used`

Every slice ticket carries one line, `Bar: chose` or `Bar: used`.

- **`chose`**: the model reached for the thing with nothing in the prompt
  naming it. This is the default, and it is what the brief's thesis actually
  claims.
- **`used`**: the model was told, and did it correctly.

`used` is legal only when the ticket also records **why `chose` was rejected**
and what would raise it later. Left unguarded it re-creates v2's problem in a
nicer costume: a feature that works only when a human aims it.

The known case is delegation. v2 could never get a model to delegate to a
sub-agent without forcing it, and that is a model-capability problem rather than
a Demido one. So the bar splits rather than lowers: the delegation **returning
correctly** is `chose`; the model **electing to delegate at all** is `used`,
revisited once the guidance system that is the point of this product exists to
push on it.

## v0.1, in four slices

Each slice ends at something a model can be pointed at, and is cut by what a
model can newly *do* rather than by which component was touched. Vertical, never
broad.

### S1: a model answers in the window

A managed `llama.cpp` server that Demido starts and stops, one GGUF chosen from
the library already on disk, an answer streaming onto the desk, and the trace
recording the assembly that produced it. No tools. No downloader.

The event-sourced trace comes along here regardless of the Session Monitor,
because history is derived from it. What defers is the monitor's UI, not the log.

`Bar: used`, since there is nothing yet for a model to choose. The gate is
that the managed route carried a real conversation into a real window.

### S2: a model uses a tool, and you can see why

Tool calling through the backend's own grammar, and enough trace surface to read
the assembly that was sent. It ends the moment a model reads a file you planted,
answers from what was in it, and the answer is unguessable from anywhere else.

**The capability matrix ships here, not at S4**
([#20](https://github.com/elpideus/demido-studio/issues/20)). A tool call in
Cautious is an approval prompt, so the gate has to be real the first time a tool
runs at all. S4 adds delegation to it, not the modes themselves.

Two more scenarios ride on this slice, both from #20.

**A greeting.** With the full registry loaded, "hello" answers and calls no
tool, on all three tiers. This is the cost of there being no Chat mode, and it
is measured rather than assumed: #19's probe had all four models on disk pick
the right tool out of three, three probes out of three. Three tools is not
twelve. A tier that fails it is fixed by widening `tools.shown` to Demido's own
tools, or by the user switching a group off in the picker, both of which already
exist.

**A denial.** Escape on the approval, and the model does something else. This is
the path most likely to be broken and least likely to be exercised, because a
small model handed a refusal typically retries the same call forever.

`Bar: chose`, and on the denial it means it chose **something else**.

### S4: a model delegates

Sub-agents, scoped on the trace log rather than given a window of their own, per
[#8](https://github.com/elpideus/demido-studio/issues/8). The modes themselves
land at S2 (#20); what this slice adds is `delegate_task`, which declares
`Ability::Shell`, and the rule that a sub-agent inherits its parent's offered
set and mode and can only narrow them. The
brief's demand:

> Models should be able to delegate an agent to do a specific task in a separate
> clean context, for two main reasons: not filling up context with useless tool
> call outputs and such things, and so that it can be parallelized

`Bar:` split, as above.

### S3: a model you did not already have

The model browser in two panes, search, download with the size stated first, and
the downloaded file immediately loadable by S1's path.

`Bar: chose`.

**S3 runs last, after S4.** Four models are already on disk, so the downloader
buys convenience while S2 and S4 buy the thesis. It is also the surface v2's
invisible defect lived on, which is the argument against building it early and
driving it late.
