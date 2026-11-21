# 11. The core: what it is built from

Written 2026-11-21, directly. Covers only what the first slice of the core needs: the web server, storage,
settings, the event log, the command line, and reading the hardware. Scheduler and resource manager come with
the features that need them.

Versions and licences were **read from crates.io on 2026-11-21**. Every part below is MIT or Apache-2.0 (or
both), which GPL-3.0-or-later accepts.

## The parts

| For | Part | Version | Licence | Why this one |
|---|---|---|---|---|
| Running many things at once | tokio | 1.54 | MIT | The common base; everything else here builds on it. |
| Web server | axum | 0.8 | MIT | Small, made by the tokio people, by far the most used. |
| The API description | utoipa + utoipa-axum | 6.0, 0.3 | MIT or Apache-2.0 | Writes the OpenAPI document from the code, so the two cannot drift. About 15 times the use of the alternative (aide). |
| Storage | SQLite through rusqlite | 0.43 | MIT | One file, no server, well known. `rusqlite` over `sqlx`: no compile-time database checks to set up, and a local file gains little from async. |
| Command line | clap | 4.6 | MIT or Apache-2.0 | The standard. |
| Processor, memory, system | sysinfo | 0.40 | MIT | Same code on Linux, Windows, macOS. |
| NVIDIA cards | nvml-wrapper | 0.13 | MIT or Apache-2.0 | Asks the driver directly: card, memory, which CUDA it supports. Falls back to `nvidia-smi`. |
| Where files go | directories | 6.0 | MIT or Apache-2.0 | The right folder on each system. |
| Putting the web UI inside the binary | rust-embed | 8.14 | MIT | One file to install on a server. |
| Ids, errors, logs | uuid 1.29, thiserror 2, anyhow 1, tracing 0.1 | | MIT or Apache-2.0 | The usual. |
| Downloading | reqwest | 0.13.5 | MIT or Apache-2.0 | The common HTTP client. Used with rustls and the system's own list of trusted certificates; no OpenSSL. |
| Checksums | sha2 | 0.11.1 | MIT or Apache-2.0 | SHA-256, to check a download against what its publisher lists. |
| Unpacking `.tar.gz` | tar 0.4.46, flate2 1.1.11 | | MIT or Apache-2.0 | The standard pair. |
| Unpacking `.zip` | zip | 9.8.1 | MIT | With only its "deflate" method: the others would pull in C libraries. |

Looked at and not taken, for now: `sqlx` (see above), `keyring` 4.2 (needed when the first secret is stored),
`reqwest` 0.13 (needed when the first download or provider call is made; the command line talks to the local
core with a few lines of its own), `wgpu` 30 (heavy; only to list non-NVIDIA cards, which the system can tell
us more cheaply).

## Decisions

1. **One process owns everything: `demido serve`.** The window, the browser, the command line and other
   programs are all clients of its API. The command line works without it too, straight on the files, for
   the things that make sense offline.
2. **One API, versioned in the path: `/api/v1/…`.** JSON in and out. The OpenAPI document is generated and
   served at `/api/v1/openapi.json`.
3. **Events go out as Server-Sent Events**, not over a WebSocket. They only flow one way, they pass through
   proxies, and a browser reconnects by itself and says where it left off, so nothing is missed. WebSockets
   stay for what truly goes both ways (voice, the built-in browser's picture). This narrows provisional
   decision C4.
4. **The event log is the memory of what happened.** Every change is appended to it and sent to listeners. It
   is how the UI stays current, how the command line follows along, and how "nothing hidden" is kept.
5. **Settings are data, and the core owns them.** One registry holds every setting: id, kind, default,
   range, the text people read, and whether it is built yet. The UI draws its Settings screen from that; the
   command line lists from it. A setting that exists only in the UI cannot happen.
6. **A key is always needed, even on this computer.** On first start the core makes a random key and keeps it
   in a file only the user can read. The command line reads that file. A browser gets in through a
   one-use link, which trades for a session cookie; the key itself never appears in an address. Requests
   with a foreign `Host` are refused (the DNS-rebinding trick). Remote use adds passkeys and encryption later.
7. **The web UI is served by the core** from the same address as the API. From disk while developing; inside
   the binary for releases.
8. **Storage is one SQLite file** in the system's data folder (`DEMIDO_HOME` overrides it), in WAL mode,
   with numbered migrations.

## Added on 2026-11-21: the download parts, and two things checked on the way

Versions and licences read from crates.io that day.

**The encryption library behind reqwest.** reqwest 0.13 uses rustls with `aws-lc-rs` by default. I remembered
`aws-lc-sys` as carrying the old OpenSSL licence, which does not combine with the GPL. That is no longer so:
`aws-lc-sys` 0.47.0 declares ISC, Apache-2.0, MIT, BSD-3-Clause and MIT-0 only. Checked, not assumed, and it
is the reason `scripts/check-licences.mjs` exists: it reads the licence of every package in the build (288 on
that day) and fails on any name that has not been read and listed. It runs in the checks on GitHub.

**A D-Bus library, tried and dropped.** zbus 6.5.0 (MIT) reads the desktop's accent colour properly, and
brings about thirty packages to do it. The system's own `busctl` (or `gdbus`) answers the same question.
zbus comes back when a second thing needs D-Bus (DECISIONS K14).

**Dates and times: jiff.** Added with the chat loop, to tell a model what day it is in this computer's time
zone. jiff 1.0.1, Unlicense OR MIT (read from crates.io on 2026-11-21). It reads the system's own time zone
data on Linux and macOS and carries its own on Windows. I knew its 0.2 names; 1.0 renamed some
(`ZonedDateTime`), which I found by reading its source, not by guessing. The other candidates: `chrono`
(MIT OR Apache-2.0) has no time zone names without a second crate; `time` refuses to read the local time
zone in a program with several threads. The calculator's sums with dates use no library at all.

## Not decided here

How heavy queries leave the request path, the scheduler, the resource manager that lends and unloads models,
and the Windows and macOS services. Each comes with the feature that needs it.

## Sources

crates.io API, one record per crate, for example <https://crates.io/api/v1/crates/axum>.
