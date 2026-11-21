# 08 — Tools system

Research date: 2026-11-20. Research only: nothing was cloned, installed or executed. Every number below comes from a cited page; anything I could not confirm from a primary source is listed in "Not verified" at the end.

## Question

Design the unified Tools system for Demido Studio: one concept that merges MCP servers and skills (a Tool = functionality + editable guidance in one package). Decide the plugin technology (owner's default: "built in Rust"), the MCP and Agent Skills import/convert paths, the minimal-context index that still works for small models, the registry/update/audit flow, and a concrete package format, runtime and lifecycle.

## Short answer

1. A Tool is a **folder with a manifest, guidance and an optional runtime**. Four runtime kinds share one index, one permission system and one UI:
   - `builtin`: native Rust compiled into Demido (first-party: files, shell, browser, search, mail...).
   - `wasm`: a WebAssembly component, written in Rust by default, run in an embedded wasmtime sandbox. This is the installable, third-party form of "Tools built in Rust".
   - `mcp`: an MCP server (stdio subprocess or remote HTTP). Unconverted = "Deprecated". Converted (snapshotted, pinned, lazily started) = "External".
   - `guide`: guidance only, no code. This is what a converted SKILL.md becomes.
2. Do **not** load third-party native dynamic libraries. No sandbox, no stable Rust ABI, one build per OS/arch, a crash kills the app.
3. Target **WASI 0.2 (`wasm32-wasip2`)** for v1 tools. WASI 0.3 is ratified and wasmtime runs it, but wasmtime's own host crates still label their 0.3 implementation experimental. Revisit after a spike.
4. Index for small models: **the host picks tools, not the model.** Always-on namespace catalog (a few hundred tokens) + host-side retrieval that injects the top 3-5 compact schemas per step + one `tools` meta-tool as fallback. Append-only disclosure so the local KV cache is not invalidated. Guidance loads only when a tool is opened.
5. Registry: a **static, signed index in a git repo** (metadata only, artifacts fetched from the publisher's official source, sha256 + minisign). Multiple registries allowed. Auto-update off. Updates that widen permissions always ask.
6. Model audit is **triage, not a safety gate**. The UI must never show "Safe". Real protection is sandbox + least-privilege manifest + pinned hashes.

## Findings

### 1. Sandboxed plugin technology

#### 1.1 WebAssembly Component Model + wasmtime

- **Current wasmtime**: 50.0.3, released 2026-11-19. Same day: 49.0.7, 48.0.10, 36.0.23. License `Apache-2.0 WITH LLVM-exception`. Sources: https://github.com/bytecodealliance/wasmtime/releases , https://crates.io/api/v1/crates/wasmtime , https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/LICENSE
- **Release policy**: new major on the 20th of each month. Normal releases get 2 months of security fixes. Every 12th release (36, 48, 60...) is LTS, supported 24 months. Source: https://docs.wasmtime.dev/stability-release.html . 48 is therefore the current LTS line (planned release 2026-08-20, so supported to about 2028-08).
- **Stability tiers**: `component-model`, `component-model-async`, `wasi-http`, `wasi-sockets` and x86_64 Linux/Windows/macOS are Tier 1. aarch64 Linux/macOS and the Pulley interpreter are Tier 2. aarch64 Windows is Tier 3. Source: https://docs.wasmtime.dev/stability-tiers.html
- **WASI 0.3**: ratified, announced 2026-06-11. Async moves into the Component Model ABI (`async func`, `stream<T>`, `future<T>`); `wasi:io` is gone; host runs one event loop for all components. Wasmtime 46+ enables it by default. Patch releases planned about every two months; 0.3.1 features (`map<K,V>`) adopted 2026-08-06. Sources: https://bytecodealliance.org/articles/WASI-0.3 , https://wasi.dev/releases , https://component-model.bytecodealliance.org/design/migrating-to-p3.html
- **Conflict to be aware of**: docs.rs for `wasmtime-wasi` 50.0.3 still describes its `p3` module as "Experimental, unstable and incomplete implementation of wasip3 version of WASI ... not compliant with semver and is not ready for production use", and `wasmtime-wasi-http` 50.0.3 says the same of its `p3` module. Sources: https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/p3/index.html , https://docs.rs/wasmtime-wasi-http/latest/wasmtime_wasi_http/ . The spec is stable; the host implementation crates do not claim to be. Treat 0.3 as "works, API may still move".
- **0.2 keeps working**: 0.3 is additive; a 0.3 runtime polyfills 0.2 imports. Source: migrating-to-p3 page above.
- **Rust guest toolchain**: `wasm32-wasip1`, `wasm32-wasip2`, `wasm32-wasip3` are all Tier 2 with `std`. `wasm32-wasip3` is "first available on stable in Rust 1.100.0" (the dev machine's version), is not tested in CI, and `std::thread` returns an error (cooperative threads planned "by the end of 2026"). Sources: https://doc.rust-lang.org/rustc/platform-support.html , https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip3.html
- **Build path for authors**: `cargo build --target wasm32-wasip2` + `wit-bindgen` (0.63.0, 2026-10-14, `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`). `cargo-component` "is in the process of being deprecated as native tooling can be used directly". Sources: https://component-model.bytecodealliance.org/language-support/building-a-simple-component/rust.html , https://crates.io/api/v1/crates/wit-bindgen
- **Capabilities are deny-by-default** in `wasmtime-wasi`: no preopened directories, no env vars, stdin closed, all socket addresses denied, name lookup denied. Grants are explicit: `preopened_dir(host, guest, perms)`, `env`, `socket_addr_check(callback)`, `allow_ip_name_lookup`. Outgoing HTTP can be intercepted with `WasiHttpHooks` / `default_send_request`. Sources: https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html , https://docs.rs/wasmtime-wasi-http/latest/wasmtime_wasi_http/
- **Component size** (wasmtime docs, hello world, `wasm32-wasip2`): 2.5 MB default build; 78 KB with `-Cstrip=debuginfo`; 64 KB with `-Copt-level=s -Clto -Ccodegen-units=1`; 50 KB after `wasm-tools strip -a`. Precompiled `.cwasm`: 284 KB default, 129 KB tuned, 90 KB for Pulley. Source: https://docs.wasmtime.dev/examples-minimal-cwasm.html . The component-model book reports a small adder component at 3.3 MB debug vs 16 KB release (page above).
- **Cold start**: wasmtime's guidance is precompile ahead of time, resolve imports once with `InstancePre`, copy-on-write memory init, pooling allocator. It gives no timings. Source: https://docs.wasmtime.dev/examples-fast-instantiation.html . Third-party measurements: about 0.5 ms to instantiate with a fresh store and 256-398 µs per call with an AOT cache (https://arxiv.org/pdf/2605.01037); about 16.9 ms average cold start when compilation is included (https://arxiv.org/pdf/2509.09400). Conclusion: compile at install time, cache, and per-call instantiation is negligible next to LLM latency.
- **Precompiled cache is a trust boundary**: `Module::deserialize` is `unsafe`; feeding modified bytes "can trivially be used to execute arbitrary code"; it only accepts output of the same wasmtime version. Source: https://docs.rs/wasmtime/latest/wasmtime/struct.Module.html . So: never download `.cwasm`; only load what Demido compiled itself into an app-private cache.
- **Execution limits**: epoch interruption (about 10% slowdown, wall-clock based) or fuel (deterministic, slower). Source: https://docs.wasmtime.dev/examples-interrupting-wasm.html
- **The sandbox is not perfect**: the 2026-11-19 patch releases fix a WASI filesystem sandbox escape on Windows via a drive-prefixed path (GHSA-f2pq-268w-886c). The 2026-11-16 batch fixed several guest-triggerable host panics/aborts and unbounded host memory growth. Source: https://github.com/bytecodealliance/wasmtime/releases . Demido must be able to ship wasmtime patch updates fast.

#### 1.2 Prior art that already does "wasm components as agent tools"

- **Zed extensions**: Rust compiled to wasm, WIT-defined API, wasmtime host with `bindgen!`. Extensions are built in CI, not on user machines. The registry is a git repo (`zed-industries/extensions`: submodules + `extensions.toml`, PR review); built archives go to object storage and the app queries an API. Each extension records the API version it was built against and the host keeps bindings per version. Zed first tried an embedded JS runtime and abandoned it. The team expects the central repo to need replacing as it grows. Source: https://zed.dev/blog/zed-decoded-extensions , https://zed.dev/docs/extensions/developing-extensions
- **Microsoft Wassette**: an MCP server built on wasmtime that exposes each exported function of a wasm component as an MCP tool. Deny-by-default; per-component policy for network, storage and environment; components pulled from OCI registries. MIT. README says "Early Development", "not production ready yet". Sources: https://github.com/microsoft/wassette , https://opensource.microsoft.com/blog/2025/08/06/introducing-wassette-webassembly-based-tools-for-ai-agents/

Both confirm the approach is practical. Neither is a dependency.

#### 1.3 Extism

- `extism` 1.41.0 (2026-10-09), BSD-3-Clause, 5.8k stars. Bytes-in/bytes-out plugin calls, host functions, PDKs for about ten languages, "host-controlled HTTP without WASI". Manifest has `allowed_hosts`, `allowed_paths`, memory limits and an optional sha256 per wasm source. Sources: https://crates.io/api/v1/crates/extism , https://github.com/extism/extism , https://extism.org/docs/concepts/manifest/
- It does not use the Component Model / WIT (maintainers said they want more maturity first: https://github.com/extism/extism/issues/666 , undated in my results).
- `allowed_hosts: null` means all hosts are allowed (manifest page above). That is the wrong default for Demido.
- It is a layer over wasmtime. Demido needs a custom typed host API anyway, so Extism adds a second ABI without removing work.

#### 1.4 Native dynamic libraries

No sandbox: the library has the full privileges of the Demido process (keys, mail, browser). Rust has no stable ABI, so plugins need a C ABI shim and a matching toolchain. One binary per OS/arch per tool, so the registry, signing and reproducibility work multiply. A crash or UB in a plugin takes down the app. Not acceptable for downloaded code. Fine only for first-party code, and there static linking (the `builtin` kind) is simpler.

#### 1.5 Subprocess tools (JSON-RPC over stdio)

This is MCP stdio. Any language, native libraries, GPU, crash isolation. But: per-OS binaries or a language runtime (Node, Python) on the user's machine, tens to hundreds of ms cold start plus runtime start, and **no isolation by default**. The MCP project's own guide says a local server "is an ordinary operating system process" that inherits env vars, can read everything the user can, and has unrestricted network; "the stdio transport is not a sandbox"; isolation "is the responsibility of the host application". Source: https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/local-server-security.md

OS sandboxing for subprocesses exists but is uneven: Linux Landlock (`landlock` crate 0.4.7, MIT OR Apache-2.0, https://crates.io/api/v1/crates/landlock) and bubblewrap; macOS Seatbelt via `sandbox-exec`; Windows support in Anthropic's `sandbox-runtime` is described as alpha (https://github.com/anthropic-experimental/sandbox-runtime , Apache-2.0 per secondary sources). The sandbox design itself belongs to the permissions/security report; this report only requires that `mcp` tools run inside it and that the UI says so when it is unavailable.

#### 1.6 Embedded scripting

Rhai 1.27.0 (MIT OR Apache-2.0), mlua 0.12.2 (MIT; Lua/Luau), rquickjs 0.14.0 (MIT) are all license-compatible (crates.io API pages for each). Pydantic's Monty is a Rust-written Python-subset interpreter for agent code (released 2026-02-27, https://pydantic.dev/articles/pydantic-monty ; license not verified). Good for small glue and for "code mode". Bad as the package runtime: each engine is its own ecosystem, capability control is hand-rolled per binding, and tool authors lose the Rust ecosystem. A scripting language can still be shipped later as a tool, or compiled into a wasm component.

### 2. Signing, verification, reproducible builds

- **minisign**: `minisign-verify` 0.3.0 (2026-09-25), MIT, zero dependencies, verify-only, supports pre-hashed and streaming modes. Sources: https://crates.io/api/v1/crates/minisign-verify , https://docs.rs/minisign-verify . Ed25519 with a pinned public key. Works offline. Proves "the holder of this key signed these bytes", nothing else.
- **Sigstore**: two Rust lines exist. `sigstore` 0.15.0 (2026-11-05, Apache-2.0, repo sigstore/sigstore-rs) describes itself as "An experimental crate to interact with sigstore". `sigstore-verify` 0.16.0 (2026-11-09, Apache-2.0, repo sigstore/sigstore-rust, 12 crates, bundle formats v0.1-v0.3, example verifies a cosign v3.0.2 bundle; no stability statement; trust root loaded via TUF). Sources: https://crates.io/api/v1/crates/sigstore , https://crates.io/api/v1/crates/sigstore-verify , https://github.com/sigstore/sigstore-rust . Keyless signing ties an artifact to a CI identity (repo + workflow) and a transparency log, which is stronger provenance than a bare key, but both crates are pre-1.0, pull in many dependencies and need a network refresh of the trust root.
- **Reproducible builds**: for Rust-to-wasm the usual recipe is a pinned toolchain, committed `Cargo.lock` with `--locked`, path remapping (`--remap-path-prefix` / Cargo `trim-paths`), and a pinned builder image; cross-architecture reproducibility is not guaranteed. Sources: https://reproducible-builds.org/docs/rust/ , https://docs.internetcomputer.org/guides/canister-management/reproducible-builds/ , https://rust-lang.github.io/rfcs/3127-trim-paths.html . Wasm is a better candidate than native code (one target, no system linker), but build scripts and proc macros can still be non-deterministic. Treat reproducibility as a badge, not a requirement.

### 3. MCP: current state

- **Current spec revision: 2026-07-28** (previous: 2025-11-25). It is a breaking, stateless redesign. Sources: https://modelcontextprotocol.io/specification/2026-07-28/changelog.md , https://blog.modelcontextprotocol.io/posts/2026-07-28/
  - No `initialize` handshake, no protocol session, no `Mcp-Session-Id`. Every request carries protocol version and client capabilities in `_meta`. Servers must implement `server/discover`.
  - **MRTR** (multi round-trip requests): servers no longer send requests to the client. A result with `resultType: "input_required"` carries `inputRequests`; the client retries with `inputResponses`. This is how elicitation now works.
  - `tools/list`, `prompts/list`, `resources/list` must not vary per connection, should be deterministically ordered, and carry `ttlMs` + `cacheScope`.
  - `subscriptions/listen` replaces the GET stream and `resources/subscribe`. `ping`, `logging/setLevel` removed.
  - **Deprecated** (12-month minimum window): Roots, Sampling, Logging, HTTP+SSE transport, OAuth Dynamic Client Registration (replaced by Client ID Metadata Documents).
  - `inputSchema`/`outputSchema` accept any JSON Schema 2020-12; `structuredContent` may be any JSON value.
- **Tools**: name, title, description, icons, inputSchema, outputSchema, annotations. "clients MUST consider tool annotations to be untrusted unless they come from trusted servers." Stateful tools use explicit handles passed as arguments. Source: https://modelcontextprotocol.io/specification/2026-07-28/server/tools.md
- **stdio transport**: newline-delimited JSON-RPC; shutdown = close stdin, wait, then kill. "Because the protocol is stateless, any in-flight requests are simply lost and the client can retry them against the fresh process." Clients detect legacy servers by probing `server/discover` and falling back to `initialize`. Source: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio.md . This makes lazy start / idle stop a first-class, spec-supported pattern for modern servers.
- **Elicitation**: form mode (flat primitive schema) and URL mode (out-of-band, for secrets, OAuth, payments). Form mode must not request secrets. Client must show which server asks, show the full URL, never prefetch it. Source: https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation.md
- **Extensions** (now a formal framework, always opt-in): MCP Apps `io.modelcontextprotocol/ui` (Stable), OAuth Client Credentials and Enterprise-Managed Authorization (Stable), Tasks `io.modelcontextprotocol/tasks` (Beta, heading to Core), **Skills over MCP** `io.modelcontextprotocol/skills` (Beta), Server Cards (Beta), Events (Beta). Source: https://modelcontextprotocol.io/docs/extensions/overview.md
  - **Tasks**: server may answer `tools/call` with `resultType: "task"`; client polls `tasks/get`, answers `input_required` via `tasks/update`, may `tasks/cancel`. Source: https://modelcontextprotocol.io/extensions/tasks/overview.md
  - **MCP Apps**: a tool declares `_meta.ui.resourceUri` pointing to a `ui://` HTML resource; the host renders it in a sandboxed iframe and talks JSON-RPC over `postMessage`. Supported by Claude, VS Code Copilot, Goose and others. Source: https://modelcontextprotocol.io/extensions/apps/overview.md
  - **Skills over MCP**: `skills/list` and `skills/get` return SKILL.md frontmatter plus a full file manifest with SHA-256 digest and size per file; files are read through `resources/read`. Hosts MUST verify size and digest, MUST NOT fetch files ahead of need, MUST bind approval to the full set of URIs and digests (any change revokes approval), MUST treat content as untrusted, and need explicit per-skill approval for `allowed-tools` or code execution. Limits: 512 files / 16 MiB per skill. Source: https://modelcontextprotocol.io/extensions/skills/overview.md . This is the same "snapshot, hash, approve, re-approve on change" model this report recommends for all tools.
- **Registry**: official metadata registry at `https://registry.modelcontextprotocol.io`, still labelled **preview** ("Breaking changes or data resets may occur"). It stores `server.json` metadata that points to packages on npm, PyPI, NuGet, crates.io, OCI registries or `.mcpb` release files; namespaces are verified by GitHub/DNS/HTTP (`io.github.user/server`). It performs **no security scanning** and gives no uptime guarantee. Read API is unauthenticated: `GET /v0.1/servers` with `cursor`, `limit`, `updated_since`. Host apps are told to consume sub-registries rather than the official one directly; aggregators should poll about hourly and keep their own copy. Registry data is CC0. Sources: https://modelcontextprotocol.io/registry/about.md , https://modelcontextprotocol.io/registry/registry-aggregators.md , https://modelcontextprotocol.io/registry/package-types.md , https://modelcontextprotocol.io/registry/terms-of-service.md
  - `.mcpb` (MCP Bundles, formerly DXT) entries must carry `fileSha256`; "MCP clients do validate the hash before installation". Source: package-types page and https://github.com/modelcontextprotocol/mcpb
- **Security guidance from the MCP project itself** (local-server-security page above): pin versions; a version pin on `npx`/`uvx` does not pin transitive dependencies, an OCI digest does; tool descriptions are an injection surface and "a server can quietly change them after you approved it (a rug pull)"; "Prefer clients that pin tool definitions"; "no check proves what a server will do"; run third-party servers isolated; pass secrets per server; deny network where not needed.
- **Official Rust SDK `rmcp`**: 3.11.0 released 2026-11-17, license field `Apache-2.0`, 52.6 M total downloads. Six releases between 2026-09-28 and 2026-11-17, so the API moves fast. Tier 1 SDK. Implements 2026-07-28 and stays compatible with 2025-11-25. Transports: stdio, child process (client), Streamable HTTP (client and server); legacy HTTP+SSE intentionally absent. Supports OAuth, elicitation (form and URL), Tasks extension, MRTR, caching, subscriptions. The repo LICENSE describes a transition from MIT to Apache-2.0 (new code Apache-2.0, unrelicensed contributions MIT, docs CC-BY-4.0). Sources: https://crates.io/api/v1/crates/rmcp , https://github.com/modelcontextprotocol/rust-sdk , https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/LICENSE , https://modelcontextprotocol.io/docs/sdk.md

### 4. Agent Skills (SKILL.md)

- Spec: https://agentskills.io/specification . A skill is a directory with `SKILL.md` (YAML frontmatter + Markdown) and optional `scripts/`, `references/`, `assets/`.
- Frontmatter: `name` (required, 1-64 chars, lowercase/digits/hyphens, must match the directory), `description` (required, max 1024 chars, what it does and when to use it), `license`, `compatibility` (max 500), `metadata` (string map), `allowed-tools` (space-separated, experimental).
- Progressive disclosure: about 100 tokens of metadata always loaded, body (recommended under 5000 tokens / 500 lines) loaded on activation, files on demand.
- Originated at Anthropic, now an open repo with governance files; code Apache-2.0, docs CC-BY-4.0; 27.5k stars. Source: https://github.com/agentskills/agentskills
- Clients add their own fields. Claude Code adds `when_to_use`, `disable-model-invocation`, `user-invocable`, `disallowed-tools`, `model`, `effort`, `context: fork`, `agent`, `hooks`, `paths`, `arguments`, plus `$ARGUMENTS` substitution and `` !`command` `` blocks that run a shell command when the skill is rendered. It truncates description text at 1,536 characters in its listing. Source: https://code.claude.com/docs/en/skills . An importer must tolerate unknown keys and must not execute `` !`...` `` blocks.
- **Skills are an active attack surface.** Snyk scanned 3,984 skills from ClawHub and skills.sh (2026-02-05): 36.82% had at least one security flaw, 13.4% a critical one, 76 were confirmed malicious; 91% of the malicious ones used prompt injection in the instruction text, alongside obfuscated install commands and links to external malware. Source: https://snyk.io/blog/toxicskills-malicious-ai-agent-skills-clawhub/ . Guidance text is executable in practice: the model follows it.

### 5. Minimal-context indexing

What the sources say:

- **Full schemas up front do not scale.** Anthropic: 58 tools across five MCP servers cost about 55K tokens; selection accuracy "degrades once you exceed 30-50 available tools". Sources: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool , https://www.anthropic.com/engineering/advanced-tool-use . At small context it is a hard failure: with an 8K window, 28 JSON-schema tool definitions "overflow the context window entirely" (2.6% exact match); compressed schemas saved 44-50% of schema tokens and restored function (+20.5 pp average over eight models, 1.5B-32B tested). Single-author preprint: https://arxiv.org/abs/2605.26165
- **Tool-search meta-tool (Anthropic)**: tools marked `defer_loading` stay out of context; the model calls a regex or BM25 search tool; up to 5 matches are expanded inline as `tool_reference` blocks, which keeps the prompt prefix stable for caching. Reported: over 85% token reduction; internal MCP eval accuracy Opus 4 49% to 74%, Opus 4.5 79.5% to 88.1%. Advice: keep the 3-5 most-used tools always loaded, prefix names by service, add a system-prompt line listing tool categories, put task keywords in descriptions. Custom client-side search (for example embeddings) is supported by returning `tool_reference` blocks. Sources as above. These numbers are for frontier models; the technique depends on the model noticing it lacks a tool and writing a good query.
- **Code execution with MCP (Anthropic, 2025-11-04)**: expose servers as a file tree of typed code wrappers; the model lists directories, reads only what it needs, and writes code that calls tools and filters data before anything reaches context. One example went from about 150K to 2K tokens. The post itself says it needs "a secure execution environment with appropriate sandboxing, resource limits, and monitoring". Source: https://www.anthropic.com/engineering/code-execution-with-mcp . Programmatic tool calling: 37% average token reduction and small accuracy gains on complex tasks (advanced-tool-use post).
- **Host-side retrieval before the model runs**: RAG-MCP reports tool-selection accuracy 13.62% to 43.13% and over 50% fewer prompt tokens (https://arxiv.org/abs/2505.03275 ; abstract does not name models). "Less-is-More" (DATE 2025) shows that giving edge-deployed LLMs fewer, relevant tools improves success and cuts execution time by up to 70% (https://arxiv.org/abs/2411.15399). TinyAgent (Berkeley, EMNLP 2024 demo) pairs 1.1B/7B models with a tool-retrieval step (https://arxiv.org/abs/2409.00608 ; the 0.998 recall figure I saw is from a secondary summary).
- **Small models and format**: an empirical study of small models on function calling found they "struggle significantly with adhering to the given output format", improving from zero-shot to few-shot to fine-tuning (https://arxiv.org/abs/2504.19277).

What this means for small local models (my assessment; there is no solid public benchmark for sub-14B models choosing among deferred tools, so Demido must measure it):

| Approach | Always-on cost | Extra model steps | Fit for small models |
|---|---|---|---|
| All schemas always loaded | Very high | 0 | Fails beyond about 10-20 tools; fails outright at 8K |
| Always-on one-line catalog of every tool | Medium, grows linearly | 1 (open) | Good up to a few dozen tools; the model can see what exists |
| Namespace catalog (two levels) | Low, near-constant | 1-2 | Good; choosing among about 10 short lines is easy |
| Host-side retrieval (BM25 / embeddings) | None | 0 | Best default: no model skill needed. Fails when the needed tool is not lexically or semantically near the request |
| Tool-search meta-tool only | Lowest | 1+ | Weak alone: model must know it is missing something and query well; extra slow local round trip |
| Code execution over tool APIs | Lowest | several | Poor default for small models; errors compound; needs a code sandbox |

Local-inference detail that changes the design: tool definitions normally sit at the top of the prompt. Rewriting them each turn invalidates the KV cache and forces full prompt re-evaluation. Anthropic solves the same problem by appending discovered tool definitions inline instead of touching the prefix. Demido should do the same: stable prefix, append-only tool disclosure.

### 6. Registries and supply chain

- Git-as-registry is proven at Zed's scale but needs human review and CI (section 1.2).
- The official MCP Registry is metadata-only with verified namespaces and no scanning (section 3).
- Skill marketplaces without review filled with malicious entries within months (section 4).
- Attackers already target LLM-based scanners (section 7).

### 7. Limits of LLM audits

- 13 LLMs on 4,070 PyPI packages (370 malicious): package-level mean F1 ranges 0.40-0.99 depending on model and prompt; models catch most malware but flag safe packages too; identifying the actual malicious lines is weak (weighted F1 0.48); they "report absent indicators as present"; longer, more complex code is harder; model size and context width were not significant factors. Authors' recommendation: use LLMs for triage, not as the decision. Source: https://arxiv.org/abs/2602.16304
- In the wild (2026-06): an npm malware sample opened with text engineered to trigger the scanner model's safety refusal so the pipeline would record "nothing found". Some model/API combinations refused; others flagged it. JFrog's rules: treat the file as fenced data; a refusal is not a pass; keep non-AI detections running; an injection attempt is itself a finding. Source: https://research.jfrog.com/post/prompt-injection-vs-scanners/ . Zscaler reports the same technique in PyPI packages from 2026-06-08: https://www.zscaler.com/blogs/security-research/shai-hulud-campaign-evolution-miasma-hades-and-ai-scanner-evasion
- Structural blind spots no model can fix: a remote HTTP MCP server's behaviour, dependencies resolved at install time, binaries without matching source, delayed or targeted payloads, and anything that changes after the audit.

## Options compared

### Runtime for installable tools

| | Wasm component (wasmtime) | Extism | Native dylib | Subprocess / MCP | Embedded script |
|---|---|---|---|---|---|
| Sandbox | Strong, deny-by-default, enforced by what is linked | Strong (same engine), weaker defaults | None | None unless OS sandbox added | Depends on engine bindings |
| One artifact for all OS/arch | Yes | Yes | No | No (or needs Node/Python) | Yes |
| Rust-first authoring | Yes (`wasm32-wasip2`, Tier 2) | Yes (PDK) | Yes, C ABI only | Yes | No |
| Typed interface | WIT | Bytes | C ABI | JSON-RPC + JSON Schema | Engine-specific |
| Idle cost | Zero (not loaded) | Zero | Loaded in process | Process RAM until stopped | Zero |
| Cold start | Sub-ms after precompile | Similar | Fast | 10s-100s of ms + runtime | Fast |
| Native libs / GPU / threads | No | No | Yes | Yes | No |
| Reproducible build / audit of what it can touch | Good: imports list is ground truth | Medium | Poor | Poor | Good (source is the artifact) |
| Host dependency weight | wasmtime (large crate, monthly majors) | wasmtime + Extism | None | None (rmcp) | Small |
| License | Apache-2.0 WITH LLVM-exception | BSD-3-Clause | n/a | rmcp Apache-2.0 | MIT / Apache-2.0 |
| Verdict | **Use for third-party tools** | Reject: extra ABI, no WIT | **Reject** | **Use for External (MCP)** | Later, as optional "code mode" |

### Signing

| | minisign | Sigstore keyless |
|---|---|---|
| Proves | Key holder signed these bytes | A named CI identity built/signed these bytes; logged publicly |
| Offline | Yes | Needs trust-root refresh |
| Rust crate | `minisign-verify` 0.3.0, zero deps, MIT | `sigstore-verify` 0.16.0, pre-1.0, many deps, Apache-2.0 |
| Key management | Publisher keeps a key; continuity checks needed | No long-lived key |
| Verdict | **v1 baseline** | Optional provenance badge later |

### Registry shape

| | Central API service | Static signed index in git | OCI registry (wkg) | Reuse MCP Registry only |
|---|---|---|---|---|
| Running cost | Server + ops | None (git host + CI) | None, but adds OCI client | None |
| Decentralized | No | Yes (add any index URL) | Yes | No |
| Review trail | Custom | PRs | None | None |
| Covers wasm + guide tools | Yes | Yes | Yes | No (MCP servers only) |
| Verdict | Reject for now | **Use** | Reject for v1 | Use as an extra source for External tools |

## Recommendation

### R1. One concept, four kinds, three labels

```
Tool package = manifest (tool.toml) + guidance (GUIDE.md) + optional runtime + schemas + evals
```

| Kind | Runtime | Who writes it | Label in UI |
|---|---|---|---|
| `builtin` | Native Rust crate compiled into Demido, behind a `Tool` trait | Demido | Built-in |
| `wasm` | Wasm component in embedded wasmtime | Anyone, Rust by default | Tool |
| `guide` | None (instructions + reference files) | Anyone; result of converting a skill | Tool |
| `mcp` converted | MCP server, pinned + snapshotted + lazy | Converted in-app | External |
| `mcp` raw / classic SKILL.md | As the ecosystem ships it | Imported | Deprecated |

Built-ins use the exact same manifest, guidance file, index entry and permission block as installed tools; they are just linked in. That keeps one code path and makes built-in guidance editable in-app like everything else.

Answer to "Tools are built in Rust; is there a better way?": keep Rust as the authoring language, change the delivery form. First-party tools are native Rust. Installable tools are Rust compiled to a wasm component. That gives the sandbox, one artifact for all platforms (which is what makes a lightweight installer, a registry, signatures and reproducible builds tractable), zero idle cost, and lets non-Rust authors take part without Demido doing extra work. Anything that needs native libraries, a GPU or threads is an MCP server (External) or a built-in.

### R2. Package layout

```
dev.example.github/                 # folder name = tool id
  tool.toml                         # manifest (below)
  GUIDE.md                          # guidance: short, caveman style, editable in app
  guide/                            # optional long sections, loaded on demand
    auth.md
    pagination.md
  schema/tools.json                 # function schemas (JSON Schema 2020-12)
  evals/selection.jsonl             # {"q": "...", "fn": "github.issue_create"} lines
  bin/tool.wasm                     # kind = wasm
  mcp/launch.json                   # kind = mcp: command/url, package pin, integrity
  mcp/snapshot.json                 # kind = mcp: discover + tools/prompts/resources lists
  assets/icon.svg
  LICENSE
  CHANGELOG.md
  tool.lock                         # sha256 + size of every file above
  tool.lock.minisig                 # publisher signature over tool.lock
```

User edits never touch the package (its hashes must stay valid). They live in an overlay:

```
<data>/tools/overrides/dev.example.github/
  GUIDE.md            # replaces or patches packaged guidance
  tool.local.toml     # narrowed permissions, disabled functions, pinned/unpinned, edited summary
```

On update the overlay is kept; if the packaged guidance changed under an edited overlay, show a three-way diff.

### R3. Manifest (`tool.toml`)

```toml
schema = 1
id = "dev.example.github"          # reverse-DNS; namespace ownership verified by the registry
name = "GitHub"
version = "1.2.0"
kind = "wasm"                      # builtin | wasm | mcp | guide
license = "MIT"                    # SPDX
authors = ["Example Dev"]
repository = "https://github.com/example/demido-github"
source_commit = "9f2c..."          # commit the artifact was built from

[index]
namespace = "github"               # function prefix and catalog line
summary = "GitHub issues, PRs, repos."            # <= 80 chars, shown in always-on catalog
use_when = ["issue", "pull request", "PR review", "repo search"]   # retrieval text
pinned = false                     # true = schema always in context

[[functions]]
name = "issue_create"              # full name: github.issue_create
summary = "Create issue in repo."
effect = "external_write"          # read | write | destructive | external_read | external_write
schema = "schema/tools.json#/issue_create"
guide = "GUIDE.md#issues"          # section loaded with this function
timeout_ms = 20000

[permissions]                      # everything not listed is denied
net = ["api.github.com:443"]
fs_read = []                       # scoped roots only: "$SESSION", "$TOOL_DATA", "$PICK"
fs_write = ["$TOOL_DATA"]
secrets = ["GITHUB_TOKEN"]         # by reference; host attaches, tool never reads the value
tools = []                         # other tools it may call through the host
ask_user = true                    # may request input (maps to elicitation UI)
model = false                      # may ask the host for an LLM completion
exec = []                          # host commands; not available to wasm

[limits]
memory_mb = 128
output_bytes = 65536               # overflow goes to an artifact file, model gets a pointer

[runtime.wasm]
component = "bin/tool.wasm"
world = "demido:tool/tool@1"
sha256 = "..."

# kind = "mcp" instead:
# [runtime.mcp]
# transport = "stdio"              # stdio | http
# launch = "mcp/launch.json"
# protocol = "2026-07-28"          # negotiated at conversion
# state = "stateless"              # stateless | session (legacy servers)
# idle_stop_s = 300
# snapshot = "mcp/snapshot.json"
# snapshot_sha256 = "..."

[compat]
demido = ">=0.3"

[provenance]
publisher_key = "RWQ..."           # minisign public key
reproducible = true                # registry CI rebuilt source_commit and got the same sha256
sigstore_bundle = ""               # optional, later
```

`effect` is the bridge to the permission levels in brief 4.16 (Cautious / Balanced / Autonomous / Smart). For External tools the MCP annotations (`readOnlyHint`, `destructiveHint`...) are imported as unverified claims; missing or untrusted means "treat as destructive".

### R4. Wasm runtime

- **Engine**: wasmtime, embedded, isolated in one crate (`demido-tools-wasm`) behind the common `Tool` trait so it can be swapped. Start on the **48.x LTS line** (48.0.10 today) unless the spike needs something newer. Subscribe to wasmtime advisories; a wasmtime security release must be able to ship as a Demido patch release within days.
- **Guest target**: `wasm32-wasip2`. Re-evaluate `wasm32-wasip3` when wasmtime's `p3` host modules drop the "experimental" label.
- **Interface** (illustrative WIT, not compiled or validated):

```wit
package demido:tool@1.0.0;

interface host {
  kv-get: func(key: string) -> option<list<u8>>;
  kv-set: func(key: string, value: list<u8>) -> result<_, string>;
  /// Attach a named secret as a header on requests to an allowed host.
  /// The component never sees the secret value.
  secret-header: func(secret: string, header: string) -> result<_, string>;
  ask-user: func(message: string, schema-json: string) -> result<string, string>;
  call-tool: func(name: string, args-json: string) -> result<string, string>;
  progress: func(message: string, fraction: option<f32>);
  log: func(level: u8, message: string);
}

world tool {
  import host;
  // plus the WASI 0.2 interfaces the manifest grants:
  // wasi:clocks, wasi:random, wasi:http/outgoing-handler, wasi:filesystem (preopens)
  export call: func(function: string, args-json: string) -> result<string, string>;
}
```

  One dynamic `call(function, json) -> json` export keeps the host bindings fixed, matches LLM tool calling and MCP, and avoids regenerating bindings per tool. Schemas ship in `schema/tools.json`, generated at build time from Rust types by the SDK. Version the world; the host keeps bindings for every supported major (Zed's approach).
- **SDK**: a small `demido-tool` guest crate (derive macro: Rust fn + doc comment to schema + dispatcher) and a `cargo generate`-style template. Authors need only `rustup target add wasm32-wasip2`.
- **Capability enforcement**:
  1. At install: read the component's imports. If it imports an interface the manifest does not grant, refuse. This check is deterministic and is the ground truth of what the code can touch.
  2. At instantiate: build the WASI context from the manifest (preopens for granted roots only, `socket_addr_check` denying everything unless raw sockets are granted, HTTP hook enforcing the `net` allow-list, redirect targets re-checked, response size capped).
  3. Raw sockets (`wasi:sockets`) are a separate, rarely granted permission.
- **Execution**: fresh `Store` per call (no state leaks between calls; state goes through `kv-*`), epoch-interruption deadline, memory limiter, output cap. Results go through the output compressor and are tagged with provenance for the anti-injection layer.
- **Caching**: compile at install into an app-private cache keyed by (component sha256, wasmtime version, target triple). Never accept a downloaded `.cwasm`. Recompile transparently after a Demido update. Drop compiled components from memory after idle.
- **Spike before committing (about two days)**: measure Demido binary-size and build-time delta from wasmtime, install-time compile and per-call latency for a realistic HTTP+JSON tool, on Linux, Windows and macOS. I could not verify these numbers from sources.

### R5. MCP: client, Deprecated mode, conversion to External

- Use **rmcp** (pin an exact version, wrap it in one crate; it ships about weekly). Speak 2026-07-28, fall back to the `initialize` handshake for legacy servers as the spec describes.
- Implement as client: tools, resources, prompts, MRTR, elicitation (form + URL, mapped to Demido's own ask-user UI with the spec's display rules), list caching (`ttlMs`), OAuth for remote servers. Later: Tasks extension (map to the task list / background jobs), Skills-over-MCP (import path), MCP Apps (sandboxed iframe in the chat; overlaps with brief 4.5 "custom elements"). Do not implement Sampling, Roots or Logging: deprecated in the current spec. Show them as "not supported (deprecated upstream)" so nothing is hidden.
- **Deprecated (raw MCP)**: works, but: started when its session needs it, live `tools/list` each time, nothing pinned, no guidance, flagged "unpinned" with stricter prompts. It still goes through the same index (its tools are indexed from the live list, never dumped wholesale into context). The UI offers "Convert" on every such entry.
- **Convert to External** (mostly deterministic; a model only polishes text):
  1. **Resolve**: registry `server.json`, an `.mcpb`, a pasted command, or a URL.
  2. **Pin**: exact version + integrity (npm `integrity`, PyPI sha256, OCI digest, `.mcpb` `fileSha256`, crates.io checksum). Install into a Demido-managed directory with a lockfile and hash the lockfile, because a top-level pin does not pin transitive dependencies. Disable package install scripts by default.
  3. **Probe in the sandbox with no secrets and no network unless needed**: `server/discover` (or legacy `initialize`), `tools/list`, `prompts/list`, `resources/list`, `resources/templates/list`, `skills/list` if advertised. Stop the server.
  4. **Snapshot**: canonical JSON of all of it to `mcp/snapshot.json`; store its sha256.
  5. **Generate the index entry**: namespace, one-line summary, `use_when`, one-line summary per function, compact schemas (long descriptions and examples move into guidance). Deterministic truncation works without a model; a model, if available, rewrites to caveman style.
  6. **Generate guidance draft** from server instructions, tool descriptions and prompts. The user can edit it.
  7. **Draft permissions**: env vars and secrets from `server.json`, network hosts unknown until declared (ask), filesystem roots (ask).
  8. **Static checks, optional model audit, user approval**. Result: a normal Tool package with `kind = "mcp"`.
- **Runtime of an External tool**:
  - Not running until a function is called. Start, call, keep alive for `idle_stop_s`, then close stdin, wait, kill. Stateless (2026-07-28) servers can be stopped at any time. Legacy servers with `state = "session"` stay up for the session or until idle; the UI says so.
  - The model only ever sees the **snapshotted, audited** names, descriptions and schemas, never live ones.
  - **Drift check on every start**: hash the live lists against the snapshot. On mismatch: block the tool, show the diff, require re-approval (and re-audit if enabled). This is the rug-pull defence the MCP guide asks clients to provide.
  - Remote HTTP servers: schemas can be pinned, behaviour cannot. Label "External (remote)" and say exactly that.
  - Runs in the OS process sandbox with only its own secrets and its declared network/filesystem scope. If no sandbox is available on the platform, show "Unsandboxed" on the tool.

### R6. Agent Skills: import and convert

- **Import as classic skill (Deprecated)**: copy the folder read-only, record source URL/commit and a digest per file (same shape as the Skills-over-MCP manifest), index `name` + `description`, load the SKILL.md body on activation, load referenced files on demand. Unknown frontmatter keys are kept and displayed. `allowed-tools` is shown, not honoured. `` !`command` `` blocks are never executed. Bundled scripts run only through the normal shell tool under the normal permission level.
- **Convert to a Tool** (`kind = "guide"`):
  - `id` from source + name; `summary` and `use_when` derived from `description` (caveman rewrite by a model, or truncation without one).
  - `GUIDE.md` = SKILL.md body. Optional model pass compresses it to caveman style; the original is kept next to it and the diff is shown, because compression can drop a step.
  - `references/` to `guide/`, `assets/` to `assets/`.
  - `scripts/`: kept as files; the manifest gains `exec` permission entries naming each script and its interpreter, which the user must approve. Porting a script to a wasm function is a manual follow-up, offered but not automatic.
  - Client-specific fields map where Demido has an equivalent (`context: fork` to "run in a sub-agent", `model` to a model hint, `paths` to an activation filter) and are otherwise listed as "not applied".
- Guidance is the highest-risk content in the whole system because the model obeys it. Converted skills go through the same static checks and audit as code, and the anti-injection layer should treat third-party guidance as lower-trust than first-party system text.

### R7. Index: layered, host-driven, append-only

**L0, always in context (fixed small budget, scaled to the model's context size)**

```
TOOLS. load: tools.open <name>
fs: read, write, search files
web: search web, fetch page
mail: read, search, send email
github: issues, PRs, repos [ext]
...
```

One line per namespace, about 8-15 tokens each by my estimate (not measured). Plus full compact schemas of 3-5 pinned core functions (task list, ask user, and whatever the session profile pins). When there are more namespaces than the budget allows, the least-used ones collapse into "more: tools.find <words>".

**L1, host-side retrieval, no model step**

Before each generation the host scores tool cards (summary, `use_when`, function names and argument names, `evals/selection.jsonl` queries) against the user message and the active task/sub-task text, and appends the top-k compact schemas (k = 3-5 for small models, more when context allows). Lexical BM25 first: it costs no model and no VRAM. If an embedding model is already loaded for another feature, fuse its scores; never load one only for this. Card embeddings are computed once at install.

**L2, model-driven fallback**

One meta-tool, two operations: `tools.open(name)` and `tools.find(words)`. The result is appended inline (schemas + the function's short guidance). With grammar-constrained decoding the tool-name enum contains only loaded functions plus `tools.*`, so a small model cannot call something it has not loaded and cannot produce a malformed call.

**L3, guidance on demand**

A function's guidance section (aim for under about 150 tokens) arrives with its schema. Longer sections in `guide/` load only when the model or the host asks (`tools.guide`). Guidance never sits in context for tools that are not open.

**Rules that make it work locally**

- **Append-only within a task step.** Never rewrite the system/tool prefix mid-conversation; append newly opened tools as a later message so the KV cache survives. Loaded tools stay loaded (LRU eviction only at compaction).
- **Compact rendering.** Render schemas in the model family's native tool format but strip descriptions to one line and drop examples; validate and repair arguments host-side against the full JSON Schema ("self-sanitizing tool requests"). A signature-style rendering (`mail.send(to: str[], subject: str, body: str)`) may save more; test per model before adopting.
- **Smaller candidate sets beat smarter selection.** Characters and sessions carry a tool profile (allowed namespaces + pins). Sub-agents get only the tools their sub-task needs.
- **Measure it.** Ship a selection benchmark: every tool carries example queries; the app can run "which function would you pick" for the current model and show retrieval recall@k and end-to-end accuracy. This is how Demido finds the real per-model limits, and it gives users a transparent answer to "can my model handle these tools".

Not recommended as defaults for small models: tool-search as the only mechanism, and code execution over tool APIs. Offer code mode later as an opt-in for strong models.

### R8. Registry, updates, audit

**Registry**

- A registry is a base URL serving static files: `index.json` (tool id, versions, per version: manifest digest, `tool.lock` digest, artifact URL at the publisher's official source, sha256, publisher key, flags such as `yanked`) and `index.json.minisig`. Authoring and review happen in a git repo by pull request; clients fetch over HTTPS and need no git.
- The registry holds **metadata only**. Artifacts are downloaded from the publisher's release URL and verified by sha256 and signature. This matches the brief's no-redistribution rule and the MCP Registry model.
- Users can add any number of registries, or install from a folder, a file or a git URL with no registry at all. The official registry's key ships in the app; third-party registries are trust-on-first-use with the key fingerprint shown.
- Namespaces are tied to a verifiable owner (for example `io.github.<user>.*` requires the PR to come from that account), as in the MCP Registry.
- Registry CI per submission: manifest schema, hashes, component imports ⊆ declared permissions, license is on the allowed list, static scans (below), and a rebuild from `source_commit` in a pinned builder image; matching sha256 sets `reproducible = true`.
- External candidates can also be browsed from any registry implementing the MCP Registry API. They are listed as "MCP server, unreviewed" and always go through conversion.

**Verification on every install and update (not optional)**

sha256 of every file against `tool.lock`; minisign signature of `tool.lock`; publisher key continuity (a changed key blocks the update and explains why); component imports vs manifest; `yanked` flag.

**Update flow**

1. Check: fetch index (conditional request), compare versions. No telemetry. Manual by default; optional scheduled check that only notifies.
2. Show, per update: changelog; **permission diff** (new capabilities highlighted); **function/schema diff** (added, removed, changed descriptions); **guidance diff**; source commit range; signature and reproducibility status; audit result if enabled.
3. User approves. Download to quarantine, verify, install side by side, switch atomically, keep the previous version for one-click rollback.
4. Auto-update (off by default; per tool or global; the toggle states plainly that the user accepts unreviewed code). Even when on: an update that widens permissions, changes the publisher key, or fails verification or audit is never applied automatically.

**Audit by a chosen model**

Order of operations: deterministic checks first, model second, human last.

- Deterministic checks (always run, shown regardless of audit setting): permission diff; imports list; hidden or bidirectional Unicode and zero-width characters; base64/hex blobs; `curl | sh` patterns; URLs and hosts not in the `net` list; text addressed to an AI ("ignore previous", "you are", "do not report"); install scripts; dependency lock diff.
- What the auditor model is given: the manifest and permission diff; the guidance diff; the function names, descriptions and schema diff; the source diff when source is available; for wasm, the import list and whether the binary was reproduced from that source; for MCP packages, first-party source, install scripts and the lockfile diff. Everything is fenced as data with an instruction that it is the subject of analysis.
- What it must return (grammar-constrained JSON): overall risk level; findings with file, line, category, quoted evidence and confidence; for each requested permission, whether the code visibly needs it; a list of what it did not read.
- Pipeline rules: a refusal, timeout or malformed answer is "audit failed", never a pass. Detected injection text is itself a high finding. Content is chunked with coverage accounting. The report is stored with the hashes of what was audited; updates audit only the diff plus a summary of the previous report. Audits are queued jobs under the resource-aware model scheduler; a remote paid model requires the usual cost confirmation.
- What the UI says (wording matters):
  - Result label: "No issues found by `<model>`" or "`<n>` issues found by `<model>`". Never "Safe", never a green shield.
  - Always next to it: coverage ("read 14 of 14 source files, 100%" or "read 38% of the package; dependencies not read"), date, and the version/hash audited.
  - A fixed, short limits note: "A model read this code and reported what it noticed. Models miss things and can be tricked by the code they read. This does not prove the tool is safe. Your protection is the permission list above and the sandbox."
  - Explicit blind-spot badges where they apply: "binary not matched to source", "remote server: behaviour cannot be inspected", "dependencies not audited".
  - If source was audited but the wasm binary was not reproduced from it, say that the audit covers the source, not the file that will run.

### R9. Lifecycle

```
discover -> fetch to quarantine -> verify (hash, signature, key continuity, imports vs manifest)
 -> static checks -> [model audit] -> user approves permissions
 -> install (versioned, content-addressed dir) -> precompile wasm -> index (cards, BM25, embeddings if available)
 -> idle: nothing loaded, nothing running
 -> select (L0/L1/L2) -> load on first call (wasm: instantiate; mcp: start in sandbox + drift check)
 -> call under limits -> result: validate, compress, provenance-tag
 -> unload (wasm: drop store now, drop code after idle; mcp: stop after idle_stop_s)
 -> update (diffs, re-verify, re-audit, atomic switch) | rollback | disable | uninstall (keep or delete tool data)
```

### R10. Order of work

1. With the first real feature work: package format, store, overlay, index (L0-L3), `Tool` trait, built-ins on that trait, in-app guidance editor, selection benchmark.
2. MCP client via rmcp, Deprecated mode, conversion to External (snapshot, pin, lazy start, drift check). Skills import and conversion.
3. Wasm spike (two days), then the wasm runtime, SDK crate and template, install from folder/git, minisign verification.
4. Registry v0 (static index, CI checks), browser and updater UI, audit flow.
5. Later: Sigstore provenance badge, reproducibility badge, Tasks extension, MCP Apps, Skills-over-MCP, code mode.

Until a stage ships, its UI entry is visibly marked "not implemented" (brief rule 0.7).

## Licenses

All recommended linked dependencies are compatible with GPL-3.0-or-later.

| Component | Version checked | License | Use | GPL-3.0-or-later |
|---|---|---|---|---|
| wasmtime, wasmtime-wasi, wasmtime-wasi-http | 50.0.3 / 48.0.10 LTS | Apache-2.0 WITH LLVM-exception | Linked | Yes |
| wit-bindgen | 0.63.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Guest SDK (linked into tools, not into Demido) | Yes |
| rmcp | 3.11.0 | Apache-2.0 (repo in MIT-to-Apache-2.0 transition) | Linked | Yes |
| minisign-verify | 0.3.0 | MIT | Linked | Yes |
| sigstore-verify (optional, later) | 0.16.0 | Apache-2.0 | Linked | Yes |
| landlock (process sandbox, see security report) | 0.4.7 | MIT OR Apache-2.0 | Linked | Yes |
| extism (not recommended) | 1.41.0 | BSD-3-Clause | n/a | Yes |
| rhai / mlua / rquickjs (not recommended now) | 1.27.0 / 0.12.2 / 0.14.0 | MIT OR Apache-2.0 / MIT / MIT | n/a | Yes |
| MCP specification and schema | 2026-07-28 | Apache-2.0 (new) / MIT (legacy), docs CC-BY-4.0 | Protocol implemented | Yes |
| MCP Registry data | preview | CC0 1.0 | Metadata read over API | Yes |
| Agent Skills spec | undated | Code Apache-2.0, docs CC-BY-4.0 | Format implemented | Yes |
| MCP servers the user installs | any | Any, including proprietary | Separate programs, downloaded from their source, talked to over a standard protocol; not linked, not redistributed | Not a combined work |
| Wassette, Zed | n/a | MIT / n/a | Design reference only | n/a |

Two licensing points that need an owner decision are in the questions below (guest SDK license; license policy for wasm tools in the official registry).

## Risks

1. **Wasmtime is a large, fast-moving dependency.** Monthly majors, frequent security patches (two batches in the week before this report, one a Windows filesystem sandbox escape). Mitigation: LTS line, one wrapper crate, fast patch releases, least-privilege preopens. Binary-size and build-time cost are unmeasured.
2. **WASI 0.3 timing.** Starting on 0.2 means a later migration of the world definition. Mitigation: versioned world, host keeps old bindings; 0.2 components keep running on 0.3 runtimes.
3. **No public evidence for small-model tool selection at this design's scale.** The index design is reasoned from frontier-model results and a few edge-model papers. Mitigation: the built-in selection benchmark, conservative k, tool profiles.
4. **Retrieval misses.** A needed tool that is not close to the request text will not be injected. Mitigation: always-on catalog + `tools.find`, task text used as retrieval query, `evals` queries as retrieval text.
5. **Process sandbox coverage for External tools is uneven**, weakest on Windows. An unsandboxed MCP server has the user's full access. Mitigation: explicit "Unsandboxed" badge, stricter approval, prefer remote or wasm.
6. **A pinned package version does not pin its dependency tree** for npm/PyPI launchers. Mitigation: managed install with lockfile hash, no install scripts, OCI digests where offered.
7. **Model audits give false comfort and false alarms**, and can be attacked through the audited content. Mitigation: wording and coverage rules in R8; audit never overrides verification or permissions.
8. **Guidance is an injection channel.** A converted skill or an External tool's descriptions are instructions the model obeys. Mitigation: snapshot + audit + diff on change; third-party guidance treated as lower trust by the anti-injection layer.
9. **Running a registry is ongoing human work.** PR review does not scale for free; an unreviewed registry fills with malware (ClawHub). Mitigation: start small and curated, verified namespaces, CI checks, yank mechanism, clear "unreviewed" labels for anything else.
10. **rmcp churn** (six releases in seven weeks) and an MCP ecosystem mid-migration: many servers still speak 2025-11-25 with sessions. Mitigation: exact pin, wrapper crate, legacy fallback and `state = "session"`.
11. **Key management.** Publishers lose or leak minisign keys. Mitigation: key continuity warnings, registry-side key rotation by signed PR, yank.
12. **Conversion quality with weak local models.** Generated summaries and guidance may be poor. Mitigation: deterministic conversion works without a model; model output is a draft the user sees and can edit.
13. **GPL boundary for wasm tools is a grey area** (loaded in-process, calling host functions). Not legal advice; see questions.

## Open questions for the owner

1. **Labels.** MCP servers and SKILL.md are the ecosystem's current standards (MCP has a formal Skills extension; the skills repo has 27.5k stars). Showing them as "Deprecated" is accurate for Demido's model but may read as "broken". Keep "Deprecated", or use "Classic" / "Unconverted" with the same warning?
2. **Wasm for third-party tools.** Do you accept "Rust compiled to a wasm component" as the meaning of "Tools are built in Rust" for anything installed, with native Rust reserved for built-ins?
3. **Guest SDK license.** The `demido-tool` crate and the WIT files get linked into every tool. If they are GPL-3.0, every tool must be GPL-compatible. Do you want them under MIT OR Apache-2.0 so authors can choose their own license?
4. **Registry license policy.** Should the official registry accept only GPL-compatible open-source wasm/guide tools (my recommendation; Zed requires an approved open-source license), while External MCP servers may carry any license because they are separate programs?
5. **Who runs the official registry?** A repo under your account with you as reviewer and key holder? How much review are you willing to do per submission, and is "unreviewed community index" acceptable as a separate, clearly labelled source?
6. **Unsandboxed External tools on platforms without a process sandbox**: allow with a warning, or block until a sandbox exists?
7. **Auto-update plus audit**: when both are on and the auditor reports nothing, may an update that does not widen permissions install silently? (Recommended: yes, with a notification and rollback.)
8. **Deprecated MCP features**: confirm Demido will not implement Sampling, Roots and Logging.
9. **Remote audit models**: audits can be large. Should a paid remote model be allowed for audits by default once chosen, or ask each time?
10. **Scripts in converted skills**: keep runnable through the shell tool with per-script approval (recommended), or refuse to run scripts until ported?

## Ideas

- **Secrets by reference.** Tools never receive a token; they ask the host to attach secret X to requests for an allowed host. A malicious tool cannot exfiltrate what it never holds. Apply the same to External tools where the server supports header injection.
- **Tool evals as a first-class file.** `evals/selection.jsonl` doubles as retrieval text, as a regression test for the index, and as the per-model benchmark users can run.
- **Tool profiles** on characters, sessions and sub-agents: the cheapest accuracy gain for small models is fewer candidates.
- **Vector routing**: host-side embedding routing of tools is a concrete, low-risk instance of the brief's "vector reasoning instead of language reasoning".
- **Per-model guidance variants**: `GUIDE.md` plus optional `GUIDE.small.md` for models under a size threshold, chosen by the host.
- **"Explain this tool" view**: for any tool show exactly what the model sees at each level (L0 line, schema as rendered for the current model, guidance) with token counts. Full transparency, and it teaches authors to write short.
- **Usage-driven pinning**: suggest pinning functions used in most sessions and collapsing ones never used; always a suggestion the user sees, never silent.
- **Dry-run mode** for `destructive` and `external_write` functions where the tool supports it, surfaced to the permission system.
- **Import `.mcpb` bundles** directly: they already carry a manifest and a mandatory sha256.
- **Wrap, don't port**: a wizard that turns a converted External tool's most-used functions into a wasm tool skeleton calling the same HTTP API, for users who want to drop the Node/Python runtime.
- **Export**: any Tool can be exported back to SKILL.md (guidance) or served as an MCP server by Demido's own API, so users are not locked in.
- **Pushback on "models convert MCPs"**: make conversion a deterministic pipeline that a model only polishes. It then works with no model loaded and gives the same result twice.

## Not verified

- Binary-size and compile-time impact of embedding wasmtime in Demido; real-world size and latency of a non-trivial Rust tool component. Needs the spike.
- Exact release date of wasmtime 48.0.0 (2026-08-20 is the planned date from the release cadence) and therefore the exact LTS end date.
- Whether wasmtime's `p3` host modules are in practice production-ready despite the "experimental" doc text; wasi.dev and docs.rs disagree in tone.
- Token costs quoted for catalog lines and guidance sections are my estimates, not measurements.
- Small-model (under about 14B) tool-selection accuracy under any of the compared index designs: no solid public benchmark found.
- TinyAgent's 0.998 tool-recall figure (secondary summary only). RAG-MCP's models and benchmark details (abstract only). The 2605.26165 preprint is single-author and unreviewed.
- Stabilization status of Cargo `trim-paths` on Rust 1.100.
- Licenses of bubblewrap, Pydantic Monty, and Anthropic `sandbox-runtime` (the last from secondary sources only).
- Which Rust HTTP client crates work unmodified on `wasm32-wasip2` today.
- Details of the October 2026 wasmtime advisories reported by The Hacker News (page not retrievable).
- The MCP client matrix page lists clients but the support marks did not survive fetching; per-client extension support is therefore not confirmed beyond what each extension's own page states.
