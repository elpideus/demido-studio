# 0004. Demido fetches its own Chrome and prunes nothing else

Status: accepted
Decided: [#29](https://github.com/elpideus/demido-studio/issues/29)

## Decision

Demido fetches `chrome-win64.zip` at the pin its own manifest names, into the
profile's runtimes folder, and launches `agent-browser` with `--executable-path`
pointing at it. It never reads or writes `~/.agent-browser` or
`~/.cache/puppeteer`.

A managed runtime's superseded pin is deleted as soon as the new pin's declared
verification command exits zero. No predecessor is retained. A linked runtime,
one the user pointed at, is never written to, never deleted and never counted in
the disk total.

## Consequences

Easy: the manifest stops lying. `agent-browser install` takes no version
argument, so a pin routed through it was never honoured, and a size measured from
an archive Demido does not fetch was never a measurement. Both become true.

Easy: one guard. Deletion keys off the managed / linked / absent state rather
than off a path comparison, so there is a single place where "may Demido remove
this" is answered.

Hard: Demido no longer shares a Chrome the machine may already hold, and pays
427.9 MiB on disk for its own. `setup.md` section 6 absorbs most of it by not
fetching the row at all when a drivable browser exists.

Foreclosed: cleaning up after other tools. The 2.65 GiB of Chrome measured on the
rig stays where it is. Deleting a binary another program fetched into the user's
home is `profiles.md`'s own hazard with a program in place of a second user, and
a rollback that needs those bytes is a re-download rather than a loss.
