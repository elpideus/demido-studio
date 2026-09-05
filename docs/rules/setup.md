# Set-up, on a machine that has nothing on it

What the guided set-up decides, what it fetches, and what it is allowed to
leave for later.

Decided on wayfinder ticket
[#21](https://github.com/elpideus/demido-studio/issues/21).

Brief B11: "Guided set-up on first launch"

## The failure this exists to prevent

Two of them, and they pull in opposite directions.

**v2 built the reasoning and never ran it.** `demido-setup` is a good crate:
`Answers` is what the user chose, `Plan` is what that leaves outstanding and is
derived on every read so there is no second copy to fall out of date, and
`target` exists so the wizard and the chat panel cannot disagree about which
model answers. All of it unit-tested. None of it ever driven from a fresh
machine to a model that answered. It is the built-but-not-working failure in one
crate.

**v2 deferred every runtime to first use, and the brief paid for it.** The
browser rung is honest about this in its own words: `agent-browser` "is found on
`PATH` when the user has it and absent otherwise. A machine without it keeps the
honest sentence about a page that builds itself in a browser." That honest
sentence sits where the brief asked for a feature:

Brief B18: "Native web search and fetch"

SearXNG had the same shape, installing itself in the background on the first
search that needed it because "a person who typed a question is waiting". A
person who typed a question is waiting because nobody asked them the question at
the moment they were expecting to be asked questions.

## 1. It is guided, and it can be left

**Set-up is the front-and-centre path on first launch, and every step has a way
out.** It is not skipped past, not minimised by default, not a row in a corner.
It is what the window is doing when it opens.

**Leaving is always possible and never final.** A user who leaves lands on the
desk with Nexus already answering
([`nexus.md`](nexus.md) §1: Nexus is what answers while the download runs) and a
persistent row offering the rest of set-up. The `Plan` is derived on every read,
so what is outstanding is always recoverable and never has to be remembered.

This is the reading of the rule that binds:

> **Startup never blocks.** A subsystem that fails is reported and skipped; the
> app still reaches a usable state. Never trap the user on a boot screen.

The banned screen is one you **cannot** leave. A screen almost nobody **should**
leave is a different thing, and the difference is a door, not a nudge.

## 2. The surface is a wizard window, and its controls are the settings page's

**A wizard window over the desk** (#7's Shell A: chat is the desk, everything
opens over it), from the family of windows in
[`design/windows.md`](../../design/windows.md).

**Every step renders the same control the settings page renders.** One component
per decision, two hosts. The accelerator row in the wizard and the accelerator
row in settings are the same code, so they cannot come to disagree, and nothing
built here is thrown away when set-up is over.

This is `demido-setup`'s own lesson moved up a layer. The crate already keeps
one `target` because "both the wizard's last step and the chat panel ask that,
and asking it in two places is how they come to disagree". The UI has the same
failure available to it.

## 3. Detected, pre-selected, always shown, always overridable

The brief asks for a **selector**:

Brief B11: "Guided set-up on first launch (GPU & GPU Ecosystem (CUDA, ROCm, etc.) selector, Runtime & Dependencies installation, etc.)"

**Nothing detectable is ever asked as a question.** The accelerator step opens
already answered, with the reason attached:

> NVIDIA, driver reports CUDA 13.2 → CUDA 13.3 build, 516 MB

and an override listing the alternatives. On a machine with one candidate it is
a settled row rather than a choice, which is `demido-setup`'s existing rule
("a step with one possible answer is not a step") with one amendment: **it is
still rendered.** A step that answers itself says so, per `AGENTS.md` rule 6,
and the brief's word `selector` is then honestly present rather than argued
away.

**CUDA and CPU only in the first cut.** ROCm and Vulkan are rendered as real
rows that say no build is fetched for them yet. They are not hidden, because the
brief names ROCm; they are not claimed, because
[#2](https://github.com/elpideus/demido-studio/issues/2) marks
`demido-hardware`'s adapter probe a **rewrite** rather than a port (DXGI-only by
design) and there is no AMD card on the rig. Shipping an untested ROCm path is
the built-but-not-working failure with a fresh coat on.

**One defect ports with the selector rather than being inherited quietly.**
[#19](https://github.com/elpideus/demido-studio/issues/19) found `demido-catalog`
picks the newest build whose toolkit is at or below the driver's version, which
hands this card the 12.4 archive: 254 MB instead of 143 MB for no gain, when the
13.3 archive initialises CUDA and offloads normally on a 13.2 driver. Minor
version compatibility is the guarantee; the **major** version is the gate.

## 4. Everything Demido needs arrives here, not at first use

**This overrules `AGENTS.md` rule 3's timing.** The rule said runtimes are
fetched "at first use". They are fetched **at set-up**. Nothing else about the
rule moves: nothing is bundled into the installer, nothing is pre-installed, and
each thing still states its size before it is fetched.

The reason is section 0's second failure. Deferral looks like restraint and
spends the user's attention at the worst possible moment: mid-question, with a
progress bar where an answer should be, or with an apology and no progress bar
at all. Set-up is the one moment in the product's life when a person has agreed
to be asked about downloads.

**The manifest, in two groups.** Both are offered with sizes; the second is a
list of checkboxes, all on by default.

| Group | What | Why it is here |
|---|---|---|
| **Required** | `llama.cpp` + `cudart`, one model | Without them nothing answers. |
| **Capabilities** | uv, a Python interpreter, SearXNG, Node, `agent-browser`, and Chrome for Testing if the machine has no browser Demido can drive | Each is a feature of the brief that silently does not exist without it. |

**Required is not a synonym for forced.** A user can leave with neither, and
Nexus answers on rung 0 while they decide. What "required" names is the honest
consequence: this group is what separates a Demido that answers from one that
cannot.

**Capabilities are a checkbox each, on by default, with the size beside them.**
This keeps everything-at-set-up as the path while leaving a machine with 400 MB
free an exit that is not a lie. A capability turned off here is offered again at
the point its feature is first reached, in the same size-stated words, because
that dialogue has to exist anyway for a user who changes their mind.

**The sizes are measured before they are shown.** A wizard whose headline figure
is a guess is a wizard that lies in its first sentence, so every figure below
was fetched from upstream on 2026-09-05 by
[#27](https://github.com/elpideus/demido-studio/issues/27) and none of them is
an estimate. **Download** is the byte count the server sends, from the response
header. **On disk** is what the archive expands to, read out of its own index.
Both are MiB, 1024 by 1024, which is also what #19's `143 MB` and `373 MB` are.

| Group | What | Pin | Download | On disk | License |
|---|---|---|---|---|---|
| Required | `llama-b10816-bin-win-cuda-13.3-x64.zip` | `b10816` | 142.6 | 182.6 | MIT, ggml-org |
| Required | `cudart-llama-bin-win-cuda-13.3-x64.zip` | same release | 372.9 | 489.0 | NVIDIA CUDA EULA |
| Required | one model | the user's choice | varies | varies | the model's own |
| Capability | `uv-x86_64-pc-windows-msvc.zip` | `0.12.10` | 16.2 | 40.2 | MIT or Apache-2.0, astral-sh |
| Capability | `cpython-3.12.14+20260901-x86_64-pc-windows-msvc-install_only_stripped.tar.gz` | `20260901` | 21.0 | 60.4 | PSF-2.0 |
| Capability | SearXNG, its archive and its environment | commit `a30b2d47` | 23.7 | 70.7 | AGPL-3.0-or-later |
| Capability | `node-v24.20.0-win-x64.zip` | `v24.20.0` | 35.8 | 101.8 | MIT, nodejs |
| Capability | `agent-browser-win32-x64.exe` | `v0.36.0` | 13.2 | 13.2 | Apache-2.0, vercel-labs |
| Capability | `chrome-win64.zip`, **only if none is detected** | `152.0.7977.82` | 193.4 | 427.9 | Google Chrome Terms of Service |

**Required is 515.5 down and 671.6 on disk before a single model. Every
capability ticked is 303.2 down and 714.2 on disk.** A machine that accepts all
of it and downloads nothing to answer with spends 818.7 MiB of network and 1.35
GiB of disk, and 63 per cent of that disk is one browser. **On a machine that
already has a browser Demido can drive, section 6 removes that row and the
capability group falls to 109.8 down and 286.3 on disk**, which is the single
largest saving available anywhere in this manifest.

**Four things the measurement turned up that this file could not have said
before it.**

**Chrome is the largest thing in the manifest, and it is neither Chromium nor
open source.** `agent-browser` fetches Google's Chrome for Testing channel,
resolved through
`googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json`,
which is Google Chrome under the Google Chrome Terms of Service. Its 308-entry
archive contains no license file of any kind: Chrome carries its credits inside
the binary, at `chrome://credits`, which is the only place a user can read them.
It is also the one row that is often already on the machine, since
`agent-browser` detects an existing Chrome, Brave, Playwright or Puppeteer
install and takes `--executable-path` for anything else that speaks CDP.
Section 6 answers whether the manifest fetches it at all: only when the machine
has no browser Demido can drive.

**373 of the required 516 MiB is NVIDIA's, not ggml-org's.** The `cudart`
companion is three redistributable DLLs (`cublasLt64_13.dll` alone is 439 MiB
expanded) under the NVIDIA CUDA Toolkit EULA, not under `llama.cpp`'s MIT. #19
recorded the size and not the ownership, and the licenses folder the brief asks
for has to carry both.

Brief B46: "A licenses folder will also be needed"

**SearXNG is AGPL-3.0-or-later**, the one copyleft dependency in the manifest,
and its 39 wheels are not: they resolve to MIT, BSD, Apache-2.0, MPL-2.0, ISC
and PSF-2.0 with nothing viral among them. v2 already built for the AGPL rather
than around it, running SearXNG through Flask's test client in-process so that
no socket is ever bound and the network clause is never engaged. That reasoning
survives into v3 and is the reason the rung is a worker rather than a server.

**The Python that uv fetches is stripped, and it still carries its license.**
uv resolves `3.12` to `install_only_stripped`, 21.0 MiB against 46.2 for the
full variant, and the difference is debug symbols. `LICENSE.txt` is at the root
of both.

## 5. Node is fetched, and the browser rung is not why

**This section had the wrong reason in it until #27 read the package.**
`agent-browser` is an npm CLI in the sense that npm is where most people get it,
and v2 met it as a `.cmd` on Windows that `CreateProcess` would not run. It is
not a Node program. It is a native Rust binary published per platform, its npm
`bin` is a nine-line shim that spawns the right one, and its own README says the
daemon needs neither Node nor Playwright. The same binaries are release assets
on `github.com/vercel-labs/agent-browser`, one file, 13.2 MiB, fetched exactly
the way `llama.cpp` and uv are fetched.

**So the browser rung costs no JavaScript runtime at all**, and the npm route
costs 39.4 MiB to download and 88.7 on disk for seven platforms' binaries when
one is wanted. Demido takes the release asset.

**Node stays in the manifest for the reason the brief actually gives it.** A
skill ships its own MCP servers, and the brief's own example of one is
`npx mcp-remote`:

Brief B52:

> skills also provide the required mcps and tools instead of the user having to grab them manually

A machine with no Node runs that skill's server not at all, which is the
`agent-browser` failure with a different name. So Node is a capability in its
own right, ticked for MCP rather than for the browser, and a user who runs no
npm-published server can leave it unticked without losing the browser.

**Node is treated exactly as uv is treated, and for the same reason.** Never
preferred over one already on `PATH`. `demido-python`'s sentence carries over
unchanged: "Somebody who has a package manager installed has an opinion about
how it is kept up to date, and an app that quietly used a second copy would be
overriding that opinion with nothing to say for itself."

## 6. The browser is detected before it is fetched, and Edge does not count

Decided on [#28](https://github.com/elpideus/demido-studio/issues/28), which
[#27](https://github.com/elpideus/demido-studio/issues/27) forced by measuring
the row: Chrome is 193.4 MiB down and 427.9 on disk, larger than `llama.cpp` and
its CUDA runtime together, and 63 per cent of the disk the whole manifest asks
for.

**Chrome is a conditional row, not an unconditional one.** The wizard probes for
a browser Demido can drive before it offers to fetch one, and section 3's rule
applies unchanged: nothing detectable is ever asked as a question, so a machine
with a usable browser opens on a settled row with the reason attached, and a
machine without one opens on a checkbox with 193.4 MiB beside it.

> Chrome 152.0.7977.82 found at `C:\Program Files\Google\Chrome\Application` →
> no download

**What counts as detected: Chrome, Brave, Vivaldi, Opera. Not Edge.** This is
the part that had to be measured rather than reasoned, because Edge is Chromium,
Edge is on every Windows 11 machine, and Edge therefore looks like the answer
that makes this whole row disappear. It is not.

Three results from the rig on 2026-09-05, driving Edge 153.0.4234.19 through
`agent-browser` v0.36.0:

1. **It works.** `AGENT_BROWSER_EXECUTABLE_PATH` pointed at `msedge.exe`, then
   `agent-browser open https://example.com` returned `✓ Example Domain` and
   exit 0. Again with `--no-first-run --no-default-browser-check`: same result.
   So there is no CDP-level objection, and the honest statement is not "Edge
   cannot be driven".
2. **It is slow, and the rig could not prove the slowness is Edge's.** Both
   successful opens took over two minutes, and a third on a warm profile was
   killed still hanging past three. The obvious next measurement, the same
   open against the fetched Chrome on the same rig, **was attempted twice and
   completed neither time**, hanging with Chrome processes alive and no result.
   So the comparison does not exist, and the honest statement is that
   `agent-browser` starts slowly on this machine and nothing here attributes
   that to Edge. **This is not a leg the decision stands on**, and it is written
   down so that nobody later cites it as one.
3. **The binary forwards arguments to the running instance.** `msedge.exe
   --version` printed `Opening in existing browser session.` rather than a
   version. That is Edge's singleton hand-off. It is the real objection, and it
   is a *variance* objection rather than a performance one: a detected-Edge path
   behaves one way on a machine where Edge is closed and another where it is
   open, which is the same class of failure as an engine that updates underneath
   the app.

**So Edge is reachable and not offered, on variance rather than on speed.**
Chrome, Brave, Vivaldi and Opera are installed deliberately by someone who
wanted them and sit still when they are not being used. Edge is present on every
Windows 11 machine whether or not anyone chose it, and is the most likely
browser on the machine to be *running* while Demido works. Detecting it would
make the rung's behaviour depend on whether the user happens to have a window
open. Section 7's existing escape covers it
without a special case: every runtime row offers "point at one I already have",
and a user who points that row at `msedge.exe` gets exactly what the rig got.
The row says so in one line, because a user who can see Edge on their taskbar
deserves the reason rather than silence:

> Microsoft Edge is not offered. It is driveable and unreliably slow to start;
> point this row at it yourself if you want it.

**`--auto-connect` is refused, and named as refused.** `agent-browser` can
discover and attach to a Chrome the user is already running, which would cost
nothing to download and hand the model the user's live profile, cookies and
logins. [`stack.md`](../stack.md) split the browser into two engines precisely
so that a page the model opens cannot reach what the user is signed in to. The
flag is in `agent-browser --help` and somebody will find it, so it is refused
here in writing rather than left unmentioned.

**`--engine lightpanda` is out of scope for the manifest.** `agent-browser`
carries a second, non-Chromium engine that would change the size argument
completely. The brief says what the browser is for:

Brief B35: "Integrated basic Web Browser that both users and LLMs have access to"

The use named there is a model checking a site it has just built. An engine that
renders a subset of the web fails at the only job the rung has, so the saving is
not available at this quality bar. Recorded so the next reader does not reopen
it on the size argument alone.

**What the fetch still costs when it happens, and what is done about it.** The
rig carried **three** fetched Chromes in `~/.agent-browser/browsers`: 420 + 420 +
429 MiB, **1.24 GiB**, because nothing removes the previous pin when a new one
lands. `profiles.md` scopes runtimes per profile, so a second Windows user pays
it again. Every runtime in section 4's table has this shape and Chrome is only
where it was first noticed, so **Demido deletes the superseded pin once the new
one verifies**, and the runtimes surface lists what is on disk with its size.
That is a manifest-wide rule, not a Chrome one.

## 7. Once per profile, and the duplication is deliberate

[`profiles.md`](profiles.md) scopes managed runtimes to the profile: "A shared
writable runtime directory is a path where one user replaces a binary another
user executes." That argument was made about one binary. It is **stronger** now
that the list is five, so the scoping stands and a second Windows user fetches
everything again.

**Two escapes, both the user reaching outside their own profile on purpose,
neither asking for elevation and neither creating a machine-wide directory.**

- **The model folder is pre-filled** from any readable model folder already
  visible on the machine, for the user to confirm. The mechanism is the brief's
  own: Brief B55: "Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks."
- **Each runtime row offers "point at one I already have"**, a path to a binary
  the user owns.

## 8. The connected route is a link, not the opening fork

v2 asked managed-or-connected first: "Everything else follows from it." **v3
takes the branch instead of presenting it.** Managed is the path; "already
running a server? point Demido at it" is a link on the runtime row.

A new user cannot answer whether they run their own inference server, and asking
puts the vocabulary of the entire product on screen one. `Answers::Route`
survives in the model layer untouched; what changes is who chooses it and when.

## 9. How it is driven live

Set-up runs once, so a machine that has been set up cannot test it. **A fresh
Windows profile is the fixture.** Making a second Windows user costs nothing,
gives a genuinely empty profile (which `profiles.md` guarantees, since Windows
scopes it), and exercises section 6's duplication on the same run.

**This raises S1's gate** ([`done.md`](done.md)). S1 was going to start from
Stefan's already-configured machine, which is the assumption v2 died of. It now
starts from nothing: new Windows user, wizard from zero, first message answered,
screenshot and trace.

## What this does not decide

- **Which rungs Nexus offers and in what words.** That is
  [`nexus.md`](nexus.md)'s, and this file inherits it rather than re-deciding it.
- **What a capability's own first-use offer looks like** when it was turned off
  here. The words are Nexus's shape; the surface is not drawn.
