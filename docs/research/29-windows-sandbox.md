# 29. The sandbox on Windows: a user of Demido's own, researched and tried

Written 2026-11-23, directly. The owner chose it on 2026-11-22 (DECISIONS T26): a program a model runs on
Windows runs as a user of Demido's own, made once at setup with administrator rights; the session's folder is
shared with that user; a firewall rule for that user shuts the network. T26 left two things to find out before
building: how to start a program as that user without its password lying in the clear, and how to take the
folder's entry off again. This note answers both, and finds what T26 did not say: the user alone does not
keep a program from changing things outside the folder, and the firewall does not shut loopback.

How sure each line is: **Tried** (on the checks' Windows machine, Windows Server 2025, as an administrator,
with `core/dev/windows-sandbox-trial.ps1` and `windows-restricted-trial.ps1`, five runs on 23 November 2026), **Read** (Microsoft's documentation, or the code
of OpenAI's Codex, which built the same thing: `codex-rs/windows-sandbox-rs`, Apache-2.0, read on 23 November
2026).

## How Codex does it (Read)

Codex's "elevated" Windows sandbox is this design, built: two local users of its own, one with the network
shut and one without, made by a setup program run once as an administrator.

- **The passwords** are long and random, kept encrypted with DPAPI (`CryptProtectData`) in a file of its
  own, and given to Windows only when a program is started (`dpapi.rs`, `identity.rs`). It uses the
  machine's scope, because a Windows service of its own must read them too.
- **A program is started** with `CreateProcessWithLogonW` (`runner_client.rs`): the user's name and password,
  through Windows's Secondary Logon service; no special right is needed for it. What it starts is a small
  runner of Codex's own, which then starts the command.
- **The network** is shut by Windows Firewall rules that name the user (`setup_provisioning/firewall.rs`):
  a block rule, every protocol, out and in, whose `LocalUserAuthorizedList` is `O:LSD:(A;;CC;;;<the
  user's SID>)`. Before adding them it asks the firewall whether rules made on this computer take effect at
  all (`LocalPolicyModifyState`): a company's policy can make them count for nothing, and then it refuses.
- **Writes are kept to the work folder** by more than the user: the runner starts the command with a
  *write-restricted* token, made from its own, which carries a SID made up for that folder. Windows then asks
  for that SID as well whenever something is to be changed, and only the folder grants it.

## Starting a program as that user (Read; Tried below)

