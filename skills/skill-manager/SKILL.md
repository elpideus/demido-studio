# Manage Demido Studio skills

You = LLM inside Demido Studio. This = on-disk shape, slash-command system, tool system, porting from other harnesses, mistakes that make skill install but never load.

Two jobs, one guide:

- **Author** — new skill, new command, new tool, edit existing. Sections: *House style* → *Validation rejects*.
- **Port** — skill written for another harness → re-express as Demido skill without changing what it does. Read *Porting* section too. It has its own prime rule.

## House style: Caveman Ultra

Every skill you author or port writes its prose in **Caveman Ultra**. Same style as this file. Not optional — it is the format.

Why: `skill.json` `description` costs tokens on every message the skill is enabled. `SKILL.md` + command bodies + tool bodies cost tokens each time model reads them. Compression = more skills affordable at once.

Rules:

- Drop articles (a/an/the), filler, pleasantries, hedging, conjunctions.
- Fragments only. One idea per line. Lists over prose. Arrows for causality (X → Y).
- Abbreviate prose words: DB, auth, config, req, res, fn, impl. Prose words only.
- **Never compress**: code blocks, `skill.json` itself, `tools.json` itself, JSON keys, tool names, fn names, API names, CLI commands, file paths, error strings, placeholder syntax, trigger words a source `description` relies on. Verbatim always.
- Never drop a fact to hit brevity. Compression makes meaning ambiguous → write it long.
- Write normal prose for: security warnings; irreversible-action confirmation; multi-step order where fragment order risks misread.

Caveman Ultra = how the skill's *text* reads. Not what skill *tells model to output* — skill that formats reports for humans still specifies whatever format that skill needs. Source that specifies output format keeps that format exactly.

Style defined in app: `src-tauri/src/caveman.rs`, level `ultra`. User picks per-conversation in chat top bar. Skill text style is independent of that dropdown — do not tell user to enable caveman for your skill to work.

## What skill is

Folder in app-data skills dir:

