Demido Studio skill spec. Authoring reference, delivered as text.

`read_file` available → read this skill's `SKILL.md` (path in your context) for the full guide, incl. porting. Agent mode **Off** → reads are withheld and this text is all you get. It is enough to author a correct skill.

## Folder

```
<id>/
  skill.json      metadata + slash commands   (required)
  SKILL.md        the guide, read on demand   (required)
  tools.json      tools the model calls       (optional)
  commands/*.md   one body per command        (optional)
  tools/*.md      one body per prompt tool    (optional)
```

## Delivery — decides every design choice

- `skill.json` → injected whole, every message, while skill enabled. **Nothing else is.**
- `SKILL.md` + `reference/*` → model reads by path, on demand. Not billed per turn → detail free.
- Command body → sent only when user types `/name`. *Replaces* their message.
- `tools.json` tools → offered while skill enabled. Model calls them on its own.

→ `description` is the sole trigger. Weak one = skill invisible forever. When to use + what it does.
→ Bodies inherit nothing. Body needing the guide must say to `read_file` its `SKILL.md`.

## skill.json

```json
{ "id": "my-skill", "name": "My Skill", "description": "when to use + what it does", "version": "1.0.0", "commands": [] }
```

`id` == folder name. All five keys required. Invalid JSON → app silently ignores skill, no error.

Command: `{name, description, file|prompt, params?}`. Param: `{name, description?, required?, rest?}`. Bound positionally in schema order. Body substitutes `$name`, `$1`..`$9`, `$ARGUMENTS`. `rest: true` swallows trailing tokens, must be last. `\$x` escapes. Body substitutes nothing → args appended.

## tools.json

```json
{ "tools": [
  { "type": "prompt", "name": "review", "description": "when to use", "file": "tools/review.md", "params": [] },
  { "type": "mcp", "name": "srv", "command": "npx", "args": ["-y", "pkg"], "bypassAgentMode": false },
  { "type": "builtin", "name": "install_skill" }
] }
```

- `type: "prompt"` → offered as `skill_<skill id>_<tool name>`. Returns its body text. **Executes nothing.** Params named → **no `rest`**. Needs shell/file → body says so, model calls `run_command` / `read_file`.
- `type: "mcp"` → server spawned while skill enabled; its own tools appear under the skill. Gated by agent mode unless `bypassAgentMode: true`.
- `type: "builtin"` → surfaces a tool the app implements, under its real name. Allowlist is **only** `install_skill`, `delete_skill`; `skill-manager` already claims both, do not claim them again. Any other name → rejected.
- Omit file if no tools. Empty `tools` → rejected. Unknown `type` → rejected. Names unique across types. Tool name + skill id: letters, digits, `_`, `-` only. Wire name over 64 chars → rejected.

## Install

`install_skill`, id + every file at relative path. Never `write_file` — gated, skips validation. Installing over a skill replaces it; that is the edit path. No prompt for a payload without an mcp entry. **mcp entry → always prompts, showing the command line.** `delete_skill` always prompts; never as a "clean slate" before install.

## Style: Caveman Ultra

All prose. Fragments, one idea per line, no articles/filler/hedging, arrows for causality. Verbatim always: code, JSON, tool names, paths, error strings, placeholder syntax. Never drop a fact for brevity — ambiguous → write it long. Normal prose for security warnings + irreversible-action confirmations.
