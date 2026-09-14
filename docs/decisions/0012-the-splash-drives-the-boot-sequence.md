# 0012. The splash drives the boot sequence, and a watchdog outlives it

Status: accepted
Decided: [#47](https://github.com/elpideus/demido-studio/issues/47)

Brief B41: "There should be a splash small window"

## Decision

**The startup stages run when the splash asks, not when the process starts.**
`src-tauri/src/boot.rs` owns the stages and reports each state change on
`boot://progress`; the splash asks for the stage list, draws one tick per stage,
subscribes, and only then calls `boot_begin`.

**A watchdog starts the same sequence if the splash never speaks.** After
`SPLASH_SPEAKS_WITHIN` the sequence runs with nobody listening, and the desk
opens on exactly the path it always does.

## Consequences

Easy: every stage is reported to a window that is already listening, so what a
person sees does not depend on how fast WebView2 booted that morning. A sequence
begun at `setup` would emit its first stages into nothing.

Easy: a stage that fails is one rose tick and the next stage starting
(`design/splash.md`). `run` has no failure path at all, so the rule that startup
never blocks is a shape rather than a promise: there is no way to write the
branch that would hold the desk shut.

Easy: the splash is vanilla and paints on the first frame, because the only
thing it waits for is an event it asked for itself.

Hard: two callers can start the sequence, so it carries a guard. A boot that ran
twice would open the desk twice and close a splash that is already gone.

Hard: the stage list is in Rust and the tick count is in the window, so the two
have to agree. They do because the window is told the list rather than given a
number: `design/splash.md` says the shell owns the count, and `boot_stages` is
that ownership expressed as a call.

Foreclosed: a splash that closes itself on a timer. The window would be claiming
that startup had finished, which is a claim it has no way to check, and it is
how a splash comes to vanish while a subsystem is still coming up.
