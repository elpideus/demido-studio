# The windows

Everything that opens over the desk. `shell.md` decides what a panel *is*; this file decides what
each panel *shows*, and it is the second half of the same argument.

Settled on [#8](https://github.com/elpideus/demido-studio/issues/8), across four revisions with
Stefan. The board that argued for it is `prototypes/canvas-board.html` in the project folder, twelve
screens at 1280×800; it is throwaway and is not in this repo.

Read `shell.md` first. Where the two disagree, they do not: this file amends `shell.md` in two
places and those amendments are written there, not here.

## The session monitor

The brief asks for a log that is inspectable **by source**, likes DeepSeek's idea, and dislikes their
UI. v2 built this three times and its own file header is honest about the first two:

> a place you went, then a 256 pixel rail column, and both times it was the same flat list of event
> kinds behind a sequence number

The third attempt got three things right. They are kept, unchanged:

- **Lanes.** Input, model, tools and agents across the top, one block per event. The shape of a run
  reads before a word does. Block width is elapsed time, block height is token weight.
- **A stream grouped into turns**, with chunk runs folded into the message they assembled.
- **A detail pane whose last tab is the raw JSON**, because the log is the record.

### The second axis is cost

v2's log says what happened and never what it cost. The brief's other thesis is getting small models
to behave *without* eating their context, so:

- every row carries its **token weight**, as a number and a bar;
- the header carries an **occupancy bar** — 21.7k of 32k, segmented by which source is holding it;
- the source column is a **ledger**, count and tokens per source, not a legend beside a search box.

**This constrains `demido-trace`, and the constraint is the point of writing it down: token weight
must be recorded per event, not per request.** A monitor that cannot say what a skill cost cannot
serve the thesis the product is built on.

### The eight sources

Colour-coded by source, and every row carries its source's icon as well, because colour is never the
only signal. The tokens are already in `tokens.css`: `--src-system`, `--src-skill`, `--src-user`,
`--src-inject`, `--src-reasoning`, `--src-tool`, `--src-artifact`, `--src-error`.

### What the model actually saw

> everything that happens, from the input getting sent to the model using the tools available up
> until the reply … should be visible. All prompts should be editable.

A list of events is not that. Selecting an event rebuilds the prompt **as it stood at that moment**,
block by block, **diffed against the previous assembly** — an injection appears as an inserted block
you can read, an evicted one as a struck-out block with its cost. Each block header carries an edit
affordance, which is where "all prompts should be editable" lands, and **Replay from here** is what
makes editing worth doing.

**This constrains `demido-trace` a second time, harder: the event stream must be sufficient to
rebuild an assembly, not merely to describe one.** It also settles chat export — export is a
projection of this, not a second exporter, exactly as the brief guesses.

### Fork and replay live on the row

Forking is something you do *at* a moment, so it belongs at that moment, beside replay-from-here and
export-selection. The header keeps the whole-session forms.

### Sub-agents are a scope on this log, not a window

**Amends `shell.md`'s rail order and the brief's own rail listing**: there is no Sub-agents window
and no Sub-agents rail icon.

The monitor's left column carries agents on top and the source ledger beneath. **Selecting an agent
filters the stream, the ledger, the lanes and the occupancy bar** to that agent's events; `Main
session` is the way back to the unscoped run. Delegation depth is the indent.

The two numbers the brief asks for live in that column's header, because they govern the run it
describes:

> User should be able to manually set the amount of parallel agents they want to run at the same
> time, and how deep agents can delegate one another

Parallelism is a slot strip — filled, queued, free — so "why is nothing happening" is answered on
screen. A delegation refused at the depth limit is a row with a stated reason, and the refusal is
handed to the agent **in its own context**, so it answers from what it has instead of stalling.

Two windows over one event stream is two windows that can eventually disagree about the run. One
cannot, and not disagreeing is the whole product claim.

## Settings

**The main Settings window holds global values and nothing else.** No scope picker, no layer chip,
no resolution to read, because there is only one layer there.

A model, a chat and a character each get their **own settings section**, and a control in one of
those is in exactly one of two states:

| State | Reads | Carries |
|---|---|---|
| Following global | the inherited value, in `--color-ink-4` | nothing |
| Overridden | the local value, row marked with `--color-signal` | the way back to global |

You always know what you are editing, because it is the thing you are standing in.

An earlier draft put a `GLOBAL · MODEL · CHAT · CHAR` provenance chip on every control and made the
chip the editor. That is v2's mistake, and Stefan named it as what made v2's settings confusing. The
one thing the chip bought — answering "why is this 0.4" — is the Navigator's job now, which lists
global and per-chat settings as separate rows that each say where they live.

The character layer needs no placeholder. When the character system lands it is another section, and
nothing else changes shape.

### Keys

The binding **is** the field: click the keys, press the chord, and a conflict is shown in place
against the command it collides with, never in a modal. Vivaldi's model, which the brief names.

The column Vivaldi does not have and Demido needs is **scope** — `global`, `desk`, `panel`. Scoping
is what lets `Enter` approve a tool call without stealing `Enter` from the composer. Global beats
panel beats desk. Two commands may share a chord in different scopes and the row says so rather than
calling it a conflict.

## Projects

Name, description, icon picker with custom PNG and SVG on the face of it, attached files and folders,
and the project's chats — one page, beside the chat list with its All / Chats / Projects filter.

One rule beyond the brief: **an attached folder states what the model will actually see** — file
count, total size, and the exclusions applied. "Connect a project to a folder" silently meaning "and
skip `node_modules`" is the invisible behaviour this project exists to refuse. A folder still being
scanned says so as a row, never as a blocking dialog.

## Market charts

One price axis. Never two. Timeframes, a symbol search that names which account or exchange answers,
a crosshair and a tooltip. The last price is the only direct label.

No indicators, no drawing tools, **and no volume pane** — volume is an indicator and the brief
postpones all of them.

Candles are **filled in both directions**, green up and violet down. The colour change is recorded in
`tokens.css`: rose against green failed colourblind separation at deutan ΔE 4.6 against a floor of 8,
violet against green measures 17.0, and a pair that separates on its own needs no hollow bodies.
Per-user chart colours are later work.

## The Navigator

The F1 panel. **One ranked list**, with the group carried on the row as a category chip. Four fixed
group headings make the eye check four places for the best match; ranking puts the best match first
and the chip says what kind of thing it is.

**With nothing typed, chats come first** and everything else ranks after them. This is a chat
application, and a settings row taking the top slot because it matched a character better is the
Navigator being clever at the user's expense.

Prefixes scope the list when you already know:

| Prefix | Scope |
|---|---|
| `@` | files |
| `>` | settings **and** commands |
| `!` | commands only |
| *(none)* | chats first, then everything else |

`>` covering both is deliberate: from the user's side "change the temperature" and "reset the
temperature" are the same errand. `!` is there for when the verb is what you want.

Typo tolerance is one mechanism with ranking, not a separate pass. A chat previews its last exchange;
a setting previews its control. Opening a setting closes the panel, opens Settings on the right page,
scrolls to the row and lights it briefly — "android style", in the brief's words.

## The code graph

**graphify's own UI, skinned, driven with no server.** Demido implements no graph viewer.

v2 wrapped graphify as a sidecar (ADR 0019) and then built its own viewer anyway — `Graph.tsx`,
`Picture.tsx`, `Diagram.tsx`, roughly 45 kB of force-directed picture, clustering and neighbour
lists. That is not written again. The mechanism below is v1's, in
`demido-studio-first-version/src-tauri/src/local/graphify.rs`, and it works:

1. **Build is a subprocess, in two stages, both required.**
   `python -m graphify <folder> --code-only [--update]`, then `cluster-only <folder>`. Extract alone
   writes `graph.json`, exits 0, and **does not write `graph.html`** — v1 shipped that broken once.
   `--code-only` is load-bearing: without it graphify demands an LLM key for any repo with a README.
2. **The viewer is a static file**, handed to an iframe as `srcdoc`. **No server, no port, nothing to
   capture.** Before it is handed over, the vis-network `<script src>` is replaced with an inline copy
   of the bundle cached once into app data, so the graph renders on a machine that has never been
   online.
3. **The skin is a `<style>` appended after graphify's own**, so equal specificity wins by cascade
   order. No fork, no patched dependency.
4. **Node positions are cached** per folder and replayed with physics disabled, so a reopened graph
   paints instead of settling for two seconds.

**The skin touches chrome only.** v1's rule, and it is right: graphify's per-node colours are
community-coded **data**. Body, sidebar, search, node info, communities, checkboxes and scrollbars
become Karl; the node hues stay graphify's, because recolouring them would be recolouring the answer.

**The model's tools need no server either.** `python -m graphify query | path | explain`, cwd set to
the folder, stdout captured — the same bargain as the build. MCP over stdio is available if the MCP
shape is ever wanted; an MCP server on a port is not.

Standing debt, recorded: the skin is coupled to graphify's own selectors, so a release of theirs can
un-style the panel.

## The browser

The design problem is not the browser, it is that two parties drive it. So the panel **says who is
driving, in words**, at the bottom of the page where the action is, in `--color-violet` — the
delegated-work colour — and names the model's last action. **Take over** is one click and one key.

Every click the model makes is a tool call in the session log, like any other.

## Nothing is pre-installed

> the important thing about Demido is that user will not be required to have anything pre-installed
> on their system, Demido should handle everything itself. That includes graphify.

Already decided in v2's ADR 0033, *"Demido fetches uv, rather than asking for it"*, for the same
reason: the app already fetches a llama.cpp build, a multi-gigabyte model, a Chromium and a hundred
megabytes of SearXNG without asking, and then stopped at a twenty megabyte binary to tell somebody to
set up a Python package manager. The people this is for are precisely the people who do not have uv.

The ladder for the code graph, and the shape every provisioning surface uses:

1. **uv** — one pinned archive, unpacked into Demido's own runtimes folder. A machine's own uv on
   `PATH` always wins.
2. **Python** — uv fetches its own interpreter. Demido manages no runtime.
3. **graphifyy** — `uvx --from graphifyy graphify`, on first use.
4. **build** — the two stages above.

**The rule:** a thing that must be fetched says so, says how big, and says it **before** it is
fetched — as a step ladder with sizes and a live log, never as a modal after a click, and never as an
error naming a command for the user to go and run. No step asks anyone to open a terminal.

## What this file does not decide

The stack that builds it ([#10](https://github.com/elpideus/demido-studio/issues/10)), which of these
screens v0.1 builds first ([#11](https://github.com/elpideus/demido-studio/issues/11)), how a skill's
tools actually run ([#14](https://github.com/elpideus/demido-studio/issues/14)), and the lesson engine
whose output the monitor displays ([#13](https://github.com/elpideus/demido-studio/issues/13)).

It names no framework and no library beyond the icon packs `shell.md` already fixed, plus graphify
and the vis-network bundle graphify itself loads.
