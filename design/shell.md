# The shell

Decided on wayfinder ticket #7, against two rejected alternatives. The boards that argued it are
`prototypes/shell-board.html` (three shells) and `prototypes/shell-a-board.html` (the winner, drawn
deep) in the project folder alongside this repo.

This file is the decision. The boards are the argument, and they are throwaway.

## Chat is the desk

Chat is not a window. It is the surface the application *is*: always present, filling the frame under
the rail, never navigated away from. Everything else (the session monitor, the code graph, the
market charts, the file explorer, the sub-agent monitor, the embedded browser) opens as a window
**over** that desk.

Two alternatives were drawn and rejected:

- **Everything is a window.** No privileged surface; the rack is a desktop and chat is the first
  window on it. More powerful, and it opens on an empty desk that has to be explained.
- **The rail switches rooms.** Chats, Code, Market and Session as workspaces with saved
  arrangements. Denser, but watching a chart while chatting needs two rooms at once.

Chat won because Demido is a chat application that owns tools, not a workbench that happens to
contain a chat. One of the three reasons v2 was set aside is that its shell was rebuilt three times
in three days; a shell with a fixed centre has far less to rebuild.

## The rail

Icons only, on `--color-chrome`, movable to either edge of the window. Navigation lives at the top,
Settings at the bottom, per the brief. Right-clicking empty rail space or the Settings button offers
**Edit Navbar**.

Default order, top to bottom: Chats, Files, Code graph, Market charts, Session monitor, Browser.
Settings sits alone at the other end.

**Amended on #8**, and it amends the brief's own rail listing too: there is no **Sub-agents** entry.
A sub-agent is a scope on the session log, not a window of its own, so it lives inside the session
monitor. See `windows.md`.

A rail icon has four states, and they are the only place in the UI that reports what is open:

| State | Icon | Marker at the leading edge |
|---|---|---|
| closed | `--color-ink-4` | none |
| open, not focused | `--color-ink-2` | 14px tick, `--color-signal-dim` |
| open and focused | `--color-signal` on `--color-edge` | 18px tick, `--color-signal` |
| pinned | `--color-signal` | 26px bar, `--color-signal` |

Pinned reads differently from merely open, deliberately: it is a different mode, not a stronger
version of the same one.

## Panels have two modes

Every panel except chat is a window with exactly two modes.

**Floating** is the default. `--shadow-float`, drags anywhere, snaps to edges and halves the way a
Windows 11 window does, overlaps with a z-order, and covers whatever is beneath it.

**Pinned** takes the panel out of the float layer and makes it part of the layout. The shadow goes,
the chat island gives up exactly the panel's width or height, and **nothing is covered**. Pinned
panels tile against each other: a panel pinned right and another pinned bottom both get their edge,
and the desk keeps what is left.

The `--space-gutter` of rack between chat and a pinned panel is a **draggable seam**. Dragging it
rebalances the two.

### Minimum useful size

**Added on #8.** A pinned panel has a floor below which it stops being useful, and the seam must not
drag past it. A panel that is squeezed instead of collapsed is the failure this rule exists to
prevent: it keeps every element and makes all of them unreadable.

Below its floor a panel **collapses to a stated reduced form**, which it must define. The session
monitor's floor is **210px high**, and its reduced form drops the scope column to icons and detaches
the inspector rather than compressing all three columns. A panel with no defined reduced form has a
floor and simply stops there.

At 1280×800, the smallest window Demido claims to support, two pins and a float leave chat roughly
670×375 of transcript. That is the worst realistic case and it was checked, not assumed.

### The gesture

There is no pin button. Pinning is a gesture with two forms that end in the same place:

1. **Hover the panel's maximise button**, or
2. **drag the panel onto that button.**

Either opens a menu anchored directly beneath the maximise button (*maximise, pin left, pin right,
pin top, pin bottom*) and the target under the cursor previews as a ghost on the desk before the
gesture is committed. The menu floats over the panel's own content, like the Windows 11 snap flyout
it is modelled on: it is transient chrome, not a panel.

**Unpinning is one gesture too.** Drag a pinned panel away from its edge and it returns to floating
**at the size it had before it was pinned**. That previous geometry is state the window manager owns
and must keep.

A pinned panel shows a grip (Lucide `grip-vertical`) at the left of its title bar where a floating
one shows nothing. That grip is the handle, and it is the only chrome that differs between the two
modes.

### No minimise

Panel title bars carry **maximise and close**, and nothing else. Minimise does not exist, because the
rail is the taskbar: close really closes, and the panel's rail icon reopens it in the geometry it
had. Two buttons, both familiar, and one less thing to explain.

The application's own title bar is an OS window and keeps its minimise. Panels are not OS windows.

### Amendment to the brief

The brief says:

> The panels system should be inspired by Hyprland and i3 tiling systems on linux.

Stefan amended this on 2026-09-03: **no i3.** Panels behave like Windows 11 or KDE Plasma windows,
minus minimise. Recorded here per the rule that a decision contradicting the brief holds only once
Stefan says otherwise in writing.

## The artifact sidebar is not a special surface

It is a panel that opens **pinned**. That is the whole specification, and it is why clicking an
artifact card can never cover the message that produced it. Its header carries preview/source
toggles (`eye`, `code-xml`) and copy; everything else about it (snapping, resizing, unpinning,
its rail marker) comes free from the rule above.

