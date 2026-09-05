# A skill's tools are one list over three provenances

Decided on wayfinder ticket [#14](https://github.com/elpideus/demido-studio/issues/14).
[`tiles.md`](tiles.md) says a skill is a runtime tile and defers the launching,
supervising and killing of its tools to this file. This is that file.

Brief B20: "Skills system. This is important because my requirements for it are complex."

The brief hands the substrate over by name:

Brief B53:

> The ones without a specific type are assumed to be the ones provided by the skill in an "engine" or "src" folder

## What v2 actually left us

Worth stating, because the ticket that opened this one believed otherwise. v2
recorded `engine: bool` on every scanned skill, set by whether an `engine/`
directory exists, and **read it nowhere**. `demido-skills` says so itself: *"This
crate starts no process."* There was no tool `type` in the manifest, no `prompt`
tools, no per-skill descriptions and no `agent` block. Neither bundled skill
carried an engine. The three-type design is unbuilt.

What v2 did build, and built well, is the MCP half: `mcp.json` parsed, rendered
as a one-line-per-server disclosure, approved by a digest over **what was shown**
rather than over the file, supervised across restarts, and killed as a process
tree in a job object. That last one caught a real leaked `npx` server on Windows.
`demido-tools` puts those behind `find_tools` progressive disclosure.

So this decision is not choosing a substrate from nothing. It is deciding whether
the unbuilt half collapses into the built one.

## The wire is MCP. The author never writes it.

**A skill's `engine/` is an MCP stdio server**, so an untyped tool and an `mcp`
tool are one mechanism with two provenances: one supervisor, one approval path,
one crash boundary, one namespace.

The overhead objection is real but misplaced. A stdio JSON-RPC round trip is
sub-millisecond against a local model turn measured in seconds. The cost that
matters is process startup, and the supervisor already amortises it: started once
per chat, restarted once by the call that finds it dead.

The cost that is real is authorship, and it is paid rather than argued away.
**Demido ships a `demido-skill` Python package, and the author writes decorated
functions.** A tool is a function with a docstring and type annotations; the
schema is derived from the annotations, the server is the package's `main`, and
the author never sees the protocol.

This is not a convenience. The brief's promise is

Brief B52: "skills also provide the required mcps and tools instead of the user having to grab them manually"

and that promise is broken the moment an author hand-rolls JSON-RPC to ship two
functions.

## One managed runtime, and any other by declaration

**`engine/` is Python, run by uv.** `engine/pyproject.toml` declares the
dependencies, `uv run` starts it, uv fetches its own interpreter, and v2's
decision 0033 already fetches uv itself onto a machine that has none. It is the
language a small model writes most reliably and the language every API a skill
wants has a client for.

**Any other language is already supported, by `mcp.json`.** Because the boundary
is the protocol and not the runtime, a skill that wants Node or Go or Rust
declares a server and owns its own process. One well-lit road, no walls.

WASM (Extism) was weighed and rejected. It sandboxes properly and needs no
runtime install, but a skill that cannot open a socket, touch the filesystem or
shell out is not the skill system the brief describes. It was rejected on the
brief, not on taste.

## What the model sees

**One flat list. The model never sees a type.** `type` is host-side provenance
and nothing else.

| `type` | Where the call goes | Costs a process |
|---|---|---|
| absent | The skill's own engine | Yes, one per skill, shared by its tools |
| `mcp` | A server from this skill's `mcp.json` | Yes, the server's |
| `prompt` | Nowhere: the markdown file is the result | No |

A `prompt` tool is the sleeper. Brief B54: "The prompt type means that the tool is nothing more than an md file that gets read and tells LLM what to do on the spot."
It is how a skill delivers guidance at the moment of use rather than as a page
read up front, and it is free.

**Names are `skill__tool`.** v2 used `skill__server__tool`, which produced
`market-analysis__market__market_quote`: thirty-eight characters a 4B model has
to reproduce exactly, most of them carrying nothing. The three-part name existed
to make collisions impossible; a **load-time refusal naming both declarations**
makes them impossible earlier and cheaper. The `find_tools` group is the skill
id.

One consequence for skill authors: the brief's example writes bare tool names in
its prose (`Owns the market_quote, market_history ... tools`). Write the
qualified name in `SKILL.md` and in every description, because that is the name
the model must emit.

### Re-describing an MCP's tool, without drift

The brief asks for this explicitly, and gives the reason: a skill may want only
some of a server's tools, and may need to reword one to fit.

- The **name is the join key**. A skill re-describing a tool its server does not
  offer is a load-time refusal, not a runtime surprise.
- The model is shown **the skill's description**. That is the whole point.
- The server's own description is captured in the approval digest, so a server
  that rewords its tool changes what is disclosed and asks again. Drift surfaces
  as a dialog rather than as a model quietly working from a stale sentence.

### A skill contributes tools. It does not open the host's.

v2's `tools` array listed **host** tool names (`run_command`, `find_tools`). In
v3, `tools` is the skill's own contributions and nothing else. Which host tools
are on the table stays the host's decision, made by `find_tools` against the
context budget. A skill that could pre-open groups would defeat the budget it is
supposed to be saving, which is the same reason the disclosure exists at all.

## Loading discipline, and the two `when`s

v2 implemented the discipline as the brief describes it, and it should carry
forward unchanged. Brief B51: "The file that gets actually loaded (if skill is enabled) is the skill.json file"
A scan reads one small JSON file per folder; a skill costs one line in context;
`SKILL.md` is not read until something asks for it.

**Nothing evaluates `when`, and nothing should.** It is a clause in that one
line, and the model's decision to open the skill is the evaluation. A separate
boolean pass per skill per turn is a second model call with no evidence it beats
the model's own judgement, and a small model is worse at an isolated yes/no than
at picking from a list.

**There are two `when`s, because there are two decisions.** The brief's example
puts `when` inside `agent`, where it means *when to delegate to this skill as a
sub-agent*. v2 hoisted it to mean *when to read this skill*. Those are different
sentences and one cannot serve both, so:

- `when` at the top level: when to read this skill.
- `agent.when`: when to delegate to it. Owned by
  [#24](https://github.com/elpideus/demido-studio/issues/24) and
  [#11](https://github.com/elpideus/demido-studio/issues/11).

On slash commands `when` gates nothing, because the user chose. It is help text
and is documented as help text.

## Slash commands, and what `params` buys

Decided on wayfinder ticket
[#25](https://github.com/elpideus/demido-studio/issues/25).

Brief B59: "There are also the slash commands, that allow users to use features from the skill on demand"

The brief gives a command a `params` array of names, descriptions and a
`required` flag. v2 has none: `invoke::parse` reads `/slug rest-of-line` and
`invoke::expand` puts the whole tail into a single `$ARGUMENTS` placeholder,
appending it when the file has no placeholder. All of that carries forward
unchanged.

**`params` is a legend, not a signature, and nothing splits the typed line.**
The user types whatever they like after `/name`, exactly as in v2, and `params`
exists to say what is worth typing. There is no positional parsing anywhere in
the path. The brief's own example is the reason: one of its three values is
`"since last January"`, one value and three tokens, so a splitter is wrong on
the first command anybody reads about.

### Three readers, and a gate for none of them

| Reader | What it does with `params` |
|---|---|
| The user | A static legend beside the composer: every name at once, in declared order, required ones marked, descriptions on hover. |
| The model | The declarations appended to the expansion, under fixed host text saying what to do about them. |
| The host | Nothing. It never validates, never refuses, never fills a default in. |

The legend **never advances**. Dimming `asset` and highlighting `timeframe`
because a space has been typed means knowing which blank is being filled, which
means splitting the tail, which is the thing that cannot be done. A guide that
is confidently wrong about where you are is worse than one that never claims to
know.

That also answers what the Navigator shows for a command with three parameters:
**one row**, carrying the names as a signature. Nothing is collected, so there is
nothing to expand into.

### `required` is addressed to the model

It is not a UI gate and not a host check. It says the model may not proceed
without a value, and the host says so in fixed words, appended after the
expanded page whenever the array is non-empty:

```text
Parameters for /market_report:
- asset (required): Market asset. Can be FOREX, Stock, Crypto, ...
- timeframe (optional): Time frame on which analysis should be made ...
- duration (required): Duration on which to make analysis ...

Take each value from what the user typed above, or from earlier in this
conversation. Ask the user for any required parameter you cannot find. Do
not guess one.
```

**The sentence is the host's, identical for every command in every skill.**
Phrasing is load-bearing at 4B, and an author made to write the asking rule by
hand writes forty variants of it, the worst of them by somebody who is not a
prompt engineer. It is the move [`done.md`](done.md) and [`tools.md`](tools.md)
both make: the sentence lives in the host, not in the thing being described. A
manifest key to override it is not needed yet, and arrives without a break when
it is.

An **optional** parameter is marked optional and nothing more. A default is a
sentence about the author's own domain, and the prompt file is where the author
already writes those. A `default` key would split that authorship across two
files that can disagree, for nothing the author cannot get by typing one line.

**A command with no `params` expands byte for byte as it did in v2.**

The cost is written down rather than glossed: nothing stops a small model
guessing a duration instead of asking for one, and nothing in v0.1 detects that
it did. The alternative, the host collecting values into a form, cannot be wrong
in that way and was rejected anyway, because it turns a free line into a set of
blanks that the brief's own example does not fit in.

### A command is the user speaking

An expansion enters the transcript as **the user's message**. That is v2's
`Expansion` unchanged, carrying `command` and `skill` so that "which skill put
these words in my conversation" stays answerable. A `prompt` tool returns a tool
result, because the model chose it rather than the user. Same file, different
door, different place in the log.

**Commands are user-only.** A model cannot invoke one, which is why `when` gates
nothing on a command. A page the model should be able to reach is a `prompt`
tool pointing at the same file, taking its gate from the tool list. One file,
two doors.

**Substitution is one function** whichever door the file came through. A command
and a `prompt` tool must never come to disagree about how a value gets into a
page.

## Trust is where a skill came from

Third-party code runs unsandboxed in v0.1, and the honest mechanism is the one
already built: you read the exact command line before it runs, and an edit to it
asks again.

Two things are decided now, because retrofitting either is expensive:

1. **A skill carries an origin**, and origin is the only input to trust:
   bundled, installed by the user from disk, or fetched from a registry. Never
   what the manifest says about itself.
2. **The manifest reserves a `sandbox` key** and refuses it with a clear
   sentence. v2 already proves unknown keys survive a manifest written for a
   later Demido, so the sandbox arrives without a `host_api` major bump. Same
   move `tiles.md` made for the reserved UI key.

## Transport

`type` enters the server schema now, defaulting to `stdio`. An `http` entry
produces a clear refusal naming the server, rather than a skill that installs
cleanly and has no tools for reasons nobody can see: v2 filed `transport` under
`unused` and started nothing, silently.

Streamable HTTP is implemented when a slice needs it. It is not in v0.1's four
slices ([#11](https://github.com/elpideus/demido-studio/issues/11)), and it
reopens the disclosure question, since a URL has no command line to show.

## `skill.json`

```json
{
  "id": "market-analysis",
  "host_api": "1.0",
  "description": "One sentence. Always in context.",
  "when": "when to read this skill",
  "commands": [
    {
      "name": "trend",
      "description": "...",
      "file": "commands/trend.md",
      "params": [{ "name": "asset", "description": "...", "required": true }]
    }
  ],
  "tools": [
    { "name": "market_download", "description": "..." },
    { "type": "mcp", "server": "market", "name": "market_quote", "description": "..." },
    { "type": "prompt", "name": "market_documentation", "description": "...", "file": "tools/market_documentation.md" }
  ],
  "agent": {
    "when": "when to delegate to this skill",
    "models": ["Gemma 4 26B A4B"]
  }
}
```

`file` rather than v2's `prompt`, following the brief. `server` is optional when
`mcp.json` declares exactly one server. Every path is relative and verified to
stay inside the skill's folder, which is v2's rule and its whole traversal
defence.

## What an author puts in `engine/`

```
market-analysis/
  skill.json
  SKILL.md
  mcp.json          optional: third-party servers this skill needs
  commands/         slash command prompts
  tools/            prompt-type tool bodies
  engine/
    pyproject.toml  dependencies, resolved by uv
    main.py         @tool functions
```

`main.py` holds one decorated function per untyped tool in `skill.json`. The
docstring is the fallback description, the annotations are the schema, the return
value is the tool result, and a raised exception is a failed result rather than a
dead server. The working directory is the skill's own folder, so a relative path
in the engine means the same thing wherever Demido was started from.

Nothing else. No server, no protocol, no lifecycle.

## What this does not decide

- The default set of failure classes a skill's engine can raise. That is
  [`lessons.md`](lessons.md).
- Sandbox enforcement, registry distribution, and the disclosure form for a
  remote server. All three wait for skills written by strangers.
