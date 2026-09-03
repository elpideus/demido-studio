# Gaps separate islands, hairlines are the exception

Carried over from v2 and **narrowed from three cases to two** on wayfinder
ticket [#9](https://github.com/elpideus/demido-studio/issues/9). Not
machine-checked: a hairline is a border in CSS whichever reason it has, so a
check would have to read intent. Reviewed by eye, and by this file being short
enough that adding a row to it is a visible act.

## The rule

Islands float on the rack with `--space-gutter` between them. **The gap is the
border.** A `border` or `outline` used to divide one region from another is a
violation unless it is one of the two cases below.

## The cases where a hairline is allowed

Use `--color-seam`. Nothing else.

1. **A scrolling region against something fixed.** A list that scrolls under a
   header or over a footer needs the edge to exist before the content reaches
   it, otherwise the first row appears to be part of the header. A gap cannot do
   this, because the scrolling content shows through it.
2. **A resize seam.** A splitter has to be visible before the pointer is on it,
   or the only way to discover a panel is resizable is to guess. The seam is the
   affordance. This is the draggable gutter between the desk and a pinned panel.

Both are physics. Neither is a judgement about whether two greys read apart, and
that is the entire test for whether a third case belongs here.

## The case that was removed

v2 allowed a third: **a control whose fill equals its surroundings**, with the
instruction to "prefer moving the control down the ramp first, take the hairline
only when the ramp has no step left".

A rule that says *prefer the other thing first* is an escape hatch from a rule
the ramp can already satisfy. Karl has eight surface steps and a `--color-well`
that exists specifically for inputs and fields; a control that cannot find
contrast in eight steps has a layout problem, not a colour problem, and
`docs/rules/surfaces.md` rule 1 already answers it.

Removing it also makes this rule nearly checkable by eye: with two cases, a
border outside a scroll container or a splitter is a review finding on sight,
with no argument about whether the ramp had run out.

Karl's ladder is tighter than v2's Rack (adjacent steps of 1.046 to 1.094), so
this case would have fired *more* often here, not less. `design/tokens.css` is
explicit about what to do instead: **if the ladder proves too tight once real
screens exist, widen the ladder.** Never reach for `--color-seam`.

## Why it is not just "borders are fine now"

The gap rule is what makes the app read as equipment rather than as a web page
with panels drawn on it, and it is the first thing that goes when a codebase
gets tired. Windows 11's File Explorer, which this app sits beside all day, uses
hairlines for a small fixed set of cases and gaps for everything else.

## How to satisfy it

- Reach for the ramp first. Most "this needs a border" is "these two surfaces
  are one step apart", which `docs/rules/surfaces.md` answers.
- If one of the two cases genuinely applies, use `--color-seam` and nothing else.
- If a third case turns up, add it here with its reasoning **before** writing it.
  A hairline that is not in this list is a violation even when it looks right.
