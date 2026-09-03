# Arbitrary values live in tokens.css

Checked by `scripts/check-rules.mjs`. CI fails on violations.

Widened from v2's `no-raw-colors` on wayfinder ticket
[#9](https://github.com/elpideus/demido-studio/issues/9), from one family to
five, plus a contrast check per theme.

## What is enforced

Five families of literal, none of which may appear in application source outside
`design/tokens.css`:

| Family | Shapes caught | Token prefix to use instead |
|---|---|---|
| Colour | `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, `oklch()`, `lab()` | `--color-`, `--cap-`, `--src-`, `--brand-` |
| Type | a `px`, `rem` or `em` length on `font-size`; a bare number or `em` on `letter-spacing`; a unitless number on `line-height` | `--text-`, `--leading-`, `--tracking-` |
| Space | a `px` or `rem` length on `padding`, `margin`, `gap`, `row-gap`, `column-gap`, `inset` and their long forms | `--space-` |
| Radius | a length on `border-radius` and its long forms | `--radius-` |
| Motion | a `ms` or `s` value on `transition`, `animation`, `transition-duration`, `animation-duration`, `transition-delay` | `--duration-`, `--delay-`, `--ease-` |

`0` is always allowed, in every family. It is the absence of a value, not a
choice of one.

## The sixth check: every theme is legible

A theme is a named set of 24 colours (`design/tokens.css`, part 2). For each
theme block the checker recomputes, from the values themselves:

- **4.5:1 minimum** for `--color-ink`, `--color-ink-2`, `--color-ink-3`, the
  four status colours and `--color-signal`, measured against the lightest
  surface in that theme's ramp. The lightest surface is found rather than named,
  so a theme that reorders the ramp is still measured against its own worst
  case. For Karl that is `--color-active`, not `--color-hover`.
- **2.5:1 minimum** for `--color-ink-4`, plus a check that it is strictly dimmer
  than `--color-ink-3`.
- **8.0 minimum separation** between `--color-rise` and `--color-fall`, for both
  a deuteranope and a protanope.

**Why `ink-4`'s floor is 2.5 and not 3.0.** 3.0 was the number originally
proposed, and computing it rejected it: Karl's `ink-4` measures **2.85:1**, and
it is meant to. WCAG sets no contrast requirement for disabled text at all, and
3.0 is the floor for non-text UI components, not for a decorative ink. `ink-4`
sits deliberately below body-text legibility so that using it for a sentence is
a visible bug. So the check that matters is not "is this readable", which it
must not be, but "is this visible at all, and still clearly dimmer than the tier
above it". Both are checked.

**Why the dichromat check is the pair and not the colours.** `--color-fall` is
violet rather than rose because rose against green is the classic red and green
dichromat failure. That separation is a property of the **pair**, so a theme
that redefines `--color-signal` and `--color-fall` independently can walk
straight back into it, and nobody would notice, because both candles still look
fine to a trichromat. Arithmetic catches it; eyes do not. Protanopia is checked
alongside deuteranopia because the failure class is red and green, not one
condition.

**Two sets of numbers exist for that pair, and they agree.** The values recorded
on [#8](https://github.com/elpideus/demido-studio/issues/8) and in `tokens.css`
came from the `dataviz` skill's `validate_palette.js`: deutan 4.6 for rose
against green, 17.0 for violet against green, floor 8. The checker here
implements Vienot 1999 dichromat simulation with CIEDE2000 and reports **6.1**
and **46.5** against the same floor, plus protan **52.8**. The absolute scale
differs because the delta-E formula does. The verdict is identical: rose fails,
violet clears the floor by a factor of five. CIEDE2000 was chosen over plain
Euclidean CIE76 because CIE76 scores those same pairs at 19 and 69, which puts a
floor of 8 nowhere useful.

## Why the rule got wider

v2 ran the experiment already, in one codebase, by one author, in one year:

- Colour **was** enforced. It held perfectly. The palette could be swapped
  wholesale by editing one file, which is exactly what Karl did.
- Type was **not** enforced. The same codebase accumulated 21 distinct font
  sizes and 468 arbitrary-value class names, including 99 uses of `text-[11px]`
  and 98 of `text-[10px]` sitting alongside 269 uses of `text-xs`.

The enforced rule held and the unenforced one did not. That is the argument, and
it is why a warning was rejected: a warning is an unenforced rule with extra
steps.

## Why it matters beyond tidiness

A repaint should be one diff, not a hunt. Every arbitrary visual value in one
file means the palette can be revised, a theme can be added, and contrast can be
audited without touching a single component.

It also keeps components honest. A component that needs a value the token system
does not have is a component asking for a token, which is a design decision
worth making explicitly rather than by typing a number.

## How to satisfy it

- Use a token: `var(--color-panel)`, `var(--text-body)`, `var(--space-gutter)`.
- Need a value the system lacks? Add the token to `design/tokens.css` with a
  comment saying what role it plays, and add its row to
  `docs/rules/surfaces.md` if it is a surface. Do not inline it.
- Ask for a **voice**, not a size. `--text-prose` says what the text is for;
  `13px` says only how tall it is, and the two stop agreeing the moment the
  scale is revised.

## Documented exceptions

1. **The splash and the wordmark are a lockup, not UI.** `design/splash.md`
   fixes the window at 440 by 264, the mark at 36px, *Demido* at 23px and the
   kana at 9.2px, and `design/wordmark.md` governs those by proportion (the kana
   at 40 per cent of the name). A lockup is typography set once at a fixed size,
   not text on a scale, so it is exempt from the type family. It is **not**
   exempt from the colour family.
2. **Vendored third-party CSS.** The graphify skin (`design/windows.md`)
   overrides selectors in somebody else's stylesheet. Karl tokens are used where
   they reach; anything that cannot is confined to that one skin file and says
   so at the top.
3. **Generated assets.** Anything under `scripts/` that renders an icon or an
   image outside the bundler cannot import CSS. It duplicates the few values it
   needs and names them in a comment.

An exception that is not in this list is a violation, even when it looks right.
