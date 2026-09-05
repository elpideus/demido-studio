# Managed runtimes: what deletes a superseded pin

Decided on wayfinder ticket
[#29](https://github.com/elpideus/demido-studio/issues/29). This file is the
back half of [`setup.md`](setup.md): section 4 states what the first fetch
costs, and this states what the folder holds a year later.

Brief B11: "Guided set-up on first launch"

The brief asks for the installation and is silent on what removes it. Everything
below is therefore a consequence of the manifest existing at all, not a
requirement anybody wrote down.

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

## What this does not decide

- **How often Demido looks for a newer pin**, or whether a runtime update is
  offered, automatic, or tied to a Demido release. This file says what happens
  when a new pin lands, not what makes one land.
- **The other caches on the machine.** `~/.agent-browser` and `~/.cache/puppeteer`
  held 2.65 GiB on the rig and Demido now neither reads nor writes them. Whether
  the Runtimes page should say so, as a courtesy rather than as a claim, is not
  decided.
- **Cross-platform paths.** The runtimes folder is per profile per
  [`profiles.md`](profiles.md), and where that sits on Linux and macOS rides with
  the rest of the cross-platform question.
