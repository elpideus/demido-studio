# Managed runtimes: what moves a pin, and what deletes the old one

Decided on wayfinder tickets
[#29](https://github.com/elpideus/demido-studio/issues/29) (sections 0 to 5) and
[#30](https://github.com/elpideus/demido-studio/issues/30) (sections 6 to 11).
This file is the back half of [`setup.md`](setup.md): section 4 there states what
the first fetch costs, and this states what the folder holds a year later and how
it got that way.

Brief B11: "Guided set-up on first launch"

The brief asks for the installation and is silent on what updates or removes it.
Everything below is therefore a consequence of the manifest existing at all, not
a requirement anybody wrote down.

## The failure this exists to prevent

[#28](https://github.com/elpideus/demido-studio/issues/28) went looking for the
size of one Chrome and found three: `chrome-150.0.7871.24`,
`chrome-150.0.7871.115` and `chrome-152.0.7977.54` in
`~/.agent-browser/browsers`, **420 + 420 + 429 MiB**, because nothing removes
the previous pin when a new one lands.

#29 then measured the rest of the same machine and found a **second** cache
nobody had counted: `~/.cache/puppeteer` holds two more Chromes and two headless
shells, **1.38 GiB**. The rig carries **2.65 GiB of Chrome across two
directories**, and this is the part that decides section 1 below: **Demido
fetched none of it.** Both caches were filled by other tools on the same Windows
profile.

Every row of section 4's table has the accumulating shape. `llama.cpp` is pinned
to a build and will be re-pinned, `cudart` is 489 MiB on disk and travels with
it, and uv, CPython, Node and SearXNG are all pinned archives.
[`profiles.md`](profiles.md) scopes runtimes per profile, so a second Windows
user accumulates a second copy of the whole thing.

## 0. Three states, and the state decides everything

A runtime row is in exactly one of three states. Every rule below keys off the
state and **never off a path comparison**, so the guard on deletion is written
once rather than at each call site.

| State | What it means | What Demido may do |
|---|---|---|
| **Managed** | Demido fetched this pin into the profile's runtimes folder | Verify, replace, delete |
| **Linked** | A path the user pointed at, per section 7 of `setup.md` | Read and launch it. Never write inside it, never delete it |
| **Absent** | Not fetched, or the capability was declined | Offer it, at its stated size |

**A linked runtime's bytes are never counted as disk Demido spent.** The ledger
in section 5 totals managed rows only. A `llama.cpp` the user built themselves is
not Demido's 671 MiB, and a total that claims otherwise is the same class of lie
as an unmeasured headline figure.

A linked row's only actions are **point somewhere else** and **switch to
managed**. There is no delete control on it at all, which is stronger than a
delete control that refuses.

## 1. Demido fetches Chrome itself, and never touches `~/.agent-browser`

For five of the six capability rows Demido can honour the delete rule, because it
unpacked the archive into its own folder. For Chrome it could not, and three
measured facts say why.

1. **`agent-browser install` takes no version argument.** Its whole interface is
   `install [--with-deps]`, resolving Chrome for Testing's last known good
   version at the moment it runs. The pin `setup.md` names is one the fetcher has
   no way to accept, and **a pin the fetcher cannot accept is not a pin**.
2. **`agent-browser doctor` reports the accumulation as healthy.** On the rig
   holding three Chromes it printed `9 pass, 0 warn, 0 fail`, named only the
   newest, and called the directory a `Cache dir`. Its `--fix`, documented as
   "auto-clean stale files", does not consider 840 MiB of superseded browsers
   stale. **`agent-browser` does not own this deletion and does not think there
   is a problem.**
3. **`~/.agent-browser` is not Demido's directory.** It is keyed off `HOME` with
   no relocation variable among the forty-odd `AGENT_BROWSER_*` settings, so it
   is per Windows profile, which happens to agree with `profiles.md`, but it is
   shared with every other program the user runs `agent-browser` from. On the rig
   that is exactly what happened.

**So Demido fetches `chrome-win64.zip` itself**, at the pin section 4 names, into
its own runtimes folder, and launches `agent-browser` with `--executable-path`
pointing at it. Demido owns the pin, the measured size and the deletion, and the
`~/.agent-browser` and `~/.cache/puppeteer` caches are **read by nothing and
written by nothing** in Demido.

The rejected alternative is worth naming: pruning `~/.agent-browser/browsers`
would delete files a tool the user also runs put there, which is
[`profiles.md`](profiles.md)'s "a path where one user replaces a binary another
user executes" one layer up, with a program in place of a user.

The cost is that Demido stops sharing a Chrome the machine may already hold.
Section 6 of `setup.md` already absorbs it: when a drivable browser exists, the
Chrome row is not fetched at all.

See [`docs/decisions/0004-demido-owns-its-chrome.md`](../decisions/0004-demido-owns-its-chrome.md).

## 2. Verify, then delete, and verification is a declared command

**Deleting the old pin on the strength of a successful unzip is how a user ends
up with two broken halves and nothing to run.** So every manifest row declares a
**verification command that exercises the runtime the way Demido uses it**, run
once immediately after the archive unpacks, and the predecessor survives until it
exits zero.

| Row | Verified by |
|---|---|
| `llama.cpp` | Loads the smallest model already on disk and generates one token |
| `cudart` | The same command. A CUDA build that cannot resolve `cublasLt64_13.dll` does not load a model |
| uv | Runs a one-line script on the managed interpreter |
| CPython | The same command |
| Node | `node -e` runs and exits zero |
| `agent-browser` | Opens `about:blank` with `--executable-path` at the managed Chrome |
| Chrome | The same command |
| SearXNG | Answers one query through the in-process Flask client |

Three things this table is saying deliberately.

**A version flag is not a verification.**
[#19](https://github.com/elpideus/demido-studio/issues/19) found a `llama.cpp`
that unpacks, reports its build and then hands this card the wrong CUDA archive.
Anything that passes without touching the GPU is a check that the download
finished, which is what the hash already tells us.

**A row may be verified by another row's command.** Four of the eight are, and
that is fine, because the pairs are useless apart. What is refused is a row whose
verification is its own unzip.

**At the first fetch there is nothing to delete**, so verification there gates the
row's state rather than a deletion: a row that does not verify is **absent with a
reason**, not managed.

**The check runs immediately after unpack, never later.** Waiting for the new pin
to serve a real user turn sounds safer and is not: it puts an unpredictable
deletion in the middle of a conversation days afterwards, which is the exact
shape of interruption section 4 of `setup.md` exists to prevent.

## 3. No predecessor is retained

**The superseded pin is deleted the moment the new one verifies.** Not archived,
not kept as a rollback, not moved to a `previous/` folder.

Retention is the disease this file was written about, and something that kept
three is why the ticket exists. The argument for keeping one is a rollback from a
bad pin, and it does not survive contact with the manifest: **every pin in
section 4 is a permanent upstream URL**, GitHub releases for `llama.cpp`, Node
and `agent-browser`, Chrome for Testing's own archive, astral's releases for uv.
A rollback is therefore **a re-download of a pin the app still knows**, not the
recovery of something lost.

So the previous pin stays **named** in the manifest after its bytes are gone, and
rolling back is one button and one download. It fails visibly and slowly, which
is the right way round: the alternative fails invisibly and expensively, at 420
MiB a time.

If a re-fetch is ever impossible because upstream pulled a release, that is a
defect in pinning to a source that mutates, and it is fixed there. It is not an
argument for a local museum.

## 4. The ledger is a settings page, and it is the wizard's own rows

Section 2 of `setup.md` binds this: every wizard step renders the same control
the settings page renders, so the two cannot come to disagree. **The on-disk
ledger is not a new surface.** It is the same runtime rows after the fact, on a
**Runtimes** page in settings:

`name, state, pin, size on disk, action`

where the action follows from the state: **Fetch** when absent, **Update** when a
newer pin exists, **Point at one I already have** on any row, **Remove** on a
managed row only. The wizard shows sizes before fetching; this shows them after.
It is the same table read in the other direction.

**A dedicated storage screen was drawn and rejected.** Totalling models, chats
and the session log alongside runtimes gives a second place for a number to be
wrong in, and models already have their own surface with the download folder the
brief asks for. A Runtimes page that quietly re-totals models will disagree with
that surface inside a week.

## 5. The Unused row, which is the half that catches failure

Section 3 fires only when things go right. A fetch killed halfway, a pin
abandoned when the user switched a row to linked, and a directory left by an
older Demido all leave bytes the manifest no longer names.

So the Runtimes page ends with **Unused**: every directory in the profile's
runtimes folder that **no current or previously named pin claims**, with its size
and one Remove.

It is computed by diffing the directory against the manifest, never by keeping a
list, because a list of orphans is itself a thing that goes stale and this whole
file is about state nobody reconciles. It is also the only place a user ever sees
that something went wrong, since a failed cleanup is otherwise silent by
construction.

## 6. A pin is a fact about a Demido release, and nothing else moves it

Decided on wayfinder ticket
[#30](https://github.com/elpideus/demido-studio/issues/30).

**The manifest ships inside the build.** A new pin exists because a new Demido
exists, and it got there because somebody ran it on real hardware first. Demido
**never queries upstream for versions**: not on launch, not on a schedule, not
behind a Check for updates button.

The rejected alternative is the obvious one. GitHub's releases API and Chrome for
Testing's `last-known-good-versions-with-downloads.json` would both answer, and
what they would return is **a version nobody has run**.
[#19](https://github.com/elpideus/demido-studio/issues/19) pinned `b10816` after
finding a `llama.cpp` build that unpacks, reports its build number and then hands
this card the wrong CUDA archive. A pin discovered from a release feed carries
none of that evidence, so an upstream check surfaces a version Demido has no
grounds to recommend and then asks the user to trust it anyway. That is a
recommendation dressed as an update.

**The cost is real and it already has an answer.** A `llama.cpp` fix cannot reach
a user who has not updated Demido. The user who needs it today is not stuck:
section 7 of [`setup.md`](setup.md) gives every row **point at one I already
have**, which makes the row **linked**, and section 0 above already says exactly
what Demido may and may not do with a linked binary. Running ahead of the pin is
supported. It is just not automatic, and it is the user's build rather than
Demido's claim.

This binds the cross-platform packaging question rather than waiting on it:
whatever ships Demido is also what ships its pins.

See [`docs/decisions/0005-runtime-pins-ship-with-the-release.md`](../decisions/0005-runtime-pins-ship-with-the-release.md).

## 7. Nothing is fetched without a press

**No fetch is ever automatic.** Not on the first launch after an update, not in
the background, not with a dialogue.

A new pin arrives as an **Update** action on the row, at its stated download
size, and the pin already on disk keeps working until the button is pressed.
Section 4 of [`setup.md`](setup.md) states the rule this protects: each thing
states its size before it is fetched. An automatic fetch honours that with a
dialogue nobody asked for at launch, or does not honour it at all and spends
515.5 MiB quietly. Both are the promise of the first install broken later.

Brief B11: "Guided set-up on first launch"

The brief asks for the installation and is silent on updating it, so this is a
consequence of the manifest existing rather than a requirement anybody wrote
down.

**An available update is one dot on the settings navbar icon**, resolving to the
Runtimes row. No modal, no toast, nothing at launch. A one-time notice is worse
because it is gone once dismissed, and the page alone is worse because the page
is a settings subpage nobody opens. A dot survives being ignored and is still
findable a month later.

**Updates are per row, and `llama.cpp` and `cudart` are one row's worth of
action.** There is no **Update all**. Update all exists to spend 800 MiB in one
click without reading the table, which is the opposite of what section 4 of
`setup.md` is for. The `llama.cpp` and `cudart` pairing is not a convenience:
section 2 above verifies both with one command because a CUDA build that cannot
resolve `cublasLt64_13.dll` does not load a model, so they move together or not
at all.

## 8. A pin bump is never mandatory

**A Demido release runs against the pin it ships with and against its
predecessor.** A change that cannot is a release blocker, fixed by shipping the
compatibility, not by forcing the download.

This is the rule that makes section 7 honest rather than decorative. Without it,
a release whose host code only speaks to the new `llama.cpp` leaves a user with a
working runtime and a broken app, and "never automatic" has quietly become
"automatic, or nothing answers". The cost lands on the person who can pay it,
which is whoever ships the release.

**One version back is the whole window**, and it is also section 9's.

## 9. Rollback is one pin back, and the manifest names it

Section 3 makes a rollback **a re-download of a pin the app still knows**, which
is only true while the manifest still knows it. Since section 6 puts the manifest
inside the release, an old pin would otherwise drop out on some release nobody
thought about.

**The manifest carries the current pin and exactly one predecessor.** That is the
rollback horizon, it is stated on the Runtimes page, and it is the same window
section 8 guarantees compatibility across. One number used by two rules cannot
drift apart from itself.

The rollback control is therefore a **Roll back to `b10816`** action on a managed
row, naming the pin and its download size, and it is a plain re-fetch through
section 2's verification like any other. A user who needs to go further back than
one pin wants a specific build rather than a rollback, and that is what linking
is for.

## 10. A linked runtime is verified when it is pointed at

Section 2 makes verification the gate before a deletion, run immediately after
the archive unpacks. A linked row never unpacks and never deletes, so as written
it was verified by nothing: a user could point at a `llama.cpp` that cannot
resolve `cublasLt64_13.dll` and find out days later, mid-conversation, which is
the interruption shape section 2 exists to prevent.

**So a linked row runs the same declared verification command at the moment it is
pointed at, and a failure means the row does not become linked.** It stays
**absent with a reason**, exactly as a failed fetch does. Section 2 already says
this for the first fetch, where there is nothing to delete and verification gates
the row's state instead. Linking is the other case of the same thing.

The user still owns the binary. What Demido refuses is to claim a row works when
it has not seen it work.

**After that, Demido has no opinion.** A linked row shows **the detected version
beside the pin Demido would otherwise use**, two facts side by side, with no
verdict, no marker and no mention anywhere else in the app. Calling the user's
build stale is a recommendation Demido has not earned about a binary it did not
build, and the user who linked a row opted out of version management on purpose.
If their build passes the verification command, Demido has no complaint to make.

## 11. The bytes Demido does not own are named, and that is all

`~/.agent-browser` and `~/.cache/puppeteer` held **2.65 GiB** on the rig, put
there by other tools on the same Windows profile, and section 1 makes Demido
neither read nor write them.

The Runtimes page ends with **one line naming both paths and their total**,
outside the ledger and outside **Unused**, with **no Remove control** and words
saying Demido did not put them there and will not touch them.

The question a user opens a Runtimes page with is what is eating their disk.
Answering it with Demido's own 1.35 GiB while a larger pile sits next door is
true and useless. Naming it without a button is the same honesty as the linked
row above: **Demido reports, it does not claim.** The rejected alternative,
saying nothing because a page that lists directories it does not own invites the
user to expect a button, is a page that withholds the most useful number on it to
avoid an awkward question.

## What this does not decide

- **Where the runtimes folder sits on Linux and macOS.** It is per profile per
  [`profiles.md`](profiles.md), and the path rides with the rest of the
  cross-platform question. Section 6 adds one thing to that pile: whatever
  packages and updates Demido is also what delivers a pin, so the updater and the
  manifest are the same shipment.

**Settled since, in [`releases.md`](releases.md).** Ticket
[#31](https://github.com/elpideus/demido-studio/issues/31) answered what a
release is and built the shipment section 6 assumed. Three things above are now
that file's:

- **Why a pin moved.** A pin entry carries a `note`, one line, rendered beside
  **Update**, and `check-release.mjs` requires it when the pin changed.
- **Where sections 8 and 9's rules are checked.** `scripts/check-release.mjs`,
  in the release workflow, alongside `nexus.md`'s rot gate and hard rule 3.
- **The dot in section 7 is shared.** A newer Demido uses the same dot and
  resolves to the same page, as a Demido row above the manifest rows, exempt
  from section 0's managed, linked and absent states.
