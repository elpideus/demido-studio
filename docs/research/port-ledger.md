# Port ledger: what in v2 is actually trustworthy

Answers github.com/elpideus/demido-studio issue #2. Audits every crate under
`demido-studio-second-version\src-tauri\crates\` (25 crates) for evidence
level, coupling, Windows-boundness, and a port verdict, so the port quarantine
rule in the wayfinder map has something to quarantine against.

Method: three parallel audits, one per crate group, each reading the crate's
own `Cargo.toml`, `src/`, `tests/`, and `AGENTS.md`, cross-referenced against
`docs/roadmap.md`'s "Known gaps" section, `docs/decisions/`, and
`docs/quirks.md` in `demido-studio-second-version`. All paths below are
relative to `demido-studio-second-version\src-tauri\crates\<crate>\` unless
stated otherwise.

**Evidence-level scale**, poorest to strongest:

- `never-run` — no tests, or the roadmap explicitly says the path has never
  been exercised.
- `unit-tested` — inline `#[test]` fns only, no `tests/` integration dir.
- `automated-test` — a `tests/` suite hitting something real (a socket, a
  filesystem, a real third-party process, a real network endpoint), but not a
  live model.
- `live-model` — a scenario in the `a_real_model.rs`-style harness, driven
  against an actual llama.cpp server and a real GGUF (or, for `demido-nexus`,
  a real hosted completion with no account).

---

## Verdict table

