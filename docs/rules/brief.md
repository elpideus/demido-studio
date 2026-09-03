# The brief stays canonical

Checked by `scripts/check-rules.mjs`. CI fails on violations.

Decided on wayfinder ticket
[#12](https://github.com/elpideus/demido-studio/issues/12).

## The failure this exists to prevent

v2's milestones 10 and 11 were built from a **summary** of the brief instead of
the brief. Nobody noticed until a dedicated audit milestone re-read the original
item by item against the running app. That is not a bug in one session: reading
a long document, compressing it, and then working from the compression is the
default behaviour of every session, human or otherwise, and it will happen again
unless the process makes it impossible.

It is one of the three reasons v2 was set aside. The other two have their own
answers ([`done.md`](done.md) for built-but-not-driven,
[`tiles.md`](tiles.md) for the shape of the code). This file answers drift.

## What is enforced

Four checks, all offline and structural.

1. **Every anchor is verbatim.** Each row of [`docs/brief-map.md`](../brief-map.md)
   carries a quoted fragment of `docs/brief.md`. It must appear there character
   for character, whitespace normalised. A row that has drifted from the line it
   claims to track is worse than no row, because it reports the requirement as
   accounted for.
2. **Every bullet of the brief has a row.** A requirement nobody has looked at
   fails the build on the next commit rather than surfacing in an audit a
   milestone later. Ids are unique and never reused.
3. **Every amendment names a real row and still quotes the brief.**
4. **Every citation resolves and quotes rather than paraphrases.** A
   `Brief <id>` reference in `docs/`, `design/`, `AGENTS.md` or `README.md` must
   name a row that exists, and a citation must carry the actual words.

## The citation form

Short form, on one line:

Brief B07: "recorded in an append-only session log"

Long form, when the passage is longer than a line: the tag alone, then a
blockquote.

Brief B05:

> NOT this specific issue, but this specific KIND of issue

And the only other legal form, for a decision the brief genuinely does not speak
to:

Brief: silent

`silent` is deliberately cheap to write and expensive to write dishonestly. The
moment a resolution says `Brief: silent` about something the brief plainly
covers, that is visible in the diff and in review. A rule with an optional
escape hatch degrades into no rule, which is why the choice is quote-or-declare
rather than quote-when-relevant.

**Files written before this rule** carry no citations. They get them when next
edited. That is a one-time boundary, not a standing exemption.

## What this deliberately does not check

`check-rules.mjs` has no dependencies and no network, and keeping it that way is
worth more than the checks a network would buy. So CI **cannot** tell you:

- whether a **Built** or **Live** cell is truthful or current;
- whether a **Decided** ticket is actually closed;
- whether the prose requirements of the brief, the ones that are not bullets,
  are all present in the ledger. Bullet coverage is automatic; prose coverage is
  maintained by hand.

Those gaps are accepted. The structural checks catch the failure that actually
happened, a session working from a paraphrase and a requirement with nobody
assigned to it. A stale `Built` cell is a much smaller lie than a missing row.

## This binds planning sessions too

Not only build sessions. A wayfinder session that decides a requirement without
opening the brief is precisely how milestone 10 happened. Every ticket
resolution cites the brief, or declares it silent, and updates the ledger's
**Decided** column for the rows it touched.

## The brief is never edited

Its verbatim guarantee is the whole value of the file, and every check here
rests on it. When Stefan overrules a line, `brief.md` stays exactly as it is and
the ruling goes into the **Amendments** table of the ledger: the quoted
original, the ticket, and what binds instead. A session that reads the brief and
then the ledger learns both what was asked for and what has since been
overruled, which is the failure with the opposite sign to drift: faithfully
implementing a line that was overruled three weeks ago.

## How to satisfy it

- Open `docs/brief.md`. It is 196 lines. Read all of it.
- Then open `docs/brief-map.md` and find the rows your work touches.
- Cite as above in the resolution and in anything you write into the repo.
- Fill in the cell you actually earned: **Decided** when the ticket closes,
  **Built** when the code lands, **Live** when a real model has driven it and
  the evidence exists.
