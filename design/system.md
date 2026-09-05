# The design system

Decided on wayfinder ticket [#9](https://github.com/elpideus/demido-studio/issues/9).

`tokens.css` holds the numbers. `docs/rules/surfaces.md` says what the surface
roles are and `docs/rules/gaps-and-hairlines.md` says how regions are separated.
This file holds the three things that are neither a number nor a rule: **what a
theme is**, **where keyboard shortcuts appear**, and **which role each real
component has**.

Read it as the extraction of `prototypes/shell-a-board.html` (7 screens) and
`prototypes/canvas-board.html` (12 screens), which are throwaway and are not in
this repo. Approved pictures are not a design system; this is what is left of
them once the hand-nudging is snapped out and every element has been told which
token it gets.

## What a theme is

The brief does not mention themes. It says the UI should be dark, once, and
nothing else about appearance. Stefan extended this on 2026-09-03: **the tokens
are structured for more themes later, and dark ships first.** Recorded here per
the rule that a decision going beyond the brief holds only once Stefan says so
in writing, the same way the Hyprland and i3 amendment is recorded in
`shell.md`.

Neither v1 nor v2 has a single line of theme infrastructure: no `data-theme`, no
`prefers-color-scheme`, nothing. There is nothing to port, which is why the
contract is written tightly now rather than discovered later.

**A theme is a named set of exactly 24 colours and nothing else.** Eight
surfaces, `seam`, `scrim`, two edges, four inks, two signals, four statuses, and
the two market directions. It declares its own `color-scheme`, because a light
theme needs native scrollbars and form controls to invert with it.

**A theme repaints; it cannot relayout.** Type, spacing, radii, motion and the
one shadow are invariant, and so are the vendor brand marks and
`--color-canvas`. This is the load-bearing half of the contract, and the reason
is this map's own Definition of Done: a ticket closes only when a real model
used the thing in a running window, with a screenshot. If a theme could change
what fits on screen, every screenshot would need retaking per theme. Under this
split one screenshot stays valid across all of them.

Density is **not** a theme. If a compact mode is ever wanted it is an orthogonal
axis that composes with every theme, and folding it in here would mean it can
never be orthogonal again.

**Themes are built in.** There is no user-authored theme format yet. When there
is one it will be declarative, taking those 24 values and nothing more, and it
will live with skill distribution rather than here. It will **never** be
arbitrary CSS. This application promises that everything the model sees is
inspectable, and a theme format that can hide the approval prompt, the occupancy
bar or the browser's "who is driving" line is a hole straight through that
promise.

**Every theme's contrast is arithmetic, not opinion.** CI recomputes it on every
build: 4.5:1 for the readable inks and the accents, 3.0:1 for `ink-4`, and 8.0
deutan separation for the `rise` and `fall` pair. Floors and reasoning are in
`docs/rules/no-raw-values.md`. This is what makes "more themes later" safe
instead of a slow decay, and it means a theme can eventually be accepted from
somebody else without reading it.

## Where shortcuts appear

> Full Keyboard navigation with cool key-shaped shortcuts indicators shown
> across the UI. Do not overdo it, only show them where it makes sense and it
> looks cool. It is okay to show them on hover so user learns it after hovering.

"Only where it makes sense" has to be a rule rather than taste, or every
component re-litigates it. Three lists, and a component consults them instead of
deciding.

**Always visible.** The Navigator's rows, because it is a keyboard surface and
the cap *is* the content. The approval prompt's Enter and Escape, because a
decision made under time pressure should not require a hover to learn. The
empty-state calls to action, which are the first thing a new user reads.

**Revealed on hover, after `--delay-keycap`.** Rail icons, panel title-bar
buttons, composer actions, context-menu items, tab strips. The dwell matters:
without it, moving the pointer across the rail flashes six caps in half a
second, which is the opposite of learning.

**Never.** Inside the transcript. On anything a model produced. On more than one
control within 200px of another cap. That last one is the brief's "do not overdo
it" made checkable: density, not taste.

The cap itself is `--radius-control` on `--color-raised`, set in the silkscreen
voice at `--color-ink-3`. It takes no hairline, because `raised` already clears
every surface it sits on except `chrome`, and a cap on `chrome` moves to `panel`
rather than gaining a border.

## Component inventory

Fifty-four components, read off the nineteen artboards plus `shell.md` and
`windows.md`. Each row is a name, its states, and the tokens each state uses,
resolved against `docs/rules/surfaces.md`. That is deliberately the level: a
name and a list of states is what v2 had implicitly, and it is why the same kind
of thing ended up with three different backgrounds. The table says what roles
exist; this says which role each thing has, so no component decides.

This is the build checklist for the shell. An incomplete inventory is an
incomplete shell, and ticket
[#11](https://github.com/elpideus/demido-studio/issues/11) picks the order.

### Shell

| Component | States | Tokens |
|---|---|---|
| Application title bar | focused, unfocused | `chrome`; title `ink-2`, controls `ink-3`, `hover` and `active` on buttons. Keeps its OS minimise. Carries the download indicator. |
| Rail | docked left, docked right | `chrome` ground, items on a `--space-gutter` rhythm. Right-click gives Edit Navbar. |
| Rail item | closed, open unfocused, open focused, pinned | closed `ink-4` no marker; open `ink-2` with a 14px `signal-dim` tick; focused `signal` on `edge` with an 18px `signal` tick; pinned `signal` with a 26px `signal` bar. `hover` under the pointer. The only place in the UI that reports what is open. |
| Desk | always present | `rack`. Not an island, never navigated away from. |
| Panel frame | floating, pinned | `panel` at `--radius-island`; floating adds `shadow-float`, pinned drops it and shows a `grip-vertical` at the title bar's left. Title bar `chrome`, carrying maximise and close only. |
| Snap menu | open, target hovered | `panel` with `shadow-float` at `--radius-control`; rows `hover`; the ghost preview on the desk is `edge`. Transient chrome, not a panel. |
| Seam | rest, hover, dragging | `--space-gutter` wide, `seam` hairline (allowed case 2), `active` while dragged. |
| Scrim | present | `scrim`. |

### Chat

| Component | States | Tokens |
|---|---|---|
| Transcript | scrolled, at top | `rack`. `--text-prose` at `--leading-prose`, `ink-2`. |
| Message | user, assistant, streaming | user on `raised` at `--radius-island`; assistant unfilled on `rack`. Inline code spans `well`. |
| Thinking block | collapsed, streaming, done | `well` ground, `src-reasoning` accent, elapsed count in the silkscreen voice. Visible by default, collapsible, carries its own Caveman level. |
| Tool call row | pending, awaiting approval, running, done, failed | `raised`; `src-tool` accent, `signal` running, `ink-3` done, `rose` failed. |
| Approval prompt | waiting, approved, denied | `raised` with an `amber` accent and a raised-hand icon. Enter and Escape caps always visible. Carries "always for this tool". |
| Artifact card | rest, hover, lit | `raised`, `hover`; lit to `edge` for as long as its panel is open. |
| Composer | rest, focused, disabled, model running | `well` at `--radius-island` (a field is a recess); focused ring `signal`; disabled states the reason in `ink-3`, never `ink-4`; send becomes stop while the model runs. |
| Model selector | rest, open, none installed | `raised`; capability tags inline. |
| Agent mode control | Cautious, Balanced, Autonomous | `raised`, active on `signal`. A shield icon on Cautious. No status dot. Three states, not two ([#20](https://github.com/elpideus/demido-studio/issues/20)). |
| Tool picker | closed, open, group expanded, group partial | Popover from the composer, `panel` on `edge`. Rows are groups (built-ins, then one per skill) with a tool count, expandable in place to individual switches. A group with some tools off renders partial, never on. |
| Chat list | rest, filtered | `chrome` column with an All / Chats / Projects filter. Rows `ink-2`, `hover`, selected `edge`. |
| Chat list empty state | no chats | Violet at 176px, `saturate(0%)`, `opacity: .05`, masked `linear-gradient(to bottom, black 50%, transparent 100%)`. Caption "No chats yet." in `ink-3`. |

### Models

| Component | States | Tokens |
|---|---|---|
| Model row | rest, hover, selected, installed, not installed | `panel` card, `hover`, selected `edge`. |
| Capability tag | present, absent | `--radius-chip` on `raised`; present in its `--cap-*` colour, absent greyed to `ink-4` and never hidden. |
| Download row | queued, running, paused, failed, done | `raised`; progress `signal`, paused `amber`, failed `rose` with retry in place, resuming rather than restarting. |
| Download indicator | idle, active, failed | In the application title bar; the queue opens as `panel` with `shadow-float`. Owns per-item pause, resume, cancel. |
| Model metadata | always | Quant, size and context in the silkscreen voice at `ink-3`. |

### Setup

| Component | States | Tokens |
|---|---|---|
| Backend picker | available, unavailable, selected | `panel` cards; a `--brand-*` mark at full strength only where the hardware is present, `ink-3` otherwise. |
| Provisioning step ladder | pending, running, done, failed | Pending `ink-4`, running `signal`, done `signal-dim`, failed `rose`, matching `splash.md`. Every step states its size **before** it fetches. Never a modal, never a command to go and run. |

### Session monitor

| Component | States | Tokens |
|---|---|---|
| Lanes strip | per event block | Four lanes on `rack`; blocks at `--radius-chip` in their `--src-*` colour. Width is elapsed time, height is token weight. |
| Occupancy bar | per source segment | `well` ground, segments `--src-*`, label `ink-3`. Reads "21.7k of 32k". |
| Source ledger | rest, hover, scoped | `chrome` column, count and tokens per source, each row carrying its source icon as well as its colour. |
| Agent scope list | main session, agent, selected | Indent is delegation depth; selected `edge`. Selecting scopes the stream, ledger, lanes and occupancy bar at once. |
| Parallelism slots | filled, queued, free | `--radius-pill`; `signal`, `amber`, `ink-4`. Answers "why is nothing happening" on screen. |
| Event row | rest, hover, selected, error | `--src-*` accent plus icon; token weight as a number and a bar. Carries fork, replay from here, export selection. |
| Turn group | collapsed, expanded | Chunk runs folded into the message they assembled. |
| Detail pane | per tab | `panel`; the last tab is raw JSON on `well`, because the log is the record. |
| Prompt assembly | unchanged, inserted, evicted | Rebuilds the prompt as it stood at the selected event, diffed against the previous assembly: inserted blocks readable, evicted blocks struck with their cost. Each block header carries an edit affordance. |

### Settings

| Component | States | Tokens |
|---|---|---|
| Settings nav | rest, hover, selected | `chrome`; selected `edge`. |
| Setting row | following global, overridden | following shows the inherited value in `ink-4` and carries nothing; overridden shows the local value, marks the row `signal`, and carries the way back to global. No provenance chip anywhere. |
| Keybinding field | rest, capturing, conflict | The binding **is** the field: `well`, `signal` while capturing, conflict shown in place in `rose` against the command it collides with, never in a modal. A scope column of global, desk or panel. |

### Projects

| Component | States | Tokens |
|---|---|---|
| Project page | rest | `panel`. Name, description, icon picker taking custom PNG and SVG on the face of it. |
| Attached folder row | scanning, ready, error | States file count, total size and the exclusions applied. Scanning is a row, never a blocking dialog. |

### Market charts

| Component | States | Tokens |
|---|---|---|
| Candle chart | rest, crosshair | One price axis, never two. Bodies filled in both directions: `rise` up, `fall` down. Last price is the only direct label. No indicators, no drawing tools, no volume pane. |
| Timeframe selector | rest, selected | Tab strip on `chrome`, selected `edge`. |
| Symbol search | rest, focused, results | `well` field; each result names which account or exchange answers for it. |

### Navigator

| Component | States | Tokens |
|---|---|---|
| Navigator panel | empty, typing, scoped | `panel` with `shadow-float`. One ranked list, category chip on the row, selected `edge`. Prefixes `@` files, `>` settings and commands, `!` commands. With nothing typed, chats rank first. |
| Arrival highlight | firing | Opens the target page, scrolls to the row, lights it `edge` and fades over `--duration-panel`. |

### Code graph

| Component | States | Tokens |
|---|---|---|
| Graph host | not built, building, built | graphify's own viewer as `srcdoc` in an iframe, no server and no port. The skin is a `<style>` appended after graphify's own and touches **chrome only**: body, sidebar, search, node info, communities, checkboxes, scrollbars. Node hues stay graphify's, because they are community-coded data and recolouring them would be recolouring the answer. |
| Build CTA | not initialised | Empty state that runs the provisioning ladder: uv, Python, graphifyy, build. |

### Browser

| Component | States | Tokens |
|---|---|---|
| Browser panel | user driving, model driving | The driver is named **in words** at the bottom of the page in `violet`, with the model's last action. Take over is one click and one key. Every click the model makes is a tool call in the session log. |

### Cross-cutting

| Component | States | Tokens |
|---|---|---|
| Keycap | hidden, revealed, active | `raised` at `--radius-control`, silkscreen voice, `ink-3`. Three placement lists above. |
| Context menu | open | `panel` with `shadow-float`; rows `hover`. |
| Tooltip | open | `panel` with `shadow-float`, `--text-meta`. |
| Tab strip | rest, selected | `chrome`, selected `edge`. |
| Icon | rest, disabled | Lucide 1.35.0 at `stroke-width: 1.8`, 12 to 18px, `currentColor`. Never drawn by hand; path geometry is never edited. |
| Splash | loading, failed | 440 by 264, exempt from the type scale as a lockup. See `splash.md`. |

## Two conflicts this extraction found

Both are places where a committed decision collides with a rule this file
freezes. Recorded rather than quietly fixed, because both are amendments to
earlier tickets.

**The Violet empty state fails the ink floor.** `shell.md` specifies "No chats
yet." at 30 per cent opacity, carried over from v1's own numbers. White at 30
per cent over `--color-rack` resolves to roughly `#606060`, which measures about
3.9:1 against the rack: below the 4.5 floor, and in exactly the situation v2's
own audit called out, where the failing text is the only thing on screen at the
moment somebody reads it. **The caption moves to `ink-3` with no opacity.**
Violet herself keeps `opacity: .05`; she is decorative and the rule does not
reach her. This amends `shell.md`.

**`splash.md` names a token that no longer exists.** Its inner top edge is
specified as `inset 0 1px 0 0 --color-panel-raised`, which was v2's name for the
role Karl calls `--color-raised`. The value is unchanged in intent; the
reference is stale. Corrected in place.

## What this file does not decide

The stack that builds it
([#10](https://github.com/elpideus/demido-studio/issues/10)), which components
v0.1 builds first ([#11](https://github.com/elpideus/demido-studio/issues/11)),
and the default keymap, which the map rules out of scope: the keybinding
mechanism is decided here and in `windows.md`, but choosing which chord does
what is build work.

It names no framework and no library beyond the icon packs `shell.md` already
fixed.
