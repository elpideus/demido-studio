# The stack

Decided on wayfinder ticket [#10](https://github.com/elpideus/demido-studio/issues/10).

`design/system.md` says what the interface is. This file says what builds it, and
`docs/rules/tiles.md` says how a piece of it is replaced. Every choice below
carries its reason, because the reason is the part a later session needs when it
wants to change one.

## The shape

| Layer | Choice | Reason in one line |
|---|---|---|
| Shell | Tauri 2 | The port ledger is only viable at all because v2 is Tauri 2 and Rust. |
| Backend | Rust, a workspace of crates | 14 of v2's 25 crates port as-is on real evidence (#2). |
| Frontend | React 19 + TypeScript | v1 and v2 are both React; v2's UI failed on shape, not on renderer. |
| Build | Vite, pnpm workspace | v2's, unchanged. No Next.js and no server components: this is a desktop SPA. |
| Primitives | Radix, one at a time | Focus traps, roving tabindex, dismissable layers and ARIA are the parts worth importing. |
| Components | Ours, from `design/system.md` | The inventory is 54 components with their tokens already assigned. |
| Styling | CSS Modules over CSS custom properties | The one-file rule is only enforceable in CSS property syntax. See below. |
| Server state | TanStack Query over `invoke`, Tauri events for streams | The session log is the source of truth; the frontend caches nothing of its own. |
| Window state | One `zustand` store | v2's call, and the only part of three shells that never needed revisiting. |

## Why not Svelte or Solid

The real gain is fine-grained updates: Solid and Svelte 5 touch the one DOM node
that changed where React re-runs a component and diffs. For Demido that would
land in exactly two places, tokens streaming into a bubble and the session log
growing while a model works.

It was rejected on evidence rather than preference. Every decision record, the
quirks file and the history of v2 were searched for a performance complaint and
there is none: the only re-render note in the whole repository is 0028 choosing
`zustand` over React context, and that worked. React 19 handles both cases with a
ref for the stream and virtualisation for the log, which is an afternoon rather
than a migration.

Against that, a switch writes off roughly 50k lines of TypeScript across v1 and
v2, replaces `react-markdown` with its remark and rehype chain (which renders
every chat bubble), and gives up Radix for Bits UI or Kobalte. `CodeMirror`,
`lightweight-charts`, `mermaid` and Lucide would all survive, so the ecosystem
argument is weaker than it looks in both directions.

The deciding argument is that v2 was set aside for three reasons and React is not
among them. Switching framework fixes none of them and spends the whole risk
budget on the part that was working.

**Revisit if** the session monitor measurably drops frames with a real model
running. The answer then is virtualisation, not a migration.

## Why not shadcn

shadcn is not a dependency. It copies component source into the repository, where
those files become ours and drift like any other file, so its consistency is a
starting condition rather than a guarantee.

Four things rule it out here, in order of weight.

1. **It hard-requires Tailwind**, because a shadcn component *is* a Tailwind class
   string with `cva` and `tailwind-merge` around it. Choosing it would decide the
   styling question below, in the direction the measurement rejects.
2. **Its token layer is smaller than Karl.** shadcn thinks in about eleven
   semantic pairs: background, card, popover, primary, secondary, muted, accent,
   destructive, border, input, ring. Karl is 24 colours with an eight step surface
   ladder and four ink tiers. Either Karl flattens into eleven and loses the
   ladder, or every component is overridden and shadcn earns nothing.
3. **It covers four of the 54 components.** Context menu, tooltip, tab strip, and
   part of the setting row's inputs. It has nothing for the rail item's four
   states, the panel frame, the snap menu, the seam, the occupancy bar, the event
   row, the prompt assembly, the keycap or the capability tag, which are the
   components that are actually hard.
4. **The consistency it sells already exists and is enforced.** `system.md` says
   which token each state of each component uses, and CI hard-fails on a raw
   value. That is a stronger guarantee than a shared origin folder.

What remains useful is shadcn as **reading material**: when building a dialog or a
select, open its version, take the markup structure and the Radix wiring, and
write the styles against our tokens. That is its homework without its token layer.

## Why CSS Modules and not Tailwind

This was measured rather than argued.

`scripts/check-rules.mjs` finds a violation by reading CSS property syntax, so a
Tailwind utility class is invisible to it. v2 shipped Tailwind v4 with the colour
rule enforced and the type rule not. Here is what its `web/src` contained on the
day it was set aside:

| Class | Uses |
|---|---|
| `text-xs` | 269 |
| `text-[11px]` | 99 |
| `text-[10px]` | 98 |
| `text-sm` | 69 |
| `text-lg`, `text-2xl`, `text-base`, `text-[9px]` | 9 |

Eight font sizes, none of them a token, none of them catchable, in a codebase
whose colour rule held perfectly across 214 commits. That contrast is the whole
argument: the rule that was enforceable held, and the rule that was not did not.

Ticket [#9](https://github.com/elpideus/demido-studio/issues/9) froze type at
seven integer steps and widened the one-file rule to five families with a CI hard
fail. Under Tailwind that rule cannot be enforced until somebody writes a
Tailwind-aware linter, and Tailwind's own scale would still sit there offering
`text-sm` as a legitimate-looking answer.

Under CSS Modules every value is written as `font-size: var(--text-body)`, which
the checker already catches with no new code. The cost is real and accepted: more
typing, and no utility shorthand.

**The rejected middle**, for the record, was Tailwind v4 with `@theme` bound to
the tokens plus a lint banning `-[...]` brackets and non-token utilities. It would
work, and it means building and maintaining that linter before the first screen is
written.

## Where state lives

Three buckets and no fourth. The session log is the source of truth (v2 decision
0002) and history is derived from it, so the frontend holds much less than a
normal application would.

1. **From Rust.** Sessions, messages, models, settings, skills. TanStack Query
   over `invoke`, with Tauri events for anything streaming. There is no second
   copy of this in a store.
2. **This window.** Which panels are open, their geometry, focus, the jump target,
   the composer draft. One `zustand` store, mirrored to disk as below.
3. **This component.** `useState`. If nothing outside the component reads it, it
   does not go in the store. That test came out of v2's decision 0028 and it is
   the one part of three shells that never needed revisiting.

Streaming tokens go into a ref with a subscription, never `setState` per token.

## The browser is one panel over two engines

The brief asks for one browser that the user and the model both use. That cannot
be one engine. v2's quirks file records why:

> **WebView2 has no isolated worlds.** Any script injected into a page shares that
> page's JS context, so a hostile page can shadow or spoof it. This is why the
> model drives Chromium over CDP rather than the built-in webview.

So the browser panel is one surface with two engines beneath it. The user browses
in the embedded WebView2. The model drives a real Chromium over CDP. The panel's
"who is driving" line names which, and `design/system.md` already treats that line
as something a theme may never hide.

**This amends the brief**, which reads as a single browser, and is recorded here
per the rule that a decision contradicting the brief holds only once Stefan says
so in writing. Stefan agreed on 2026-09-03.

## Both panel modes ship in v0.1

`design/shell.md` specifies floating and pinned. Both ship, and the reason the
cheaper phasing was withdrawn is that the work already exists: v2's decision 0030
implemented four placements (`floating`, `left`, `right`, `full`), pointer-event
dragging, pinning by throwing a window at an edge, and unpinning back to the
rectangle it had before, all driven and measured through CDP.

That is a port under the quarantine in the map's Notes, not a licence to trust it:
it enters v3 inside a vertical slice and only once that slice is driven live.

What changes from 0030 is the gesture surface, per `shell.md`: the hover-or-drag
menu on the maximise button, the ghost preview, the `grip-vertical` on a pinned
title bar, the draggable seam, and the minimum useful size with its stated reduced
form.

## What the shell persists, and where

**One `shell.json` per account profile, written by Rust, debounced after a gesture
settles.** It holds per-panel geometry, pre-pin geometry, pin state and edge, seam
positions, which panels are open, the rail's edge and the rail's Edit Navbar
order. The rail's edge keeps its own key, so resetting a layout somebody got lost
in does not also move the rail they placed once on purpose.

It is generation-numbered, and a layout that will not load is discarded without a
word, because startup never blocks.

**This reverses v2's decision 0028**, which put the arrangement in `localStorage`
and explicitly rejected the profile directory as "a round trip to Rust on every
sash drag". Two things changed. Multi-account makes `localStorage` wrong outright,
since it is one origin for the whole machine and every account on it would share
one layout. And the sash objection dies with debouncing, which 0028 already had in
its `SETTLE` clock and then wrote to the wrong place anyway.

It stays **out** of the settings schema and its provenance machinery, which 0028
got right: a sash position has no default worth resolving and must never appear in
the Navigator as a thing to jump to.

What "per account" means as a security boundary is ticket
[#15](https://github.com/elpideus/demido-studio/issues/15). This only says the
layout is scoped to whatever that boundary turns out to be.

## What this file does not decide

Which of these screens is built first
([#11](https://github.com/elpideus/demido-studio/issues/11)), the account boundary
itself ([#15](https://github.com/elpideus/demido-studio/issues/15)), and how a
skill's tools actually run
([#14](https://github.com/elpideus/demido-studio/issues/14)), which is the runtime
half of `docs/rules/tiles.md`.