| Crate | Evidence | Coupling | Windows-bound | Verdict | Reasoning |
|---|---|---|---|---|---|
| **demido-core** | unit-tested (17 inline `#[test]` fns across `atomic.rs`, `paths.rs`, `program.rs`, `tree.rs`; no `tests/` dir) | Foundation: depended on by all 24 other crates; depends on nothing | Partial, and already forked. `Cargo.toml` pulls `windows` (`Win32_System_JobObjects`) only under `cfg(windows)`; `src/tree.rs:49,70` gates the job-object child-tree killer behind `#[cfg(windows)]` **with an explicit `#[cfg(not(windows))]` no-op already written** (`tree.rs:54-58`); `src/paths.rs:174` reads `%APPDATA%`/`%LOCALAPPDATA%` under `cfg(windows)` | PORT-WITH-REVIEW | Already cross-platform-branched, but has zero integration evidence beyond inline unit tests, and the Windows arm (job objects, `%APPDATA%`) needs a real non-Windows counterpart proven, not assumed. |
| **demido-trace** | automated-test — the shared session-log contract suite runs unignored against both backends (`sqlite.rs:534-536`, `jsonl.rs:263`) | Depends on demido-core; depended on by demido-chat and demido-navigator | None found — no `cfg(windows)`/`cfg(target_os)` anywhere in the crate | PORT-AS-IS | Fully portable persistence layer with a real dual-backend contract suite and no platform-specific code. |
| **demido-chat** | **live-model** — strongest evidence in the audit. `tests/a_real_model.rs` has 29 scenarios (e.g. `a_managed_server_carries_a_turn_with_a_tool_in_it`, `a_model_asked_to_think_first_still_answers_and_still_calls_a_tool`) plus `tests/a_sampler_set.rs` and `tests/a_caveman_measurement.rs`, all driven against a real llama.cpp/GGUF, gated on `DEMIDO_GGUF` | Depends on demido-core, demido-inference, demido-tools, demido-trace (+ demido-market, demido-prompts, demido-settings; dev-deps demido-graph, demido-skills, demido-mcp). Top-of-stack orchestrator — travels with demido-trace, demido-tools, demido-inference as one cluster | None found in the crate's own files; Windows behavior it exercises lives in its dependencies | PORT-AS-IS | Best-validated crate in the whole codebase (31 live scenarios against a real model), no Windows cfg of its own. |
| **demido-inference** | automated-test — `tests/over_http.rs` runs a real (unignored) TCP HTTP server and drives `OpenAiBackend::generate()` over an actual socket, SSE decode off the wire | Depends on demido-core; depended on by demido-chat, demido-tools, demido-nexus, demido-setup | None found — process supervision spawns via demido-core's job-object-wrapped child, but the Windows cfg lives there, not here | PORT-AS-IS | Trait/contract design exercised over a real socket, no direct platform coupling. Carries forward the `--n-gpu-layers auto/all/<count>` and silent-arg-rejection landmines from `docs/quirks.md` regardless of platform. |
| **demido-tools** | automated-test — `tests/a_stranger.rs` (ignored, needs network/Node/npm) drives two real third-party MCP servers end to end (`npx -y warframe-market-mcp`, `uvx mcp-server-time`) through manifest, disclosure, approval, start, catalogue, grammar, call, stop | Depends on demido-core, demido-inference (+ demido-graph, demido-market, demido-mcp, demido-skills, demido-web); depended on by demido-chat | Yes, in the shell only. `src/command.rs:176` `#[cfg(windows)]` builds a `cmd /d /s /c "<line>"` invocation working around the `cmd.exe` quoting bug in `docs/quirks.md`, with a `#[cfg(not(windows))] sh -c` counterpart already present; `src/workspace.rs:287/289` splits a symlink test by `cfg(unix)`/`cfg(windows)` | PORT-WITH-REVIEW | Core confinement logic (`workspace.rs`) is already cross-platform; `command.rs`'s shell handling is built entirely around the Windows `cmd.exe` quoting quirk and its Unix branch is comparatively unexercised. |
| **demido-hardware** | automated-test, narrow — `detection_never_panics_on_this_machine` and `cuda_is_never_offered_without_a_driver_that_runs_it` call the real `Machine::detect()` unignored on every `cargo test`, but roadmap Known gaps: "Only one driver has ever been asked... every live run has been one machine reporting CUDA 13.2" | Depends on demido-core; depended on by demido-catalog, demido-vram, demido-setup | Heavily, and admittedly so. `Cargo.toml` pulls `windows` (`Win32_Graphics_Dxgi`) only under `cfg(windows)`; `src/probe.rs` (GPU enumeration) is Windows-only by design — its own `AGENTS.md`: "Other platforms report an empty list and a note. Cross-platform detection lands with cross-platform support at 1.0." `src/cuda.rs:99` (`nvcuda.dll`) does have a Linux counterpart (`libcuda.so.1`, `:101`) — only adapter enumeration is Windows-only, not CUDA-driver querying | PORT-WITH-REVIEW (adapter probe closer to REWRITE) | CUDA-driver detection is already cross-platform; GPU enumeration is Windows-only by explicit design and needs a real non-DXGI implementation to port, not a recompile. |
| **demido-vram** | automated-test on the parser only — `tests/a_real_gguf.rs` (ignored) reads real GGUF headers off disk, no inference. Roadmap itself (Settled decisions): "*Built and dormant*: `demido-vram`'s `Ledger` exists, is tested, and has no consumer... Delete the crate rather than let it rot if that day does not come." `COMPUTE_OVERHEAD` is "known to be in the right region and still not measured directly" | Depends on demido-hardware; depended on **only** by demido-setup — no runtime consumer in chat or inference | None found | PORT-WITH-REVIEW (candidate DROP) | Fully portable but explicitly dormant dead weight by the codebase's own admission; port only if v3 actually builds the tiered-VRAM scheduler this was written for. |
| **demido-catalog** | automated-test, strong — `tests/against_upstream.rs` (ignored) hits real GitHub releases and installs a real Vulkan build; its own `AGENTS.md` says every one of its fixes was found this way, "and every one of them would have shipped broken" | Depends on demido-core, demido-hardware, demido-download; nothing among the audited crates depends on it (demido-inference takes only a resolved binary path) | Minimal — `src/install.rs:268` `#[cfg(unix)]` restores the executable bit lost by zip unpacking ("Windows does not have one"); designed cross-platform from the start, extension-based archive selection so "fetching a Linux build from Windows works" | PORT-AS-IS | Designed cross-platform from the start, with real network evidence that caught upstream-naming bugs unit tests missed. |
| **demido-download** | automated-test — `tests/over_http.rs` (real TCP socket + real filesystem against a deliberately misbehaving fake server: truncation, ignored `Range`, `416`, silence); `tests/against_a_real_cdn.rs` (ignored) downloads a real file from Hugging Face | Depends on demido-core only; consumed by demido-catalog and demido-models | None found | PORT-AS-IS | Portable, single-purpose; its own `AGENTS.md` notes there is deliberately no second implementation to reconcile. Clusters tightly with demido-models. |
| **demido-models** | automated-test — `tests/against_hugging_face.rs` (ignored) fetches a real 214 MB GGUF end to end into the library | Depends on demido-core, demido-download; only consumed by the Tauri layer directly | None found | PORT-AS-IS | Roadmap cites this crate as proof that search/download/choose works end to end against a real file. Ships with the caveat that the *wizard's* use of it has never been driven (see demido-setup). |
| **demido-settings** | unit-tested — `src/store.rs` tests atomic write/move-aside against real temp dirs; no `tests/` integration dir | Depends on demido-core only; consumed by demido-chat, demido-navigator, demido-prompts, demido-characters (by design pattern) — widest fan-out of any non-core crate | None found | PORT-AS-IS | Foundational per decision 0008 (settings resolve with provenance); heavily depended on, so it should move first and cleanly. |
| **demido-setup** | unit-tested — `src/plan.rs`/`src/answers.rs`; its own `AGENTS.md` states plainly "no GPU, no server and no installed backend are involved" in its tests | Depends on demido-core, demido-hardware, demido-inference, demido-vram — heaviest pull of any crate in this group; consumed only by the Tauri layer | None found in the crate itself; inherited from demido-hardware/demido-vram | PORT-WITH-REVIEW | Roadmap Known gaps: "No model has been downloaded through the wizard... Search, download, choose and verify through the window has not been done once." The plan logic is proven in isolation; the end-to-end wizard path is not. |
| **demido-prompts** | unit-tested — `src/catalog.rs` (e.g. `a_default_uses_exactly_the_placeholders_it_declares`); no `tests/` dir | Depends on demido-core; consumed by demido-chat | None found | PORT-AS-IS | Small, self-contained, mirrors demido-settings' "declare once / resolve once" seam by explicit cross-reference in its own AGENTS.md. |
| **demido-characters** | unit-tested — `src/card.rs` (PNG/card round-trip incl. CRC) and `src/characters.rs`; no `tests/` dir | Depends on demido-core; soft-couples to demido-settings (character id keys a settings layer) | None found | PORT-WITH-REVIEW | Roadmap Known gaps: "No character has been imported from a real card... import one card from each of two sources before calling it settled" — real-world SillyTavern cards are an untested edge the fixtures don't cover. |
| **demido-skills** | unit-tested with a real-filesystem check — `every_skill_this_repository_ships_installs` (`src/skills.rs`) scans the repo's actual bundled `skills/` directory end to end; no live-model evidence | Depends on demido-core; consumed by demido-chat, demido-navigator, demido-tools. Clusters with demido-mcp: "a skill's `engine/` is an MCP stdio server" (roadmap.md) | None found | PORT-WITH-REVIEW | Roadmap Known gaps: "The bundled skills directory has never been resolved in a packaged build. Right in dev, where `resource_dir` is a target directory. Only an installer settles the rest." Everything else is well covered. |
| **demido-mcp** | automated-test — `tests/a_real_process.rs` (own `main`, re-execs itself as a real child process over real pipes: drive one, supervise two, restart, lose a tool) | Depends on demido-core (`demido_core::child`, where the Windows-only logic actually lives); consumed by demido-chat, demido-tools | Indirect but load-bearing. No `cfg(windows)` in the crate itself, but its own `AGENTS.md` documents behavior meaningless without demido-core's Windows branch: "On Windows the child is very often not the server... `tree` puts the child in a job object with `KILL_ON_JOB_CLOSE`... On Unix `npx` execs node rather than forking... this is a no-op." Also relies on demido-core's `PATHEXT` search | PORT-WITH-REVIEW | Portable and well-tested by a genuine subprocess integration test, but Windows correctness is entirely delegated to demido-core — port in lockstep with `demido-core::tree`/`program`, not alone. Carries forward every `docs/quirks.md` Windows-process landmine: `PATHEXT`, `.cmd`-shim killing, `cmd.exe` quote parsing. |
| **demido-web** | automated-test (real network, no live model) — `tests/a_real_search.rs`, `tests/a_real_browser.rs`, `tests/a_real_page.rs`, `tests/a_real_searxng.rs`, all `#[ignore]`d live-network contract tests against real DuckDuckGo/Wiby/SearXNG/agent-browser | Depends on demido-core, demido-python (fetches uv for SearXNG); consumed by demido-tools. Hub cluster with demido-python and demido-graph | Portable in code — no `cfg(windows)` in `src/`, but `src/browser.rs:86` documents the Windows-only `PATHEXT` quirk it works around, and SearXNG "cannot be checked out on Windows at all" per `docs/quirks.md` | PORT-WITH-REVIEW | Portable design with real network-contract tests, but carries Windows-specific process-spawn workarounds and a SearXNG bootstrap history that needs re-verification on the target OS. Carries forward every keyless-search-engine and DuckDuckGo-202 landmine from `docs/quirks.md`. |
| **demido-graph** | automated-test — `tests/a_real_extraction.rs`, `tests/a_real_graph.rs` (both need `graphify` + `DEMIDO_GRAPH`), no live model. `placing.rs` is explicitly called "the riskiest code here" by its own `AGENTS.md`, guessing an undocumented third-party id format | Depends on demido-core, demido-python (uvx fetch of graphify); consumed by demido-chat, demido-tools | Portable — no `cfg(windows)` hits; spawns `uvx`/`graphify` cross-platform via tokio process | PORT-AS-IS | No Windows-specific code; the real risk is the `graphify` id-slugging contract, which travels with the crate regardless of platform. |
| **demido-navigator** | unit-tested only — no `tests/` dir, inline ranking/source tests against a `JsonlStore` in a temp dir. Roadmap Known gaps: "The Navigator has never been opened" | Depends on demido-settings, demido-skills, demido-trace; nothing among the audited crates depends on it | Fully portable — pure fuzzy-match/indexing logic (`nucleo-matcher`), no OS-specific deps | PORT-AS-IS | Small, portable, well-unit-tested pure logic; the open gap is a UX validation question, not a portability one. |
| **demido-vault** | Mixed. Vault-store logic is unit-tested against a reversing fake keeper. The DPAPI keeper itself is covered by `keeper::contract` (`src/keeper.rs:183-245`, e.g. `what_was_wrapped_comes_back`, `the_same_secret_wraps_differently_every_time`), which runs live against real Windows DPAPI on the audit machine — closer to automated-test for that one path. Roadmap Known gaps: "The vault has held one kind of thing and only in a test" — nobody has typed a real credential through Secrets in the UI | Depends on chacha20poly1305, rusqlite; not a Cargo dependency of demido-nexus/demido-providers, but both reference its `<domain>/<id>` secret-naming convention conceptually | Hard Windows-bound by design. `Cargo.toml` pulls `windows` (`Win32_Security_Cryptography`) for DPAPI only under `cfg(windows)`; `src/keeper.rs:61,77` gate the DPAPI keeper behind `#[cfg(windows)]`. Roadmap: "There is no keeper on Linux or macOS, so a build there has no vault and says so." Decision 0025: DPAPI keys are bound to the OS user, by design, with no fallback | REWRITE | The `Keeper` trait plus its contract suite (decision 0025) is portable and worth keeping as the seam; the only working implementation is Windows DPAPI, so a v3 targeting other platforms needs a macOS Keychain / Linux secret-service `Keeper` written from scratch — decision 0025 names these as future work that does not yet exist. |
| **demido-nexus** | **live-model** — `tests/a_real_keyless.rs` sends a real completion request with no account (decision 0034); roadmap confirms this closed on 2026-08-31. `tests/a_real_provider.rs` (ignored) checks each catalogue host's `/models` live | Depends on demido-inference (produces the same `Connection` type the local llama.cpp route does); consumed by the Tauri layer. Deliberately not merged with demido-providers (different vault prefix, per its own AGENTS.md "Why this is not demido-nexus") | Fully portable — no `cfg(windows)`, pure HTTP via reqwest | PORT-AS-IS | Strongest evidence tier besides demido-chat: a genuine live-model run with no account, and zero platform coupling. Carries forward the "keyless is mostly gone" landmine from `docs/quirks.md`/decision 0026 — the catalogue is real but will rot and needs the same live check re-run in v3. |
| **demido-providers** | never-run for live behavior — no `tests/` dir, only inline unit tests (id derivation, clash numbering); hitting a real endpoint is deliberately demido-inference's job, not this crate's, per its own AGENTS.md | Depends on demido-core; nothing else depends on it | Fully portable — plain JSON file read/write, no OS-specific code | PORT-AS-IS | Trivial, well-reasoned data-management crate (id stability, atomic writes); low risk despite thin coverage because the logic itself is small and fully unit-tested. |
| **demido-python** | automated-test for the fetch mechanics — `tests/a_real_uv.rs` (ignored) fetches the pinned uv archive and runs it — but roadmap Known gaps: "A machine with no uv has never fetched one... the whole path on a machine with neither" has not happened | Depends on nothing demido-internal; depended on by demido-web and demido-graph — the shared bootstrap hub | Portable across all three OSes **by design**: `src/release.rs` has explicit `cfg!(target_os = "windows"/"linux"/"macos")` branches for the platform/arch table, and `install::flatten` handles per-platform archive-layout differences | PORT-AS-IS | Explicitly cross-platform already (three-OS table in code); the only untested path is a cold machine, which is an environment concern, not a portability one — and it is the shared dependency demido-web and demido-graph both need ported alongside them. |
| **demido-market** | live-model-adjacent / automated-test — `tests/a_real_market.rs` is gated on real TradingView session cookies (`DEMIDO_TV_SESSION`/`DEMIDO_TV_SIGNATURE`), the same two vars that gate the live-model scenario in `demido-chat/tests/a_real_model.rs`. `session.rs`'s protocol logic is separately unit-tested against in-memory duplexes | Depends on demido-core (`demido_core::child`, moved here from demido-mcp when this crate "turned out to be a verbatim copy," per its own AGENTS.md); consumed by demido-chat, demido-tools | Windows-flavored but not hard-gated: no `cfg(windows)` in its own `src/`, but it inherits demido-core's Windows job-object process handling; its sidecar (`sidecars/market/index.mjs`) is Node/JS and portable | PORT-WITH-REVIEW | Logic and protocol are portable, but child-process lifecycle leans on demido-core's Windows-specific handling, which should be reviewed alongside it. **Note on the ticket's premise**: this crate does not use MetaTrader 5 at all — it is a reverse-engineered TradingView websocket client via a Node sidecar (`@mathieuc/tradingview`); MT5's documented Windows-only Python API (`docs/quirks.md`) applies to a market-data path this crate never took. |
| **demido-keys** | unit-tested only — inline tests cover `binding` (chord refusal, bad modifier, bad name) and `keymap` (diff-only file, revert, clash naming). Roadmap Known gaps: "No key has ever been rebound" in a real window | Depends on nothing demido-internal; nothing depends on it — fully standalone | Fully portable — pure schema/matching logic; deliberately does not do the actual key-matching itself ("the window matches"), so no platform keyboard code lives here | PORT-AS-IS | Smallest, most self-contained crate in the workspace; pure data/logic, well-covered, zero OS coupling. |

