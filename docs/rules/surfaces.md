# Every surface has a declared role

Carried over from v2, repainted to Karl on wayfinder ticket
[#9](https://github.com/elpideus/demido-studio/issues/9). Not machine-checked:
the token names are already enforced by `no-raw-values`, and which *role* an
element has is a judgement a script cannot make. This table is the judgement,
made once.

## Why

The palette was never the problem. v2's tokens were already Windows 11 File
Explorer's exact greys, and the app still read as inconsistent, because nothing
said which element got which token. So every component picked one that looked
right next to whatever it happened to sit beside, and after a few dozen
components the same kind of thing had three different backgrounds and two
different kinds of thing shared one.

A palette is not a design system. A palette plus a rule about who gets what is.

## The ramp

**Chrome is light and content is dark.** Down from `panel` is content, further
into the page; up from it is chrome, out of the page towards the hand.

That direction is not obvious and v2 got it backwards twice before fixing it on
2026-08-31. The first two ramps read *elevation rises with lightness*, which is
a true statement about a stack of cards and the wrong statement about this
application. The biggest surface on screen is the one you read, and it was the
**lightest** thing in the window while the bar you drag the window by was the
**darkest**: furniture that receded, content that glared. Every editor Demido
sits beside all day does the opposite, for the same reason. What you look at for
an hour should be the dark ground; what you reach for occasionally should stand
slightly proud of it.

| Token | Value | Role |
|---|---|---|
| `--color-void` | `#0a0a0a` | Behind the app itself. The frame around the custom title bar, and any gutter that is not the rack. |
| `--color-well` | `#161616` | Recessed *into* a panel: code blocks, payload dumps, log output, text inputs, search fields, the raw JSON tab. |
| `--color-rack` | `#1c1c1c` | The ground the islands stand on. The body, a window body, and the chat transcript, which is deliberately the rack rather than an island. |
| `--color-panel` | `#202020` | An island face. A page, a card, a section, a window's content, a floating panel. |
| `--color-raised` | `#272727` | An element raised *on* an island: a chip, a button at rest, the composer, a table header, a badge, a keycap. |
| `--color-chrome` | `#2c2c2c` | A bar rather than a surface: the application title bar, a panel title bar, the rail, a rail column, a tab strip, the settings nav. |
| `--color-hover` | `#323232` | The pointer is over it. Nothing more. Never a resting state. |
| `--color-active` | `#383838` | Pressed, held, or being dragged. Never a resting state. |
| `--color-edge` | `#2c4434` | Selected, active, armed. The one tinted surface: it means state, not elevation. |
| `--color-edge-strong` | `#385642` | Pressed while selected, or focused while selected. |
| `--color-seam` | `#2e2e2e` | The only hairline. Two allowed cases, in `docs/rules/gaps-and-hairlines.md`. |
| `--color-scrim` | black 60% | Behind something floating over the rack. |
| `--color-canvas` | `#ffffff` | An artifact preview. HTML and SVG expect a page, not a panel. Invariant across themes. |

`edge` and `edge-strong` sit between content and chrome deliberately, so a
selected row is lighter than the page and lighter than the rail. They used to be
darker than every grey around them, which reads as "selected" on a light panel
and reads as a hole on a light bar.

Two consequences worth stating, because they are what most v2 components got
wrong on the way through:

- **A window body is ground, not an island.** It is `rack`, and the page inside
  it draws islands on it. A page that was `panel` inside a body that was also
  `panel` had been invisible for as long as both were.
- **A card is `panel`, not `well`.** A recess is nearly black under this ramp,
  and a list of cards in one reads as a hole rather than as a list.

## The rules that follow from it

1. **One step at a time.** A raised element on a panel is `raised`, not `hover`.
   A recess in a panel is `well`, not `void`. Skipping a step to get more
   contrast is how the ramp drifts.
2. **`hover` and `active` are never resting states.** If something is on `hover`
   when nobody is pointing at it, it wanted `raised`.
3. **Nesting bottoms out.** A well inside a well is still a well. Three levels of
   nesting is a layout problem, not a colour problem.
4. **State is not elevation.** Selected is `edge`, live is `signal`, waiting is
   `amber`, delegated is `violet`. None of them is "a lighter grey".
5. **A component that wants a surface with no row here is asking for a row.** Add
   it with its role, or find the row it actually meant.

Rule 5 is the load-bearing one. It is what makes a bento layout stay coherent as
it grows, and it is the difference between this file and a palette.

## Text on these

The ink ramp's ratios are measured against `--color-hover`, the lightest thing
text ever sits on, so each is a floor rather than a best case.

`--color-ink-3` is the last tier that is still body text. `--color-ink-4` is
decorative and disabled **only**, and putting a sentence somebody has to read in
it is a bug. That is the rule this table was breaking most in v2: its own audit
on 2026-08-31 found **seventeen sentences in `ink-4`**, every one of them an
empty state or an explanation of what a surface does, which is to say the only
thing on screen at the moment somebody reads it.

On Karl the difference is `#b0b0b0` at 5.9:1 against `#7d7d7d` at 2.9:1. That is
the difference between passing AA and not.

## Which component gets which

This table says what the roles are. `design/system.md` says which role each of
the fifty-odd real components has, so no component has to decide.
