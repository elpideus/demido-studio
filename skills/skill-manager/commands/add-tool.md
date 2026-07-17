Add tool to installed skill `$skill_id`.

What tool does: $brief

**Read the Skill Manager guide first.** Its `SKILL.md` path is in your context (skills block). This body is sent alone — guide is NOT in your context. Read its `tools.json` section before designing.

Steps:

1. **Read target skill.** Folder `$skill_id` in skills dir — bracketed path prefix on this prompt names *this* skill's folder; target sits beside it. `read_file` its `skill.json`, `SKILL.md`, and `tools.json` if present. Match voice. Tool must not duplicate what a command already does.
2. **Tool or command?** Tool = model calls it on its own, from `description` alone. Brief says "when I type" / "let me run" → wrong shape, use `/add-command $skill_id` instead.
3. **Pick type.**
   - Instructions the model should follow → `type: "prompt"`. Returns body text. Executes nothing.
   - Live data or an external system, already has an MCP server → `type: "mcp"`.
   - Needs to run code and no MCP server exists → **not a tool.** Prompt tool whose body tells model to `run_command` the script, or a plain command. Say so, do not fake it.
4. **Design.**
   - prompt: `name`, `description`, `file` (body under `tools/`) or inline `prompt`, `params` — named args, **no `rest`**, `required` for what it cannot run without. Wire name is `skill_<skill id>_<tool name>` → keep under 64 chars, letters/digits/`_`/`-` only.
   - mcp: `name`, `command`, `args`, `env` as the server documents. `bypassAgentMode` only if tools must work in agent mode Off — default off.
   - `description` is the sole trigger. When to use + what it does. Caveman Ultra, one line.
5. **Write body** for a prompt tool as `tools/<name>.md`, `\$name` placeholders. Sent alone → needs the guide, tell it to `read_file` `$skill_id`'s `SKILL.md` first. Escape any placeholder body only discusses.
6. **Merge `tools.json`.** Existing file → add entry, keep every existing one. No file → create `{"tools": [ ... ]}`. Names unique across both types. Empty `tools` array → rejected.
7. **Reinstall whole skill** with `install_skill`: id `$skill_id` + every file — `skill.json`, `SKILL.md`, merged `tools.json`, every pre-existing command + tool body, new body. File missing from payload → rejected.
8. **mcp entry → install prompts the user**, showing the command line it will run, in every agent mode. Expect it. Tell user up front. Refused → nothing installed; do not retry around it.
9. **Report** tool name as the model will see it, what triggers it, and for an mcp entry what process now runs while the skill is enabled.