---

## Crate clusters

Assembled from what each crate's own `Cargo.toml`/`AGENTS.md`/tests actually
couple it to, not from guesswork.

1. **The agent-loop cluster: demido-chat, demido-trace, demido-tools,
   demido-inference.** `demido-chat` is the only crate with true live-model
   evidence, and it earns that status by wiring the other three together
   (session log, tool registry, inference backend) exactly as the app does.
   None of the four stand alone as a port unit; this cluster additionally
   reaches into demido-mcp, demido-skills, demido-graph, demido-web,
   demido-market, demido-prompts, and demido-settings.

2. **The hardware/resource cluster: demido-hardware, demido-vram,
   demido-catalog.** `docs/quirks.md`: "Detection reports, it never decides...
   demido-hardware says what CUDA the driver runs, demido-catalog turns that
   into a choice of archive." `demido-vram` is this cluster's dormant leaf —
   built, tested, and consumed only by demido-setup.

3. **The Python-bootstrap cluster: demido-python (hub), demido-web,
   demido-graph.** Both demido-web and demido-graph depend on demido-python
   directly to fetch `uv`/`uvx` for SearXNG and graphify respectively.
   Porting either requires porting demido-python alongside it; low risk
   because demido-python is already three-OS aware in code.

4. **The skills/MCP cluster: demido-skills, demido-mcp, demido-tools.** A
   skill's `engine/` folder is itself an MCP stdio server (`docs/roadmap.md`),
   and `demido-tools`' `tests/a_stranger.rs` is what actually exercises the
   pairing end to end against a real third-party server. Port decisions on
   demido-skills or demido-mcp should be made jointly with demido-tools.

