# The splash

Decided on wayfinder ticket #6. Drawn in Karl on `prototypes/brand-board.html`
in the project folder.

The brief asks for it directly:

> There should be a splash small window (similar to that of Discord), showing the
> logo, the Demido Studio name and the loading status (maybe even what it is
> actually loading?).

The parenthesis is taken at face value. The splash names the subsystem coming up,
not a percentage and not a spinner.

## The window

- **440 x 264**, frameless, transparent background, corners rounded at 12px.
- Face is `--color-panel` with `--shadow-float` beneath it and a single lighter
  edge along the top (`inset 0 1px 0 0 --color-raised`): a rack unit's front
  bezel catching light. Neutral, because green is reserved for state.
- Padding 26px. Content is weighted to the bottom, air above: one block, a
  faceplate, rather than a header and a footer with a void between them.

## What is on it

Top, the **stacked lockup** (see `wordmark.md`): mark at 36px, *Demido* at 23px,
デミド at 9.2px under it. Version beneath in `--color-ink-4`, indented to the
wordmark's own left edge so the block reads as one stamped plate rather than three
loose lines.

Bottom, the **scale**: one tick per startup stage, then the stage line naming the
one currently running, in `--color-ink-3`, uppercase, tracked.

## Tick states

| State | Fill | Height |
|---|---|---|
| pending | `--color-ink-4` | 11px |
| done | `--color-signal-dim` | 11px |
| running | `--color-signal` | 18px |
| failed | `--color-rose` | 11px |

The running tick is the only full-height one. That is the whole animation: no
sweep, no pulse, no indeterminate bar. Something is either up, coming up, waiting,
or broken.

## A failure does not stop it

A subsystem that fails turns its tick rose and **the splash carries on to the next
one**. It is information, not an alarm. The app reports the failure in Settings and
still reaches a usable state.

This is the startup-never-blocks rule expressed in the first window the user ever
sees, which is the right place for it: a splash that can hang is a splash that
trains people to force-quit.

## How it is built

Vanilla. No framework, no router, no store. It is painted before anything else
exists, so it cannot wait on anything else existing. Colours come from
`tokens.css`; both faces are vendored rather than fetched, because the splash must
render correctly on a machine that has never been online.

The tick count is the number of startup stages, which the shell owns. This file
does not name them.
