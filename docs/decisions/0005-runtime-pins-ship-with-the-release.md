# 0005. Runtime pins ship with the release, and nothing fetches without a press

Status: accepted
Decided: [#30](https://github.com/elpideus/demido-studio/issues/30)

## Decision

The runtime manifest ships inside the Demido build. A pin moves because a new
Demido shipped with a different one, verified on real hardware first. Demido
never queries upstream for versions, on launch or otherwise.

A new pin is an **Update** action on its Runtimes row at its stated download
size, per row, never automatic and never bundled into an Update all. The pin on
disk keeps working until the button is pressed, and an available update shows as
one dot on the settings navbar icon.

A release runs against the pin it ships with and against its predecessor. The
manifest names the current pin and exactly one predecessor, which is both the
rollback horizon and the compatibility window.

A linked runtime runs the same declared verification command at the moment it is
pointed at, and a row that fails it stays absent with a reason rather than
becoming linked.

## Consequences

Easy: an offline, auditable answer to "where did this version come from". Every
pin on a user's disk was run by somebody before it was recommended, which an
upstream release feed cannot promise about anything it returns.

Easy: the size promise of `setup.md` section 4 holds for the life of the install
rather than only for the first hour of it. Nothing spends 515.5 MiB without being
asked.

Hard: an upstream fix cannot reach a user who has not updated Demido. The escape
is `setup.md` section 7's **point at one I already have**, which is supported and
manual and hands version management to the user for that row.

Hard: host code may not depend on a pin its predecessor cannot satisfy. A change
that needs the newer runtime waits for the release after next, or ships the
compatibility.

Foreclosed: a background updater for runtimes, and any rollback further than one
pin. Both were traded for a window small enough that two rules can share it
without drifting.