- Windows: `%APPDATA%\studio.demido.app\skills\<id>\`
- macOS: `~/Library/Application Support/studio.demido.app/skills/<id>/`
- Linux: `~/.local/share/studio.demido.app/skills/<id>/`

```
<id>/
  skill.json      metadata + slash-command declarations   (required)
  SKILL.md        the guide — model reads on demand        (required)
  tools.json      tools this skill gives the model         (optional)
  commands/*.md   one prompt body per slash command        (optional)
  tools/*.md      one prompt body per prompt tool          (optional)
  reference/*.md  files a body tells model to read         (optional)
```

## How skill reaches model — read before designing

Four channels. Pick wrong → skill never fires.

1. **`skill.json` → always-on.** Skill enabled in **Tools → Skills** → app injects raw `skill.json` + absolute paths of every other file in folder. That is all. **`SKILL.md` body is NOT injected. `tools.json` is NOT injected.**
2. **`SKILL.md` + `reference/*` → on demand.** Model reads them with `read_file`, using injected absolute paths, only if `description` convinced it skill applies.
3. **Command body → on demand.** Costs nothing until user types `/name`. On send, body *replaces* user's typed text as the message.
4. **`tools.json` tools → offered while skill enabled.** Model sees each tool's name + description in its tool list, calls when it decides. Skill disabled → tools not offered at all.

Consequences — all bite:

- **`description` is sole trigger for the *skill*.** Nothing else in context attracts model. Weak description → skill invisible forever, however good `SKILL.md` is. Write description as *when to use + what it does*, not what it is.
- **Tool `description` is sole trigger for the *tool*.** Same rule, separate field. Model picks tool from that line alone.
- **`SKILL.md` can be long.** Not billed per turn. Put full detail there. Do not cram knowledge into `description` to dodge a read.
- **Command body + tool body inherit nothing.** Sent alone; `SKILL.md` not in context. Body needs guide knowledge → body must say `read SKILL.md at the path in your context first`. Never write "follow the guide already in your context" — it is not.

Enabled toggle gates all: disabled skill hides its commands and withholds its tools.

## skill.json

```json
{
  "id": "my-skill",
  "name": "My Skill",
  "description": "One line shown in Tools → Skills. Also the sole trigger the model ever sees.",
  "version": "1.0.0",
  "commands": []
}
```

- `id` **must equal folder name.** Mismatch → rejected at install.
- All five top-level keys required. `commands` may be `[]`.
- **Invalid JSON → app silently ignores skill.** No error. Trailing comma = skill lost.
- `description` in Caveman Ultra, one line. Front-load trigger words user would type.
- Tools do **not** live here. Separate `tools.json` — below.

## Commands

```json
{
  "name": "review",
  "description": "Review a file for correctness",
  "file": "commands/review.md",
  "params": [
    { "name": "path", "description": "file to review", "required": true },
    { "name": "focus", "description": "what to look for", "rest": true }
  ]
}
```

| Field | Meaning |
|---|---|
| `name` | what user types after `/`. Kebab-case. |
| `description` | shown in `/` popup, filters it. |
| `file` | path to prompt body, relative to skill folder. |
| `prompt` | inline body, one-liners. `file` wins if both set. |
| `params` | optional param schema — below. |

Exactly one of `file` / `prompt` must yield non-empty body. Declared `file` not in payload → install rejects. Command with neither key → install rejects. `params` schema is not a body. Do not declare command before writing its body.

Name collision across two enabled skills → both auto-qualify to `/<skill-id>:<name>`. Unique names stay bare. Do not rename to dodge — handled.

### Parameters

Args typed after command bind to `params` **positionally, in schema order**, then substitute into body.

| Placeholder | Resolves to |
|---|---|
| `$ARGUMENTS` | entire raw argument string |
| `$1` … `$9` | Nth token |
| `$name` | param declared `name` in `params` |

Tokenise = whitespace-separated. Double quotes keep token whole:
`/review "src/my file.rs" perf` → `$1` = `src/my file.rs`, `$2` = `perf`.

Flags:

- `"required": true` → invoking without it fails with usage hint, no half-filled prompt sent. Use when command meaningless without value.
- `"rest": true` → swallows every remaining token → free text with spaces needs no quotes. Must be **last** param. Enforced at install.

`/` popup renders call shape from schema: `/review <path> [focus...]`. Angle = required, square = optional, `...` = rest.

**Fallback:** body substitutes no placeholder at all → args appended to end of body as new paragraph. Body needs value elsewhere than end → body must use placeholder.

**Escaping:** backslash makes placeholder literal — `\$1` renders `$1`. Needed only in bodies that *discuss* placeholders (like this skill's own commands). Undeclared `$word` left alone already → `$5 per month` safe. `SKILL.md` never expanded → no escaping there.

### Writing command body

Body becomes user's message verbatim. Write as instruction to yourself. Caveman Ultra.

```md
Read the Skill Manager guide first — its SKILL.md path is in your context. This body is sent alone; the guide is not in your context.

Review file at `$path`. Focus: $focus.

1. `read_file` it.
2. Report issues as `path:line — problem. fix.`
3. Do not restructure code that is merely unfamiliar.
```

Two facts decide body design:

- **Install path injected automatically** as bracketed prefix before body → relative path like `reference/rules.md` resolves. Still name the file explicitly.
- **Filesystem tools except reads are gated on working folder** set for conversation. Body writes files → say so up front, check working folder first, stop with clear message rather than loop.

## tools.json

Optional. Tools the **model** calls. Command = user types it. Tool = model decides.

```json
{
  "tools": [
    {
      "type": "prompt",
      "name": "review_diff",
      "description": "Review a diff for correctness bugs. Use when user asks for review of changed code.",
      "file": "tools/review-diff.md",
      "params": [
        { "name": "path", "description": "file to review", "required": true }
      ]
    },
    {
      "type": "mcp",
      "name": "warframe",
      "description": "Warframe Market price data",
      "command": "npx",
      "args": ["-y", "warframe-market-mcp"],
      "env": { "WF_LOCALE": "en" },
      "bypassAgentMode": false
    },
    {
      "type": "builtin",
      "name": "install_skill",
      "description": "shown in Tools popup; model always sees the real description"
    }
  ]
}
```

- File optional. Skill with no tools ships no `tools.json`.
- `tools` may not be empty — omit file instead. Empty array → install rejects.
- Unknown `type` → install rejects. Never silently ignored. Three types exist: `prompt`, `mcp`, `builtin`.
- Names unique across **both** types within a skill. Duplicate → install rejects.
- Names + skill `id` must use only letters, digits, `_`, `-`. Providers reject anything else in a tool name. Skill id otherwise free-form → skill declaring tools is held to stricter rule.
- Invalid JSON → install rejects. Hand-edited unparseable file → app logs, offers no tools.

### type: prompt

Model calls it → app reads body → substitutes args → **returns body text as tool result**. Nothing executes.

| Field | Meaning |
|---|---|
| `type` | `"prompt"` |
| `name` | tool name. Offered as `skill_<skill id>_<name>`. |
| `description` | sole trigger. When to use + what it does. |
| `file` | path to body, relative to skill folder. |
| `prompt` | inline body. `file` wins if both set. |
| `params` | same schema as command params, minus `rest`. |

Rules:

- Offered as `skill_<skill id>_<name>`. Wire name over 64 chars → install rejects. Shorten tool name or skill id.
- **No `rest` param.** Tool args arrive named, not as tokens — nothing to swallow. `rest: true` → install rejects.
- `$name` substitutes declared param. `$1`..`$9` + `$ARGUMENTS` resolve in **param declaration order**, not typed-token order. Body substitutes nothing → arg values appended.
- Body gets same bracketed install-path prefix as command body.
- Prompt tool **executes nothing**. Body needing shell/file → body says so, model calls `run_command` / `read_file`, which carry their own gates.
- Why it exists beyond commands: agent mode **Off** withholds `read_file` → enabled skill's `SKILL.md` unreachable → prompt tool is the only delivery path for that knowledge.

### type: mcp

One MCP server. Spawned while skill enabled, killed when disabled or deleted. Its tools appear under the skill in Tools popup. Server reports its own tools — you do not list them here.

| Field | Meaning |
|---|---|
| `type` | `"mcp"` |
| `name` | server name. Server id becomes `skill:<skill id>:<name>`. |
| `command` | binary to spawn. Required, non-empty. |
| `args` | array of args. Optional. |
| `env` | env vars map. Optional. |
| `description` | shown in popup. Optional — server's tools carry their own. |
| `bypassAgentMode` | tools skip agent-mode permission gate. Default `false`. |

Rules:

- `command` empty → install rejects. Nothing to spawn.
- Default **gated**: server's tools obey agent mode like builtins. `bypassAgentMode: true` → they skip it. Ask only when tools genuinely must work in Off mode.
- Hand-configured servers (Settings → MCP) are ungated always. Skill servers default closed because a skill's `tools.json` can be model-written.

**Declaring an mcp entry makes `install_skill` ask the user first.** The prompt shows the command line it will run. This is deliberate and it is the whole safety story: a prompt tool is inert text, but an MCP server is a spawned process, so the user — not the model — decides it runs. Never add an mcp entry as a way to run a command. A skill that needs a shell tells the model to call `run_command`, which is gated properly.

### type: builtin

Surfaces a tool the **app already implements**. Skill does not implement it — skill decides the model sees it. Offered under its real name, unprefixed, while skill enabled. Skill's toggle is its switch.

| Field | Meaning |
|---|---|
| `type` | `"builtin"` |
| `name` | must be in the allowlist below. |
| `description` | overrides popup text only. Model always sees the real description — skill may not reword what a tool claims to do. |

Allowlist — **only these two**:

- `install_skill`
- `delete_skill`

`skill-manager` already claims both. **Do not claim them in another skill** — two skills surfacing one tool means whichever is enabled offers it, and the popup shows it twice.

Any other name → install rejects. Allowlist is the boundary: `run_command`, `write_file`, `read_file` and the rest are gated on agent mode, and a skill (which a model can author) must never hand itself one. Everything claimable is already offered in every mode → claiming grants no new permission, only moves which toggle controls it.

**Consequence:** `skill-manager` disabled or deleted → model cannot install or delete skills at all. That is the trade for tools living under the skill that brings them.

## Installing

Use **`install_skill` tool**. Writes whole folder atomically, validates invariants, needs no working folder, offered in all agent modes incl. Off. Can only write inside Demido's own skills dir. Author freely: install, read back, fix, reinstall.

Prompting:

- Payload with no mcp entry → never prompts. Install, read back, fix, reinstall freely.
- Payload whose `tools.json` declares an mcp entry → **always prompts**, every mode. Expect it. Unparseable payload also prompts — app cannot clear what it cannot read.

Do **not** hand-write skill files with `write_file` — gated, and skips validation.

Pass `id` + every file at path relative to skill folder (`skill.json`, `SKILL.md`, `tools.json`, `commands/x.md`, `tools/y.md`). Replacing existing skill leaves user-edited extra files alone.

`install_skill` not in tool list → stop, say so plainly. Do not improvise another tool. Do not restate plan — neither installs anything.

Change a skill → reinstall over it. `install_skill` is the edit path. No partial-edit tool for skills. `edit_file` is gated on working folder.

Skill live immediately — no restart, no import. Tell user to enable in **Tools → Skills**.

## Deleting

`delete_skill` (id only) removes skill folder + everything in it.

It **always asks user first**, every mode incl. autonomous. It is `remove_dir_all`, no undo, on a folder that may hold hand-written user files. Expect the prompt. Do not retry around a refusal.

Call it only when removal is what was actually asked. Never as "clean slate" before install — installing over a skill already replaces it.

Never delete the skill you are running. `skill-manager` = tool doing the work, not thing being worked on. Its id appears in bracketed install-path prefix at top of your prompt — that line says where *this guide* lives, so relative paths resolve. Not a target.

## Validation rejects, in order

1. missing `skill.json`
2. missing `SKILL.md`
3. any path containing `..` or starting `/`
4. invalid JSON in `skill.json`
5. `id` ≠ folder name
6. bad or non-final command param name
7. command whose `file` wasn't provided
8. command with no body at all
9. invalid JSON in `tools.json`, or entry with unknown `type`
10. `tools.json` with empty `tools`
11. skill id not tool-name-safe, when skill declares tools
12. tool name not tool-name-safe
13. prompt tool wire name over 64 chars
14. bad tool param name, or `rest: true` on tool param
15. prompt tool whose `file` wasn't provided
16. prompt tool with no body at all
17. mcp entry with no name, or no command
18. builtin entry naming a tool outside the allowlist
19. duplicate name within `tools.json`

## Checklist before install

- [ ] `skill.json` parses. `id` == folder name.
- [ ] `description` is the trigger: when to use + what it does. Front-loaded. Caveman Ultra. Nothing else reaches model unprompted.
- [ ] `SKILL.md` exists. Holds full durable knowledge — not billed per turn, so detail is free.
- [ ] Every command body + tool body needing the guide says to `read_file` `SKILL.md` first. Body inherits nothing.
- [ ] Every command `file` + tool `file` in payload.
- [ ] Every `$name` in a body is a declared param. Every declared param used or deliberately optional.
- [ ] Command `rest` param last. Tool params have no `rest`. Values a body can't work without are `required`.
- [ ] Placeholders a body only *talks about* are escaped.
- [ ] `tools.json` only if skill has tools. Every entry has a `type`. Names unique across types.
- [ ] mcp entry only when skill genuinely needs a server. `bypassAgentMode` only when tools must work in Off mode. User will be prompted — that is expected, not a bug.
- [ ] All prose Caveman Ultra. Code, JSON, tool names, paths, error strings verbatim.

---

# Porting

Take skill written for another agent harness → re-express as Demido Studio skill **without changing what it does**.

## Prime rule: every instruction survives, wording compresses

Source is spec — for **content**. Not for wording.

- **Preserve**: every fact, rule, step, constraint, example, trigger word, exception, format spec. All of it. Nothing dropped, nothing added, nothing "improved". Do not add steps author didn't write. Do not remove one you think is redundant.
- **Compress**: prose wording → Caveman Ultra. Translation, not rewrite. Meaning identical, words fewer.

Test after each block: could a model follow ported text and do exactly what source text made it do? No → you compressed too far. Restore words.

Facts do not compress. Only filler dies. Passage cannot survive compression without losing meaning → **keep it long**. Say so in report.

**Port you didn't read = fabrication.** Every line installed must come from file you actually read in this conversation. `list_dir` or `read_file` failed, or you skipped them → you have nothing to convert. Stop, say so. Do not install plausible-looking skill assembled from folder name + guesses — worse than no port, because it looks finished. Skill named `caveman` whose `SKILL.md` reads "This skill contains a caveman test" = that failure. Placeholder content, invented commands like `/test`, "a skill for X" descriptions = same bug.

Compression example. Source:

```md
When you are reviewing the file, you should first read it completely, and then
you should report any issues that you find, using the format shown below.
```

Ported:

```md
Review file → read whole file first → report issues in format below.
```

Counter-example — do NOT do this. Source: "Report issues as `path:line — problem. fix.` Do not restructure code that is merely unfamiliar." → dropping second sentence because it "reads like filler" = lost instruction = failed port.

## Conversion never destroys anything

Three hard rules. Not style advice — breaking one loses user's files.

1. **Source is read-only.** Belongs to another harness; user still uses it there. `read_file` / `list_dir` and nothing else. Never `write_file`, `edit_file`, `run_command` against source path. Never "tidy it up" as part of port.
2. **Output goes only where `install_skill` puts it** — Demido's own app-data skills folder. Sole write path in a conversion. You never choose output dir. You never write converted skill next to source.
3. **Never call `delete_skill` during a conversion.** Nothing about porting requires removing one. `install_skill` already replaces skill of same id → no "clean slate" step exists. Two ways this goes wrong:
   - **Do not delete the skill you are running** — `skill-manager`. New skill's id comes from **source** folder name.
   - **Do not delete the old copy of the skill you are porting**, if already installed. Reinstalling over it is the update path.

Believe something must be deleted → stop, ask user in plain words. `delete_skill` is `remove_dir_all`, no undo. It will prompt you. That prompt is the last line of defence, not a formality to click through.

## Recognise the source

Do not assume from folder name. Look at what is there → map by structure.

| Signal on disk | Source format | Reading |
|---|---|---|
| `SKILL.md` with YAML frontmatter (`name`, `description`, often `allowed-tools`) | Claude Code / Agent Skills | body = always-on text; frontmatter = metadata |
| same, **no `commands/` folder at all** | Claude Code, skill-as-command | still invocable `/<name> [args]` — see below |
| `.claude/commands/*.md`, or `commands/*.md` with frontmatter | Claude Code slash commands | one file = one command |
| `.agent/` or `.agp/` tree with `workflows/`, `rules/` | Antigravity | `workflows/*` = commands; `rules/*` = always-on |
| `.opencode/command/*.md`, `command/*.md`, `opencode.json` | OpenCode | `command/*` = commands; agent/rule files = always-on |
| `.cursor/rules/*.mdc` with `description` / `globs` / `alwaysApply` | Cursor | `alwaysApply: true` → always-on; else describe trigger in prose |
| `AGENTS.md`, `CLAUDE.md`, `.github/copilot-instructions.md`, bare `*.md` | plain instruction doc | all always-on; no commands unless doc defines them |
| `plugin.json` / `marketplace.json` alongside | plugin bundle | port skill inside it, not bundle |
| `mcp.json`, `.mcp.json`, `mcpServers` key in any config | MCP server config | → `tools.json` mcp entry. See mapping table. |

Unknown layout → fall back to structural question: **"is this text meant to be true all the time, or run once on request?"** Always-true → `SKILL.md`. Run-once → command. That question resolves most formats you have never seen.

## Skill that is itself a command

Commonest shape. Most often botched.

Claude Code skill folder with nothing but `SKILL.md` (+ `README.md`, maybe `reference/`) is **not** a commandless skill. In Claude Code the skill *is* a slash command: user types `/<folder-name> <args>`, args arrive as context alongside body. That is how `/caveman ultra`, `/graphify .` work.

**Demido has no implicit command.** `commands[]` == `[]` → no `/` command exists. Full stop. Port must *materialise* implicit command explicitly, else skill loses entire invocation surface → toggle user can never steer.

Pattern:

1. `SKILL.md` keeps body — every instruction, compressed to Caveman Ultra. Still the on-demand knowledge.
2. Add one command named after skill + `params` schema for whatever args meant. Read body to find out: line like ``Switch: `/caveman lite|full|ultra` `` *is* the arg spec, though no frontmatter declares it. Enumerated set → one param. Free text → one `rest` param.
3. Body is short. Job = *apply* what `SKILL.md` says. Knowledge must not be duplicated into it — but body is sent alone, so it must tell model to read `SKILL.md` first. Like:

   ```md
   Read this skill's SKILL.md first — path is in your context; this body is sent alone.

   Activate caveman style at level `$level` for rest of conversation.

   Apply `$level` from now on, incl. Auto-Clarity exceptions. Acknowledge in one short line, then answer whatever comes next in that style.
   ```

4. Default: source says a level is default → param **not** `required`, and body must say what happens when absent.

Same reasoning for any source whose invocation carries an arg frontmatter never declared. Body's own prose is the spec. Read it before designing schema.

## Mapping table

| Source concept | Demido Studio |
|---|---|
| frontmatter `name` | `skill.json` `name` (Title Case); `id` = kebab-case folder name |
| frontmatter `description` | `skill.json` `description` — **keep trigger wording**; only this reaches model unprompted. Compress filler, never the trigger words |
| frontmatter `version` | `version`, else `"1.0.0"` |
| skill body prose | `SKILL.md`, minus frontmatter block, compressed to Caveman Ultra, every instruction intact |
| one command/workflow file | one `commands/<name>.md` + one entry in `commands[]` |
| `argument-hint: <path> [focus]` | `params` schema: `path` required, `focus` optional |
| `$ARGUMENTS`, `$1`..`$9` | identical — carry through unchanged |
| `{{args}}`, `{{input}}`, `$INPUT`, `%1`, other dialects | rewrite to `$ARGUMENTS` / `$1` / `$name` |
| `!`shell` ` prefix, `@file` inlining, other pre-execution syntax | **not supported** — rewrite as instruction to call `run_command` / `read_file`, flag in report |
| `mcpServers` / `mcp.json` entry the skill depends on | `tools.json` `{"type":"mcp"}` entry — same `command`, `args`, `env`. Tell user install will prompt |
| tool the source's skill defines for model to call | `tools.json` `{"type":"prompt"}` entry, if it is instructions. Real code → not portable; report it |
| `allowed-tools`, `model`, `disable-model-invocation` | **no equivalent** — drop, report. Demido gates tools by agent mode, not per skill |
| `references/` or `assets/` file the body reads | keep same relative path, ship in payload |
| bundled scripts (`.py`, `.sh`) | ship them; body must tell model to run via `run_command` (PowerShell) → needs working folder |

## Rewrites you make anyway

Behaviour-preserving = plumbing, not content. Do them silently.

- **Compress prose to Caveman Ultra.** Every ported body + `SKILL.md`. Facts intact.
- **Strip YAML frontmatter** from every ported body. Demido reads metadata from `skill.json` only; leftover `---` block gets fed to model as prose.
- **Tool names.** Source may name foreign tool. Demido builtins: `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `run_command` (PowerShell), plus web, Google, skills tools. `Read`→`read_file`, `Bash`→`run_command`, `Grep`/`Glob`→`search_files` / `list_dir`, etc. Tool with no counterpart → leave instruction, drop tool name, report it.
- **Shell dialect.** `run_command` is PowerShell. Body hard-coding `bash`/`sh` syntax will fail. Port commands, or tell model to shell out via `bash -c`.
- **Escape placeholders a body only discusses.** Body that *talks about* `$1` (meta-skill, guide) must write `\$1`, else expansion eats it. `SKILL.md` never expanded → no escaping there.
- **Add the read-the-guide line** to any ported command body that assumed skill body was in context. In source harness it often was. In Demido it never is.
- **Command names collide** across enabled skills → Demido auto-qualifies both to `/<skill-id>:<name>`. Do not rename to dodge — handled.

## What can't come across

Say these in report. Do not fake them.

- **Auto-invocation.** Claude Code / Cursor trigger skill or rule by description match or glob. Demido has three triggers: enabled toggle (puts `skill.json` in context), user typing `/name`, model calling a `tools.json` tool. Nothing fires on a glob. Rule that fired on `*.ts` → becomes `SKILL.md` text saying when it applies, or a command user runs, or a prompt tool whose `description` says when to use it.
- **Per-skill tool restrictions**, model pinning, hooks, subagent spawning.
- **Nested/multi-level skills.** Flatten to one folder, or port as separate skills.
- **Source tool backed by real code** (source ships a binary/script implementing a tool). Demido prompt tool returns text only. Ship script + tell model to `run_command` it, and report the change.

MCP servers a source skill declares **do** come across now → `tools.json` mcp entry. Older guidance said otherwise.

Fidelity beats completeness: skill landing with three of four commands + honest report beats one with fourth command that silently does nothing.

## Install result

Use **`install_skill` tool** — never `write_file`. Validates, writes atomically, needs no working folder, offered in every agent mode. Pass `id` + every file at path relative to skill folder. Installing over existing skill replaces it → iterate freely: install, read back, fix, reinstall.

Port carrying an mcp entry → install prompts, showing command line. Tell user it will, and why: source declared that server; Demido makes them approve the process.

Id = **source** skill's folder name, kebab-cased. Two guards: never `skill-manager` (that is this skill — you would overwrite manager with its own output); *different* skill of that id already installed → say so, confirm before replacing. Re-running conversion, overwriting your own previous attempt → no confirmation needed.

Asymmetry: *reading source* = normal `read_file` / `list_dir`, gated on conversation's working folder. Installing = not gated. → check working folder before reading. Not set → stop with clear message; user can also paste source text directly.

## Checklist before install — port

- [ ] You actually read every source file. Nothing installed invented, placeholder, or guessed.
- [ ] Source classified from structure, not name.
- [ ] Source with no `commands/` folder → implicit `/<name> [args]` command materialised.
- [ ] Every command has body — `file` in payload, or non-empty `prompt`. Install rejects neither. `params` schema is not a body.
- [ ] **Every instruction from source survives.** Compressed wording, zero lost facts. Re-read source next to your port and diff the *claims*, not the words.
- [ ] Nothing added that source didn't say.
- [ ] All prose Caveman Ultra. Code, JSON, tool names, paths, error strings, trigger words verbatim.
- [ ] Passage that compression made ambiguous → left long, flagged in report.
- [ ] Always-on vs on-demand split matches what source did, not what reads tidier.
- [ ] `description` keeps source trigger wording — sole trigger in Demido.
- [ ] Command bodies that need guide say to `read_file` `SKILL.md`.
- [ ] Frontmatter stripped from every body; metadata → `skill.json`.
- [ ] Arg syntax translated; `params` schema matches source's argument hint.
- [ ] Foreign tool names mapped; shell snippets PowerShell.
- [ ] Source MCP config → `tools.json` mcp entry, same command/args/env. User warned about install prompt.
- [ ] Every declared command `file` in payload; every `reference/` file bodies read too.
- [ ] Report lists what dropped, what rewritten, what now needs manual trigger.
