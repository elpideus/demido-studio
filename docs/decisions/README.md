# Decision notes

Why things are the way they are, for the choices where a reasonable alternative
was rejected.

Decided on wayfinder ticket
[#12](https://github.com/elpideus/demido-studio/issues/12).

## When one is written

All three must be true, or there is no note:

1. **Hard to reverse.** Changing your mind later costs something real.
2. **Surprising without context.** A future reader will ask why it was done this
   way.
3. **The result of a trade-off.** There were genuine alternatives and one was
   picked for specific reasons.

Ordinary features and bugfixes get nothing. A wayfinder resolution earns a note
only when code will point at it: the tile contract does, the palette's history
does not.

## The shape

`NNNN-slug.md`, numbered in order, never renumbered.

```markdown
# NNNN. Title

Status: accepted            (or: superseded-by NNNN)
Decided: <ticket or session link>

## Decision

<a few lines. What binds, stated flatly.>

## Consequences

<what this makes easy, what it makes hard, what it forecloses.>
```

**The note is short and the reasoning is not in it.** That is deliberate. The
map's tickets already hold long-form reasoning with the alternatives that lost,
and a second copy in the repo would be a copy that drifts. The note exists so
that code can reference a stable path, and so a reader with no network can still
find out what binds and why it might surprise them. `Decided:` is the one hop to
the argument.

## Referencing

Reference a note from the code it governs, so the reasoning is one hop from the
thing it explains. CI checks that a `docs/decisions/NNNN-slug.md` reference
resolves and is not superseded, because a dangling reference tells the reader an
explanation exists when it does not, and a reference to a superseded note is
worse: it hands them reasoning that has since been overturned.

When superseding, set `Status: superseded-by NNNN` and update the references.
The check finds any you miss.
