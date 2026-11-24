# 34. Tools other people write: the runtime, the interface, the asks, the signature

Written 2026-11-24, directly, re-reading where research note 08 (2026-11-19) has aged. DECISIONS C5 gave the
direction: one package format; installable tools are Rust compiled to WebAssembly components; MCP servers are
"External"; raw MCP and classic skills "Deprecated".

How sure each line is: **Read** (crates.io and the crates' own sources, 24 November 2026), **Tried** (built and run
here, the same day).

## The runtime (Read, Tried)

- **Wasmtime 51.0.0**, `Apache-2.0 WITH LLVM-exception`; its minimum Rust is 1.97 (Demido builds with 1.100).
  Note 08 had 50.0.3; a new major comes on the 20th of each month. The exception only adds permissions, and the
  licence check already allows it (R12).
- The **component model**, with `wasmtime::component::bindgen!` making the host's side of an interface from its
  WIT. A tool built for `wasm32-wasip2` with Rust's standard library imports a few WASI interfaces (the
  environment, the standard streams, the clock): **wasmtime-wasi 51 with the `p2` feature** gives them, from a
  `WasiCtx` built with nothing: no files opened, no environment, no arguments, no network. WASI is
  deny-by-default (note 08), so the host grants by not adding.
- **Limits**: epoch interruption (an engine-wide counter moved every 100 ms by a thread of Demido's; a store's
  deadline 300 ticks, so about 30 s), and a memory limiter (`StoreLimitsBuilder`: 256 MB, 10 instances).
- **Tried**: the sample tool (below) compiled to a 74 KB component; Wasmtime loaded it, its `run` read a file
  through the host, counted it and answered; a path that leads out of the folder was refused by the host; a
  tool whose manifest did not ask for the folder was not given it.

## The interface (Tried)

`tools/wit/tool.wit`, `package demido:tool@0.1.0`: a tool exports `run(args: string) -> result<string, string>`
(the model's arguments as JSON in, text for the model out), and imports four calls, `log`, `fetch(url)`,
`read-file(path)`, `write-file(path, text)`. That is the whole of what it can reach; the host answers each only as
far as the manifest asks. A guest is written with **wit-bindgen 0.57** (Apache-2.0 WITH LLVM-exception OR
Apache-2.0 OR MIT), `cargo build --target wasm32-wasip2`, as note 08 found Zed's extensions are.

## What a tool may ask for

Two things, in its manifest: **the folder** (`none`, `read`, `write`) and **the hosts it may read pages from**.
Nothing else exists to ask for: no programs, no other files, no sockets, no clock beyond WASI's. Each maps onto
what Demido's own tools reach (section T): reading the folder is `Reads`, writing it is `Changes` (the folder
saved first, so all of it is one Undo, as a program's is), reading pages is `Network`, through Demido's own
reading of pages (N: the public internet only, under Demido's name, `robots.txt` followed).

## How a package is signed and checked (Read, Tried)

- **minisign** (note 08): `minisign-verify` 0.3.0, MIT, no dependencies, verify-only, checks the BLAKE2b-prehashed
  Ed25519 signatures `minisign -S` makes by default (legacy unhashed ones are refused). A publisher signs with
  `minisign` or with Demido's `pack_tool` example (on the `minisign` crate 0.10, MIT, a development dependency).
- **What it proves**: that the holder of the key named in the package signed exactly these bytes. Not who holds
  the key. So the first install is "trust on first use", as SSH does: the person is shown the key's id (as minisign
  prints it) to compare with the one the publisher shows; every later version must be signed with the same key.
- **Sigstore** (keyless, tied to a CI identity) stays out for now: its Rust crates are pre-1.0, pull in many
  dependencies and need a network refresh of their trust root (note 08).

## The package

A `.tar.gz` (`<name>-<version>.demido-tool`) of `tool.json`, `guidance.md` and `tool.wasm`, and its signature
`<package>.minisig`. Its manifest names the tool, its version, its publisher and their minisign key, a sentence for
people, the line a model sees (L18: what the model cannot do without it), its arguments (`text`, `whole`,
`list`), its asks, and where updates are announced (`latest.json`: version, package, signature). A tool's name
cannot be one of Demido's own.

## A tool of Demido's own, as a package (Tried)

`tools/word-count`: the words, lines and characters of a file in the folder. Built from its source, packed and
signed by `tools/word-count/build.sh` with a development key of this computer's (never in the repository), it
installed with `demido tools install … --yes`, showed in the Tools screen with its key and asks, and ran in a
session's turn (a test with a scripted model).

## Not done

A registry to browse (note 08's static signed index), an audit of a tool by a model (note 08: triage, never a
gate), precompiled components kept on disk (`Module::deserialize` is a trust boundary, note 08: each run compiles
again, about a second for this one), WASI 0.3.