The card in the transcript stays lit while its panel is open, so the link between the two is visible
without hovering anything.

## States that go wrong

A shell is judged on these, not on the happy path.

**No model installed.** The composer is disabled rather than hidden, and says why: *"Pick a model to
start. Nothing is loaded yet."* The desk offers two ways out, not one: **Browse models**, and
**Start on Nexus (free, keyless)**. Ticket #4 decided Nexus degrades *downward* to a local download
and never sideways into a paywall, so the free rung is offered plainly rather than buried.

**No chats.** The chat list shows Violet, carried over from v1 unchanged: 176px wide,
`filter: saturate(0%)`, `opacity: .05`, masked with `linear-gradient(to bottom, black 50%,
transparent 100%)` so she sinks into the rack rather than sitting on it, with *"No chats yet."*
beneath. These are v1's own numbers, from its `Sidebar.tsx`.

**Amended on #9.** v1 set that caption at 30% opacity, which over the rack resolves to about
`#606060` and measures roughly 3.9:1: below the 4.5 floor, on the only text on screen at the moment
somebody reads it. The caption is `--color-ink-3` with no opacity. Violet keeps `opacity: .05`,
because she is decorative and the ink rule does not reach her. See `design/system.md`.

**A download fails.** A row in the queue, never a dialog. It states what happened and offers retry in
place, resuming rather than restarting; the other downloads keep running. The queue lives behind a
download indicator in the application title bar and owns per-item pause, resume and cancel.

**A tool call in Cautious.** Nothing runs until approved. Enter approves, Escape denies, and *"always
for this tool"* is how Cautious decays into Balanced by the user's choice rather than by nagging.

**The tool picker.** A popover from the composer, beside the mode control, listing what this
conversation offers the model: built-in groups first, then one row per installed skill, each
expandable in place to its individual tools. Everything is on by default and a disabled tool is not
sent to the model at all. It is the only surface for this, and it is the reason there is no Chat
mode: an empty set is what Chat would have been. See
[`docs/rules/tools.md`](../docs/rules/tools.md).

**A model thinking.** Visible by default, streaming, collapsible, timed, and carrying its own Caveman
level independent of the answer's. The send button becomes stop while the model runs; the elapsed
count is the progress indicator. There is no indeterminate bar.

## Colour beyond the palette

`tokens.css` gains a semantic block below the accents. Every value **aliases an accent Karl already
defines**: no new colour is introduced, and the one-file rule holds.

Capabilities are facts about a model, and they appear as tags with an icon and a colour wherever a
model appears, in the model list, the detail pane, the composer:

| Capability | Token | Aliases |
|---|---|---|
| Vision | `--cap-vision` | `--color-sky` |
| Tools | `--cap-tools` | `--color-signal` |
| Reasoning | `--cap-reasoning` | `--color-violet` |
| Audio | `--cap-audio` | `--color-amber` |

A capability the model lacks is greyed, never hidden.

The session log is colour-coded **by source**, which is the axis the brief asks it to be inspectable
along: `--src-system`, `--src-skill`, `--src-user`, `--src-inject`, `--src-reasoning`,
`--src-artifact`, `--src-tool`, `--src-error`. Each row also carries its source's icon. That is what
turns an append-only log into something scannable.

Brand colours are the one exception, because they are not ours to choose: `--brand-nvidia`,
`--brand-amd`, `--brand-intel`, `--brand-vulkan`. They are used at full strength only where the
hardware is actually present, and in ink everywhere else, so the backend picker stays a list rather
than becoming a logo wall.

## Icons come from packs

**Never draw an icon.** Both packs below are vendored, and every icon carries a comment naming its
upstream identifier.

| Role | Pack | Licence | Obligation |
|---|---|---|---|
| UI icons | Lucide 1.35.0 | ISC | `licenses/lucide-icons/lucide/LICENSE` |
| Vendor marks | Simple Icons | CC0-1.0 | notices entry only |

Lucide is not a new dependency: v1 and v2 both already ship `lucide-react`. Vendor marks appear on
the setup wizard's backend picker, where naming whose runtime you are choosing is the screen's whole
job; they remain their owners' trademarks and the use is nominative. Both belong in
`THIRD_PARTY_NOTICES`.

The only permitted departure from a pack's defaults is Lucide's `stroke-width` at **1.8** rather than
2, because these render at 12–18px on a dark ground where 2 blooms. That is a documented prop.
Editing path geometry is not permitted.

## There is no status dot

A small coloured circle beside a label is banned from this UI. It was on nine components in an
earlier draft (agent mode, the approval prompt, the model selector, the assistant byline, the
download indicator, every download row), and it is one of the most reliable tells of a generated
interface. It also did no work that the label's own colour or a real icon could not do better.

State is carried by an icon, by colouring the words, or by the thing itself: a shield on Cautious, a
raised hand on an approval that is waiting, a progress bar already coloured by its own state. Where
the words suffice, nothing is added.

## What this file does not decide

What the mode control gates and what the tool picker offers (#20), the stack that builds it (#10), what Nexus may honestly promise on the empty state (#18), and which
of these screens the v0.1 slice builds first (#11). This file describes the shell; it names no
framework and no library beyond the two icon packs.