5. **The settings-family cluster: demido-settings, demido-prompts,
   demido-characters.** Not a Cargo-dependency cluster but a documented design
   cluster: all three share "one definition, one resolution path, no loaded
   state, re-read every call" by explicit cross-reference in each other's
   `AGENTS.md`, and demido-chat is what joins them at runtime.

6. **The download cluster: demido-download, demido-models.**
   demido-download's own `AGENTS.md`: "This exists because Demido bundles
   nothing... the first useful thing the app does is fetch a few hundred
   megabytes." demido-models is its only in-workspace consumer besides
   demido-catalog.

7. **The core-dependents' Windows split.** `demido-core` is the shared base
   for all 25 crates, and it is the one crate whose Windows-specific piece
   (job-object child trees, `%APPDATA%`/`%LOCALAPPDATA%`) already carries a
   working `#[cfg(not(windows))]` fallback in source. `demido-hardware`'s GPU
   probe (DXGI) has no such fallback and is the crate most in need of new,
   not ported, code for a cross-platform v3. `demido-market` shares
   demido-core's process-tree/job-object handling with demido-mcp (their code
   was, per demido-market's own `AGENTS.md`, "a verbatim copy" of each other
   before the shared logic moved into demido-core) — any review of one's
   Windows process handling should include the other plus demido-core.

