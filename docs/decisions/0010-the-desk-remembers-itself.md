# 0010. The desk remembers itself, and Rust holds the memory

Status: accepted
Decided: [#42](https://github.com/elpideus/demido-studio/issues/42)

Brief B42: "a VSCode-like Icons-only sidebar"

## Decision

**The shell layout is one `shell.json` per profile, written by Rust, and it is
not a setting.** It sits beside the profile's other data under `%LOCALAPPDATA%`,
carries a generation number, and is written once a gesture has settled rather
than on every state change the window makes on the way there.

**A layout file this build cannot read is discarded without a word.** Missing,
unparseable, or stamped with an older generation all mean the same thing: draw
the default desk. There is no migration, no repair and no report, and
`Store::read` returns an `Option` rather than a `Result` so that no caller can
be written which does otherwise.

This reverses v2, which kept the layout in the webview's `localStorage`.

## Consequences

Easy: the layout is inside the same boundary as the chats and the vault, drawn
by Windows rather than by a browser origin
([`docs/rules/profiles.md`](../rules/profiles.md)). A second Windows user gets
their own desk for free, and nothing in Demido had to arrange that.

Easy: growing the layout costs one line. A shape that can no longer read what
the last one wrote bumps `GENERATION`, and the cost is that each user rearranges
their desk once.

Easy: a corrupt layout cannot keep a window from opening, which is the failure
this rule is really about. Startup never blocks ([`AGENTS.md`](../../AGENTS.md)),
and a desk arrangement is the one piece of state whose loss costs a gesture.

Hard: the window can no longer arrange itself and be done. Every change to the
desk crosses IPC, so a layout gesture in the frontend has a Rust half, and the
debouncer is what keeps that from being a file rewritten on every animation
frame.

Foreclosed: putting the layout in the settings ladder, and with it a settings
row, a settings default and a settings history for something that has a window
to be seen in. The two are different kinds of state: a setting is typed, a
layout is the residue of gestures nobody typed.
