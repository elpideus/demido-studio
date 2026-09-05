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
| **Capabilities** | uv, a Python interpreter, SearXNG, Node, `agent-browser`, Chromium | Each is a feature of the brief that silently does not exist without it. |

**Required is not a synonym for forced.** A user can leave with neither, and
Nexus answers on rung 0 while they decide. What "required" names is the honest
consequence: this group is what separates a Demido that answers from one that
cannot.

**Capabilities are a checkbox each, on by default, with the size beside them.**
This keeps everything-at-set-up as the path while leaving a machine with 400 MB
free an exit that is not a lie. A capability turned off here is offered again at
the point its feature is first reached, in the same size-stated words, because
that dialogue has to exist anyway for a user who changes their mind.

**The sizes are measured before they are shown.** Exactly one number in the
manifest is known today: 516 MB for the two `llama.cpp` archives, from #19. The
rest are pinned by
[#27](https://github.com/elpideus/demido-studio/issues/27), the same way #19
pinned the backend, and no estimate of mine reaches a screen. A wizard whose
headline figure is a guess is a wizard that lies in its first sentence.

## 5. Node is fetched, and that is the price of the browser rung

**`agent-browser` is an npm CLI**, which v2 met as a `.cmd` on Windows that
`CreateProcess` would not run. Fetching it reaches through to a Node runtime and
a Chromium download. So Demido fetches Node.

**Named plainly because it is the largest consequence of section 4**: a
JavaScript runtime enters a Rust application's dependency list for the sake of
one CLI. The alternative was speaking CDP directly, which is a browser
automation stack Demido would then own; the brief asks for the feature, not for
that.

**Node is treated exactly as uv is treated, and for the same reason.** Never
preferred over one already on `PATH`. `demido-python`'s sentence carries over
unchanged: "Somebody who has a package manager installed has an opinion about
how it is kept up to date, and an app that quietly used a second copy would be
overriding that opinion with nothing to say for itself."

## 6. Once per profile, and the duplication is deliberate

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

## 7. The connected route is a link, not the opening fork

v2 asked managed-or-connected first: "Everything else follows from it." **v3
takes the branch instead of presenting it.** Managed is the path; "already
running a server? point Demido at it" is a link on the runtime row.

A new user cannot answer whether they run their own inference server, and asking
puts the vocabulary of the entire product on screen one. `Answers::Route`
survives in the model layer untouched; what changes is who chooses it and when.

## 8. How it is driven live

Set-up runs once, so a machine that has been set up cannot test it. **A fresh
Windows profile is the fixture.** Making a second Windows user costs nothing,
gives a genuinely empty profile (which `profiles.md` guarantees, since Windows
scopes it), and exercises section 6's duplication on the same run.

**This raises S1's gate** ([`done.md`](done.md)). S1 was going to start from
Stefan's already-configured machine, which is the assumption v2 died of. It now
starts from nothing: new Windows user, wizard from zero, first message answered,
screenshot and trace.

## What this does not decide

- **The sizes and the licenses of the manifest.**
  [#27](https://github.com/elpideus/demido-studio/issues/27) measures them
  upstream, as #19 did for `llama.cpp`.
- **Which rungs Nexus offers and in what words.** That is
  [`nexus.md`](nexus.md)'s, and this file inherits it rather than re-deciding it.
- **What a capability's own first-use offer looks like** when it was turned off
  here. The words are Nexus's shape; the surface is not drawn.