8. **Deliberate non-clusters, worth preserving as-is.** demido-nexus and
   demido-providers explicitly reject merging (different vault prefixes,
   different jobs: one is a router over free hosted tiers, the other is a
   list of providers somebody typed in) — keep them as two crates in v3.

---

## Quirks carried forward

Every landmine below is still true of v2 and applies unchanged to whatever in
v3 replaces the crate named. Source: `demido-studio-second-version\docs\quirks.md`.

- **WebView2 has no isolated worlds.** Any script injected into a page shares
  that page's JS context, so a hostile page can shadow or spoof it — this is
  why a model drives Chromium over CDP (via `agent-browser`, behind
  demido-web/demido-tools) rather than the built-in webview, and why the
  E2E bridge needs `withGlobalTauri` and the `mcp-bridge:default` capability
  to reach the webview leg at all.
- **The 260-character path limit.** Deep `node_modules` plus a deep repo path
  still trips it in some tools; keep any v3 checkout shallow.
- **`PATHEXT` and `npx`.** `CreateProcess` does not read `PATHEXT`, so
  `Command::new("npx")` fails with "the system cannot find the file
  specified" even though the shell resolves it fine. `demido-mcp`'s
  `host::resolve` (via `demido-core`) does the `PATHEXT` search itself before
  spawning; anything else that spawns a user-named program on Windows has the
  same problem and, outside that path, none of the same handling.
