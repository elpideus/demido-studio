# 0006. Demido checks for its own releases, and one dot says so

Status: accepted
Decided: [#31](https://github.com/elpideus/demido-studio/issues/31)

## Decision

A Demido release is a signed `vX.Y.Z` tag on `main`, built on `windows-latest`
and published to GitHub as one NSIS installer plus a minisign-signed
`latest.json`. NSIS rather than MSI, because Tauri's NSIS target installs per
user and its MSI target does not, and per user is what a Demido profile being a
Windows profile requires.

Demido asks its own release endpoint, off the startup path and at most once a
day. The refusals in `runtimes.md` section 6 and `nexus.md` section 5 do not
transfer: a Demido release exists because somebody ran it on real hardware, which
is the evidence an upstream feed cannot supply, and GitHub is not a commons
anybody donates to.

A newer version reuses the notification vocabulary that already exists: one dot
on the settings navbar, resolving to a Demido row at the top of the Runtimes
page, exempt from that page's managed, linked and absent states. The download
happens on the press. First launch never checks, and one switch turns checking
off.

An update replaces application binaries. It never names a profile's runtimes
folder, vault, chats or session log, and uninstall leaves all of them.

The rules that are only meaningful at a tag move into `scripts/check-release.mjs`:
the predecessor pin, `verified_on` rot, nothing third-party bundled, and the tag
matching the version in the build files.

## Consequences

Easy: hard rule 3 gets a checker. It has been enforced by review for want of an
installer, and there is now an installer to inspect, so `AGENTS.md` reaches zero
review-enforced hard rules.

Easy: an upstream `llama.cpp` fix has a path to a user again. It travels inside a
Demido release, which is what section 6 said and could not deliver while nothing
told a user a release existed.

Hard: the minisign private key is unrecoverable in one direction. Lose it and
every installed copy stops accepting updates permanently, because a replacement
public key ships only inside a build the old key would have to sign for. The
escape is the manual install, which fails visibly and slowly, and it is the only
one.

Hard: the installer is unsigned for Authenticode, so every download raises
SmartScreen until a certificate exists, which waits on the same domain #17 waits
on.

Foreclosed: an About page with its own update surface, and any second way of
telling a user something about their own build. That closes the runtime-rot
warning as well: rot stays a release gate, because a warning fired before
anything has failed is a claim about a source Demido has not called.
