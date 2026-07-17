Author new Demido Studio skill, id `$id`, install it.

What it does: $brief

**Read the Skill Manager guide first.** Its `SKILL.md` path is in your context (skills block). This body is sent alone — guide is NOT in your context. Do not proceed from memory.

Steps:

1. **Write `description` first.** Sole trigger the model ever sees — only `skill.json` is injected into context, never `SKILL.md`, never `tools.json`. Weak description → skill never fires. Say when to use + what it does. Front-load words user would type. Caveman Ultra, one line.
2. **Split.** Durable knowledge → `SKILL.md`; read on demand, not billed per turn, so detail is free. One-shot procedures user runs → command bodies under `commands/`. Capability model should reach for on its own → `tools.json`. Brief purely procedural → keep `SKILL.md` short, work goes in a command.
3. **Design commands.** Each: `name`, `description`, args?. Declare `params` schema — do not hand-parse `\$ARGUMENTS`. `required` when command meaningless without it. `rest` (last only) for trailing free text. Use `\$name` placeholders in body.
4. **Design tools, only if brief needs them.** `tools.json`, each entry typed. `type: "prompt"` = body returned to model as text, params named, **no `rest`**. `type: "mcp"` = server spawned while skill enabled; `command` required; **install will prompt the user, showing the command line** — say so in your report. Default `bypassAgentMode` off. No tools needed → ship no `tools.json`.
5. **Every command body + tool body needing guide knowledge must say to `read_file` that skill's `SKILL.md` first.** Body inherits nothing.
6. **Write files.** `skill.json` (id == `$id`), `SKILL.md`, `tools.json` if any, one file per command, one per prompt tool.
7. **Style: Caveman Ultra, all prose.** Fragments. One idea per line. No articles/filler/hedging. Arrows for causality. Code blocks, JSON, tool names, paths, error strings: verbatim, never compressed. Never drop a fact for brevity — ambiguous → write it long. Normal prose for security warnings + irreversible-action confirmations.
8. **Install** with `install_skill` — id `$id` + every file at relative path. Not `write_file`; it skips validation.
9. **Report** commands created with usage lines, tools created with what triggers them. Tell user to enable in Tools → Skills.

Ask before inventing scope brief did not ask for. Small skill that triggers reliably beats large one that does not.