- **Windows job objects for killing `.cmd` shims.** Resolving `npx` to
  `npx.cmd` means the process actually spawned is `cmd.exe`, and the `node`
  doing the work is its child; `Child::kill` reaps only `cmd.exe` and leaves
  `node` running, still holding the pipe. `demido-mcp`'s `tree` (via
  `demido-core::tree`) puts every child in a job object with
  `KILL_ON_JOB_CLOSE`, which is also the only thing that cleans up when
  Demido is killed rather than closed.
- **`cmd.exe` does not read `\"` as an escaped quote, and Rust writes one.**
  `Command::new("cmd").arg("/C").arg(line)` quotes the way a C runtime reads
  it back, which is not how `cmd` parses its command line — every quoted
  argument arrives with backslashes in the middle of it. `demido-tools`'
  `command::shell` uses `raw_arg` under `cmd /d /s /c "<line>"` instead.
- **`--n-gpu-layers` takes `auto`, `all`, or a count** — a sentinel meaning
  "all of them" is rejected by llama.cpp's argument parser, and the failure
  surfaces as a usage-message tail rather than anything about the model.
  `demido-inference`'s `Offload` mirrors all three spellings for this reason,
  and a rejected argument dies before the model loads, so it looks identical
  to a bad model file from the outside — the stderr tail is what tells them
  apart.
