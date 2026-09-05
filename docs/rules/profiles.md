# Profiles, and what separates one from another

Who a Demido user is, where their things live, what protects them, and what
that protection does not cover.

Decided on wayfinder ticket
[#15](https://github.com/elpideus/demido-studio/issues/15).

## The word

**Profile** is a person using Demido on this machine. **Account** is a
credential at somebody else's service: a Google account, a Bybit account, a
TradingView login.

The brief uses "account" for both, which is why this section comes before the
rest. A threat model that says "an account cannot read another account's
accounts" carries no information. Everywhere in this repo, from here on: a
profile is a person, an account is a login somewhere else. `docs/stack.md` and
`docs/rules/lessons.md` were written before the split and said "per account"
where they meant per profile; both are corrected.

## The identity model

**A Demido profile is a Windows profile.** One OS user, one profile. No in-app
profile list, no profile switcher, no login screen. Somebody who wants a second
Demido profile makes a second Windows user, which is the mechanism Windows
already ships and the one every file under `%LOCALAPPDATA%` is already
protected by.

Brief B12:

> Multi-account system, allowing different users to use the software on the same machine (think families, friends in college rooms, etc.)

The requirement is met by the operating system rather than by Demido. This
amends the brief: the flatmate scenario is served by a second Windows account,
not by a second identity inside one Windows account. See the Amendments table in
[`docs/brief-map.md`](../brief-map.md).

This is the position v2 also reached, in `demido-core/src/paths.rs`:

> One profile per OS user; Windows already provides the isolation that an
> in-app multi-user system would have duplicated.

The difference is that v2 arrived there silently, as a doc comment on a paths
module, with brief B12 left looking unbuilt. This repo arrives there as a ruling
with its cost written down.

**The cost, stated plainly.** Two people sharing one Windows login share one
Demido profile completely: the same chats, the same projects, the same Google
tokens, the same broker keys. Demido does not detect this and does not warn
about it, because it cannot tell two people apart at one login without asking
for a passphrase, and asking for a passphrase is the design that was rejected.
The brief's college room is served only if the room sets up Windows users.

**What this buys.** There is no Demido login, no passphrase, no key-derivation
function, no lock state, no unlock prompt, no forgotten-passphrase path, and no
second security boundary that could be subtly weaker than the first. v2's ADR
0025 objection to a passphrase stands unanswered because nothing now asks it:

> Demido runs background turns, restarts, and reads mail on a timer; a
> passphrase means either prompting at every launch for something people will
> choose badly, or caching it, which is the first shape wearing a hat.

Background turns, timers and restarts all keep working, because there is no
locked state for them to wait on.

## The key

**v2's ADR 0025 survives unchanged, and it is now the whole answer rather than
half of one.** A random 32-byte key encrypts every secret with
ChaCha20-Poly1305, and the operating system wraps that key: DPAPI on Windows.
The wrapped key sits in the vault beside the secrets, which is safe precisely
because it is useless anywhere else.

The fit is exact, and that is what makes the identity ruling coherent rather
than merely cheap. DPAPI derives from the Windows user's login credentials and
refuses another account and another machine. If a profile is a Windows user,
then the key boundary and the identity boundary are the same boundary, drawn by
the same authority, with no seam between them for a bug to live in.

**The `Keeper` trait ports as written**: `what`, `wrap`, `unwrap`, no
lifecycle, with the contract suite that comes with it. The macOS keychain and
the Linux secret service are the second and third implementations and they are
still two functions each. DPAPI's optional entropy argument stays unused, for
v2's reason: a second secret Demido would have to store is a second secret to
protect.

**Where there is no keeper, there is no vault.** v2's answer stands, and this
ruling removes the escape hatch a passphrase layer would have offered: a Linux
build with no running secret service has no accounts at all, rather than a
passphrase-protected vault instead. That is a real cost of making the OS the
only key holder. It lands on Linux, not on Windows, and it is a cross-platform
packaging problem rather than an architectural one.

## What is scoped and what is shared

Windows scopes the profile. This table is about which of Demido's things sit
inside that boundary, and which deliberately sit outside it.

| Thing | Where | Why |
|---|---|---|
| Chats, projects, session log, artifacts, snapshots | Profile | The content the person authored. |
| Settings, keybinds, shell layout, prompts, characters | Profile | Theirs by definition. |
| Skills | Profile | A skill is code that runs inside a turn. A machine-wide skills folder would let one Windows user install something that executes in another's session. |
| Lessons | Profile | Already ruled on [#13](https://github.com/elpideus/demido-studio/issues/13). |
| Accounts, and every secret | Profile | DPAPI enforces this whether or not Demido intends it. |
| Code graphs | Profile | Derived and rebuildable, which argues shared, but a graph names every path in a private project. Privacy wins over disk. |
| Managed runtimes: uv, Python, Node, Chromium, llama.cpp | Profile | Executables Demido fetched and will later run. A shared writable runtime directory is a path where one user replaces a binary another user executes. Duplicated on purpose, and the argument is stronger now the list is five rather than one ([`setup.md`](setup.md) §6). |
| Model weights | Shared, by the user's own choice | Tens of gigabytes. Two Windows users on one machine should not each download Gemma. |

**Model weights are shared by pointing, not by privilege.** Demido creates no
machine-wide directory and asks for no elevation. The mechanism the brief
already specifies does the whole job:

Brief B55: "Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks."

A household that wants one copy of the weights puts them where both Windows
users can read them and adds that folder in each profile. The default download
folder stays inside the profile, because a default that writes outside the
profile is a default that needs permissions Demido should not ask for on first
launch.

**Set-up makes both escapes one click rather than a settings expedition**
([`setup.md`](setup.md) §6): the model folder field is pre-filled from any
readable model folder already on the machine for the user to confirm, and every
runtime row offers a path to a binary the user already has. Both are the user
reaching outside their own profile deliberately, which is what this section
permits and what elevation would not be.

**"Global" now means profile-global.** The settings ladder from
[#8](https://github.com/elpideus/demido-studio/issues/8) is global, then model,
then chat, then character. Its top rung is the profile. There is no
machine-global settings tier, and nothing is shared between Windows users except
the weights they chose to point at.

## The threat model

Written so the settings page has something true to print, in the register ADR
0025 set: a sentence that says who cannot read this, and therefore implies who
can.

**Defended against.** The vault file copied off the disk. A backup. A sync
folder. Another Windows user on this machine reading your profile directory. The
disk pulled out and mounted elsewhere. In every one of these the wrapped key is
present and worthless, because DPAPI refuses another account and another
machine.

**Not defended against, and never claimed to be.**

- **Code running as you, on this machine.** It asks the OS to unwrap the key
  exactly as Demido does. Nothing on a desktop defends against this.
- **Anyone with your Windows password.** They are you, as far as DPAPI is
  concerned.
- **A machine administrator.** An admin reads another user's profile directory,
  and an admin who resets your Windows password destroys the DPAPI master key
  the old password protected. That seals the vault rather than opening it: the
  secrets become unrecoverable, not exposed.
- **A second person at your Windows login.** This is the brief's own scenario
  and Demido does not address it. See the cost stated above.
- **Chats, projects and the session log at rest.** These are plain files inside
  the profile, protected by the Windows ACL on `%LOCALAPPDATA%` and nothing
  else. Only secrets are encrypted. An administrator, or anyone who takes the
  disk to a machine where they are one, reads every conversation.

**The sentence the UI shows** is the keeper's own, never a reassurance written
around it: "Windows DPAPI, tied to this user account on this machine."

## Forgetting, and leaving

**There is no passphrase, so there is nothing to forget.** The Windows password
is Windows' problem, with the one consequence noted above: a self-service
password change re-wraps DPAPI blobs and costs nothing, an administrator reset
does not and seals the vault. A sealed vault is an error naming the reason, and
never a fresh key, because a second key would open the vault, report nothing,
and orphan every secret in it.

**Uninstall leaves the profile alone.** Removing the program removes the
program. Chats, projects and secrets stay where they are, because an uninstaller
that deletes a year of conversations is a data-loss bug wearing a checkbox.
Deleting the data is a separate, explicit action inside Demido, it names what it
is about to destroy, and it is the only thing in the app that shreds the vault.
Deleting a Windows user takes their Demido profile with it, which is the
operating system's behaviour and the correct one.
