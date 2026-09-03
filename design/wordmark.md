# The wordmark

Decided on wayfinder ticket #6 against four alternatives. The board that drew them
is `prototypes/brand-board.html` in the project folder, alongside this repo.

## The lockup

```
[mark]  Demido Studio
        デミド
```

- **Demido** in Instrument Sans **600**, tracking **-0.022em**, `--color-ink`.
- **Studio** in Instrument Sans **400**, tracking **-0.01em**, `--color-ink-3`.
- **デミド** in Zen Kaku Gothic New **500**, tracking **0.1em**, `--color-ink-3`,
  at **40 per cent** of the name's size.
- The mark sits to the left, at roughly 1.15x the name's size, and is optional.
  The wordmark is valid without it; the mark is never invalid without the wordmark.

The kana sits under **Demido** alone, aligned to its left edge, not under the pair.
That is not a layout preference. デミド transliterates *Demido* and nothing else, so
centring it under both words would state something untrue.

## Two lockups, one each

Both are approved and both are canonical. They are not interchangeable: each owns a
shape of space, and picking by eye is how a wordmark drifts.

**Stacked** is the primary, and is the one above.

```
[mark]  Demido Studio
        デミド
```

Use it wherever the lockup gets a block of its own and vertical room to sit in: the
splash, the about box, the installer, the README, a marketing header.

**Inline** puts the kana to the right of Studio behind a hairline.

```
[mark]  Demido Studio │ デミド
```

Use it wherever the lockup shares a single horizontal band with something else: a
title bar, a tab strip, a header row. The hairline is not decoration and is not
optional here. Without it the kana reads as a third word in the name.

One rule holds them together: **never both on the same screen.** A window that shows
the stacked lockup in its body and the inline one in its title bar has two wordmarks,
not one used twice.

## Why this and not a silkscreen label

v2 set the name in Martian Mono, uppercase, tracked 0.2em: the same voice the app
prints on a rack unit's faceplate. It is consistent with the palette and it turns a
name into a part number.

The name is not a part number. Demido was Stefan's first chatbot, built on a
Telegram bot-maker in a community of about a hundred people. When that bot-maker
stopped being maintained he learned PHP and built his own, which is where his
programming began. *Demido* is *demo*, Japanized.

Demido Studio exists because harnesses integrate tools badly and building the thing
beats working around it. That is the same act as the one the name records, which is
why the name is set as a name.

## Rules for the kana

The kana is a signature, not a second name.

1. It appears on the **splash, the about box and the installer**. Nowhere else by
   default; anywhere else is a decision, not a habit.
2. Never larger than **40 per cent** of the name.
3. Never brighter than `--color-ink-3`, and never set in `--color-signal`. It is
   provenance, not state.
4. **Dropped below 11px, not scaled.** Kana carry more strokes than Latin at the
   same size and fail first. The wordmark is still correct without it, and mush is
   worse than absence.
5. Never used alone as the mark. The drawing in `mark.svg` is the mark.

## Faces

Both are vendored, never fetched: the splash must render correctly on a machine
that has never been online.

| Role | Face | Token |
|---|---|---|
| Name, UI, body | Instrument Sans | `--font-sans` |
| Code, labels, data | Martian Mono (variable width axis) | `--font-mono` |
| Kana | Zen Kaku Gothic New | `--font-kana` |

Martian Mono keeps the job it had in v2: its wide cut is the equipment-label voice
and its condensed cut sets code. Losing the wordmark did not lose the face.