`CreateProcessWithLogonW` takes the user, the password and the command, and needs no right a person's own
account lacks (Microsoft's documentation). The password must be at hand as text for that call and no longer:

- made once, at setup: 24 random bytes, written as text, never shown;
- kept with DPAPI in the **person's own** scope: only a program running as the person can read it back, not
  the sandbox user, not another person on the computer. (Codex needs the machine's scope; Demido does not,
  because it has no service: it starts programs as the person.) The vault (U1) already keeps keys in Windows
  Credential Manager, which is DPAPI in the person's scope; the password can be one more entry there.
- one catch: if the person is not an administrator, setup runs as whoever types an administrator's password
  into Windows's prompt, and DPAPI there is that administrator's. So setup must hand the password back to
  the person's own Demido, which keeps it: the elevated step writes it, protected in the machine's scope,
  where only the person and the administrators can read it, and the person's Demido takes it into its own
  scope and deletes the file.

## Tried on Windows: what the user keeps a program from

`core/dev/windows-sandbox-trial.ps1`, on the checks' Windows Server 2025, as an administrator, the firewall on
in every profile. A user `DemidoTrial` made with a random password; commands started as it with
`Start-Process -Credential`, which is `CreateProcessWithLogonW`.

| Question | What Windows did |
|---|---|
| A user of its own, with a password nobody sees | made; DPAPI kept the password (262 bytes) and gave it back |
| A program started as that user | yes: `whoami` said `demidotrial` |
| Started so by a person **without** administrator rights | yes, it ran as `demidotrial`; but PowerShell then could not wait for it by its number ("Access is denied"): a program of another user cannot be opened by a person. Demido must keep the handle `CreateProcessWithLogonW` gives back, and do everything through it |
| It reads a file in the person's profile (`C:\Users\runneradmin`) | **no**: "Access is denied" |
| It reads a folder the person made at `C:\` | **yes**, and **changes it**: a folder made at `C:\` gets `Authenticated Users: Modify` from Windows's own defaults |
| It reads a folder the person made at `D:\` (a second disk) | **yes**; it cannot change a file there, but `Users` may create new files and folders in it |
| Shared (one entry for the user), it writes in the session's folder | yes |
| The folder's entry taken off, it gives itself access back to a file it made | **yes**: it owns that file, and an owner may always change who may use it (`icacls … /grant` as the owner) |
| Once the person is made the owner of everything in the folder again | it cannot |
| A block rule for its user, every protocol, any address: the internet | shut (`curl` could not connect) while the person's own connection still worked |
| The same rule: a server on this computer, on `127.0.0.1` | **reached**: the rule did not stop loopback |
| A name looked up | **yes**: lookups go through Windows's own DNS service, which is not that user |

Taking the folder's entry off, tried again in a folder only the person may use (no entry inherited from
`C:\`; `core/dev/windows-restricted-trial.ps1`): with the entry off, the sandbox user could neither write nor
read the file it had made there. As that file's owner it could not open it again either, not with `icacls`
and not by calling Windows itself (`CreateFileW` asking only for what an owner may always do, then
`SetSecurityInfo`: "Access is denied"). Where it could list the folder (the first trial's), it could. So the
owner's way back depends on the folder around it; Demido closes it either way, by making the person the
owner of everything in the folder again when the session gives it up. That worked in both trials.

## Loopback (Tried)

Windows Firewall's rules did not stop that user reaching a server on `127.0.0.1`: not a block rule for any
address, and not rules that name `127.0.0.0/8` for TCP and for UDP (and `::/127`; PowerShell refuses `::1`).
Every other address was shut. So a program inside reaches everything this computer serves on loopback. Of
Demido's own: its server and the model runtime both ask for a key that lives in the person's own folder,
which that user cannot read; but whatever else the person runs there (a database, a development server)
answers it as it answers the person. The way to shut loopback too is a filter of the Windows Filtering
Platform itself, which the firewall is built on and which can name a user (`FWPM_CONDITION_ALE_USER_ID`);
not tried.

## Names looked up (Tried)

With the network shut, a name can still be looked up: lookups go through Windows's own DNS service, which
runs as Windows, not as that user. A program inside can say a little to the outside that way (the name it
asks about), which is how data is carried out of such sandboxes. Linux's sandbox has no network at all, so
no lookup either; macOS's Seatbelt profile does not let it reach the system's lookup service (`mDNSResponder` is
not among the services it may ask). Shutting it on Windows
would take a filter on that service's traffic for names asked by that user, which Windows does not offer;
the honest answer is to say so.

## Writes kept to the session's folder (Tried)

The user alone does not do it: outside the profiles, Windows's defaults let every user change folders made at
`C:\` and create files on other disks. What does is Codex's second wall. A small program running as the
sandbox user (here a PowerShell runner; in Demido it would be a hidden command of `demido`, as the voice
engine is) makes a copy of its own token, *write-restricted*, carrying three restricting SIDs: one made up for
the session, its logon SID, and Everyone. Then, whenever something is to be changed, Windows also asks
whether one of those three is granted it. The session's folder grants the session's SID; almost nothing else
does. Two things it needs or it does not start at all (STATUS_DLL_INIT_FAILED, found the hard way, three
tries): the logon SID among the restricting SIDs, and a default DACL on the token that grants the logon SID
what the new program makes for itself. A made-up SID cannot be granted with `icacls` ("No mapping between
account names and security IDs"); it can through Windows's own calls.

| Question | What Windows did |
|---|---|
| The runner starts a command with the write-restricted token | yes, as `demidotrial2` |
| It writes in the session's folder (granted to the user and to the session's SID) | yes |
| It writes in a folder at `C:\` that every user may change | **no**: "Access is denied" |
| It writes in a folder shared with the same user but not the session's SID (another session's) | **no** |
| It reads that other folder | yes: reads are not restricted |
| Its own temporary folder, granted to the session's SID, set as `TEMP` | works |
| It writes in its own user's profile, or changes its own registry | **no** (so `TEMP` must be a folder of the session's) |

## What it keeps a program from, and what not

Kept from it: everything in the person's profile (their documents, keys, browser, mail, `AppData`),
anything another person on the computer keeps in theirs, changing anything outside the session's folder and
its own temporary folder, and the network (with the first user). Not kept from it, and to be said so where
the person chooses the level:

1. **Reading what Windows lets every user read.** The system, and, by Windows's own defaults, folders made at
   `C:\` and everything on other disks. A project kept in the person's profile (`C:\Users\…`, where Visual
   Studio and most tools put them) is not readable; one at `C:\Projects` or on `D:\` is, and so are the
   person's other projects beside it. Linux's sandbox and macOS's profile shut those too; here it would take
   an entry refusing the sandbox users on each such folder, which Demido could offer for the folders it
   knows (IDEAS).
2. **Another session's folder, while both are in use at the same moment**: readable, not changeable.
3. **Loopback**, and **names looked up** (above).

## Proposed: the build

1. **Setup, once, with Windows's own prompt for an administrator** (from the Setup screen; the person can
   refuse, and then the levels ask, as now): two users of Demido's own, one with the network shut
   (`DemidoSandbox`) and one without (`DemidoSandboxNet`), each with a random password, hidden from the
   sign-in screen; block rules for the first, out and in, every protocol; refused, and said so, if the
   computer's policy makes local firewall rules count for nothing (`LocalPolicyModifyState`). The passwords
   go to the person's own Demido (DPAPI in the person's scope, or the vault), never shown.
2. **A command**: the core starts `demido sandbox-runner` as the user, with `CreateProcessWithLogonW`,
   started waiting and put in the command's job first (T16), and keeps the handle it is given (a program of
   another user cannot be opened by its number). The runner makes the write-restricted token and starts
   `cmd /D /S /C` with it, in the folder, with a clean environment and `TEMP` in a folder of the command's own.
   Output comes back through pipes, as now.
3. **The folder**: a SID made up once for the session; while the session uses the folder, the folder has two
   entries (the user, to read and change; the session's SID, for the write check), set through Windows's own
   calls; when it stops (the session ends, its folder changes, Demido closes), both entries come off and the
   person is made the owner of everything in it again.
4. **Tests on the checks' Windows machine**, which is an administrator: the trial's questions, as tests.
5. **Not known yet**: how long the entries take on a large folder (`node_modules`), since Windows writes them
   on every file inside; whether the job holds a program started through Windows's Secondary Logon service,
   which puts it in a job of its own (nested jobs exist since Windows 8); antivirus programs that watch for
   new users. To be found while building.

Codex's code was read to learn how; nothing is copied from it. If any part is adapted rather than written
anew, its Apache-2.0 notice goes with it.