- **llama.cpp silently rejects unknown sampler fields.** A compatible server
  is entitled to reject a body carrying a field it has never heard of (M17);
  `Options` now carries each sampler as an `Option` so "off" and "zero" are
  distinguishable on the wire.
- **DPAPI keys are bound to the OS user.** Copying the profile to another
  machine or account will not decrypt the vault — that is intended
  (decision 0025), and it is the whole reason demido-vault is a REWRITE
  rather than a port for any non-Windows target.
- **The keyless inference and search landmines are still current as of
  2026-08-28**: Hack Club's endpoint answers 404, GitHub Models 410, GaiaNet
  doesn't resolve, Pollinations answers out of a cache and 402s on anything
  novel; DuckDuckGo answers 202 with a challenge page under load; SearXNG
  cannot be checked out on Windows at all (`utils/templates/etc/httpd/...` has
  a colon in the path, illegal on NTFS) and has no stable release channel, so
  it must be pinned to a commit tarball, not a tag.
- **MT5's Python API is Windows-only and requires the terminal running and
  logged in** — true of MetaTrader 5 in general, but not applicable to
  `demido-market`, which does not use MT5 (see the crate's row above).

---

## What this settles for the wayfinder map

- **Port as-is, no review needed**: demido-trace, demido-chat,
  demido-inference, demido-catalog, demido-download, demido-models,
  demido-settings, demido-prompts, demido-graph, demido-navigator,
  demido-nexus, demido-providers, demido-python, demido-keys.
- **Port with review** (portable design, an unproven path, or an inherited
  Windows dependency worth checking before trusting): demido-core,
  demido-tools, demido-hardware (adapter probe closer to a rewrite),
  demido-setup, demido-characters, demido-skills, demido-mcp, demido-web,
  demido-market, demido-vram (candidate drop if the tiered-VRAM scheduler
  stays unbuilt).
- **Rewrite**: demido-vault. The `Keeper` trait and its contract suite travel;
  the only implementation (DPAPI) does not, by design.
- **Drop**: none outright. demido-vram is the closest thing to a drop
  candidate — the roadmap itself says to delete it rather than let it rot if
  nothing ever consumes the `Ledger` — but it is fully portable and cheap to
  carry forward if v3 intends to build the scheduler it was written for.

Nothing here found a crate that is actively broken; the split is between what
has been proven against something real (a socket, a real model, a real
third-party process) and what has only ever proven itself to itself.
