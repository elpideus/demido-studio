# A release is the only thing that reaches a machine

Decided on wayfinder ticket
[#31](https://github.com/elpideus/demido-studio/issues/31).

[`runtimes.md`](runtimes.md) section 6 made the release the only thing that moves
a runtime pin, and then leaned on it three times without it existing. This file
is that thing: what a release is, how an installed Demido learns a newer one
exists, what an update replaces, and where the rules that fire only at release
time are checked.

Brief B03: "windows-only at first, during the pre-release versions"

Brief: silent

The brief asks for the installation and says nothing about updating it. B03 is
the only line this file answers, and it answers the near half of it: what a
Windows-only pre-release ships as. The far half, cross-platform close to 1.0,
stays with the vault and the per-platform paths.

## 0. A release is a tag, and it carries one installer

A Demido release is a **signed `vX.Y.Z` tag on `main`**. CI builds it on
`windows-latest` and attaches, to a **published** GitHub release:

- one **NSIS installer**, built by Tauri's own bundler,
- `latest.json`, signed with the minisign key of section 7,
- generated notes, plus the **Pins** section of section 8 when a pin moved.

Published rather than draft, because the updater cannot read a draft. v2's
workflow drafted its releases and nothing consumed them but a human.

**One installer, not two.** v2 shipped `msi` and `nsis` together, which is two
upgrade paths, two uninstall behaviours and two things to test, for one person.
NSIS is the one that survives because Tauri's NSIS target supports
`installMode: currentUser` and its MSI target is per-machine only. That is not a
preference here:
[`profiles.md`](profiles.md) made a Demido profile a Windows profile, so a
per-machine install on a machine with several Windows users recreates the shared
writable directory that file exists to refuse. **A per-machine install is
refused**, not merely not the default.

Tauri's bundler is the installer system, and NSIS is one of the two things it
emits. Nothing third-party is added to reach it, so hard rule 3 is untouched.

## 1. Demido checks for itself, and the objection that killed the other two checks does not reach it

Section 6 of [`runtimes.md`](runtimes.md) refused to query upstream for versions
because a release feed answers with **a version nobody has run**. Section 5 of
[`nexus.md`](nexus.md) refused a launch probe because a volunteer commons should
not pay for information the user usually does not need.

Neither objection survives contact with Demido's own releases. A Demido release
exists **because somebody ran it on real hardware first**, which is the exact
evidence section 6 demanded and could not get from ggml-org, and the endpoint is
GitHub rather than a commons anyone is donating to.

**So Demido asks its own endpoint.** Not during startup, and at most once a day:
`https://github.com/elpideus/demido-studio/releases/latest/download/latest.json`,
which is a permanent URL that always resolves to the newest published release.

## 2. Demido is the first row of the Runtimes page

There is **one** notification vocabulary in this app and section 7 of
[`runtimes.md`](runtimes.md) already spent it: one dot on the settings navbar
icon, no modal, no toast, nothing at launch. A newer Demido uses that same dot
and resolves to the same page.

**The Runtimes page gains a Demido row above the manifest rows**, with the same
columns and the same stated download size. The ordering is the honest one, since
updating Demido is the only thing that moves every pin under it.

**That row is exempt from the state vocabulary, and the exemption is written
rather than inferred.** Section 0 of `runtimes.md` gives every row a state of
managed, linked or absent and derives its action from that state. The Demido row
has none of them: it is never **linked** (there is nothing to point at), never
**absent** (it is running), carries no **Remove** (uninstalling is the operating
system's job, per section 5) and no **Roll back** (section 9's horizon is about
pins the manifest names; an older Demido is a manual install of an older
release). It carries name, current version, newer version if there is one, size,
and one **Update**.

The rejected alternative is an About page with its own update surface, which
re-opens the thing this section closes: two notification stories, one too many.
A table where one row quietly obeys different rules is where a reader assumes a
symmetry that is not there, so the row states its own exemption in place.

## 3. Nothing is fetched without a press, and first launch never checks

Section 7 of [`runtimes.md`](runtimes.md) binds the app the same way it binds a
runtime. The check is a check. **The download happens on the press and not
before**, at the size the row states, and Tauri's updater performs it.

**First launch does not check.** The wizard is already spending 818.7 MiB of the
user's attention ([`setup.md`](setup.md)), and a version check on top is noise at
the one moment the build is current by definition.

## 4. The check is one call and one switch

The call reveals an IP and a version to GitHub and carries nothing else.
[`accounts.md`](accounts.md)'s egress gate does not reach it, since no account
data is involved.

**It is on by default with one switch on the Runtimes page**, and it is the only
network call Demido makes about itself. The switch exists because an app whose
thesis is that everything the model sees is inspectable cannot also phone home
silently. **Turned off, the row stays and says checking is off**, because a row
that disappears is a setting the user cannot find again.

## 5. An update replaces binaries and names nothing else

A machine can hold several profiles, each with its own runtimes folder, vault,
chats and session log ([`profiles.md`](profiles.md)).

- The installer replaces **application binaries only**.
- `deleteAppDataOnUninstall` is set to `false` **explicitly**, though that is
  already the default, because it is the load-bearing one and a default is not a
  decision.
- **Uninstall leaves every profile's data where it is and says so on the way
  out.** The alternative is a checkbox that deletes a vault.

## 6. The rules that fire only at a tag live in `check-release.mjs`

`check-rules.mjs` stays what it is: the per-commit hard rules of
[`AGENTS.md`](../../AGENTS.md), run on every push. **A second script,
`scripts/check-release.mjs`, runs in the release workflow before the build**, and
failing it fails the tag build. That is what "does not ship" means, in the three
places that already said it and had nowhere to point:

1. **The predecessor pin** (`runtimes.md` section 8). The manifest carries the
   current pin and exactly one predecessor, and the release runs against both.
2. **Source rot** (`nexus.md` section 5). No source's `verified_on` is more than
   90 days old.
3. **Nothing third-party bundled** (hard rule 3). The built bundle is inspected
   for third-party binaries.

The third is why this script is worth a file of its own. Hard rule 3 has been
enforced by review since
[#16](https://github.com/elpideus/demido-studio/issues/16), for want of an
installer to check. Section 0 produces the installer, so the rule gets its
checker and `AGENTS.md` reaches **zero review-enforced hard rules**.

A fourth check is free and prevents a whole class of confusion: **the tag matches
the version in `tauri.conf.json` and `Cargo.toml`**.

## 7. Two keys, and only one of them costs money

**Minisign, now.** Tauri's updater refuses an unsigned `latest.json`. The private
half is a GitHub Actions secret with an offline copy in Stefan's own vault; the
public half is compiled into every build.

The failure worth writing down is the asymmetric one: **a lost private key cannot
be replaced through the updater**, because a new public key only reaches a
machine inside a build the old key would have to sign for. Every installed copy
stops accepting updates, permanently. The mitigation already exists and is
section 3 of [`runtimes.md`](runtimes.md)'s: no museum, because the manual
install is always there. **A lost key degrades to "download the installer from
GitHub again"**, which fails visibly and slowly. That is what buys permission to
keep the key handling this simple. **Rotation is a release-notes event, never
routine.**

**Authenticode, parked with its trigger named.** Without a certificate, every
NSIS download raises SmartScreen. An OV certificate runs roughly US$200 to $600 a
year on one person, on top of the domain
[#17](https://github.com/elpideus/demido-studio/issues/17) is already waiting on,
and a certificate wants an identity to be issued against, so **the trigger is the
same domain**. SmartScreen reputation accrues from download volume regardless.
Until then the README says the installer is unsigned and why, which is honest
rather than cheap.

## 8. A pin that moves carries a line, and CI requires it

Section 7 of [`runtimes.md`](runtimes.md) leaves the user a dot saying a newer
pin exists and a row saying how big it is. Neither says what changed.

**A pin entry carries a `note`: one line, why it moved.** It renders under the
row beside **Update**. `check-release.mjs` **requires it when the pin changed**,
and a pin that did not move renders nothing rather than linking upstream.

The person who moved the pin is the only person who ran it, so they are the only
honest source for that sentence. Pointing the user at upstream's release notes is
section 6's rejected alternative in another costume: notes written by people who
never ran it on this card, which is a recommendation dressed as an update.

## 9. Rot stays a release gate

Section 5 of [`nexus.md`](nexus.md) made a 90-day-old `verified_on` a release
gate and explicitly not a runtime one, when nothing could tell a user anything
about their own build. Section 1 above builds exactly that channel, and **section
5 stays as written anyway.**

The actionable half is already covered: the dot and the Demido row say a newer
build exists. A second banner saying the current build is old is the competing
notification story section 2 refused. And the unactionable half is already
honest, since `nexus.md`'s exhausted walk is a message naming what was tried
rather than a silent failure. A rot warning would tell a user something is
probably stale **before anything has failed**, which is a claim about a source
Demido has not called.

## What this does not decide

- **Cross-platform packaging.** B03's far half. What a Linux or macOS release is
  built from and installs as rides with the vault and the per-platform paths in
  the map's fog. Section 0's NSIS argument is Windows reasoning and does not
  transfer on its own.
- **A release cadence.** Nothing here says how often a release happens, only what
  one is and what it is checked against.
- **What `latest.json` says beyond the updater's own fields.** The Pins section
  of section 8 is release notes, read by a person on GitHub. Whether the in-app
  row shows anything more than the pin's own `note` is undecided.
