Add slash command to installed skill `$skill_id`.

What command does: $brief

**Read the Skill Manager guide first.** Its `SKILL.md` path is in your context (skills block). This body is sent alone — guide is NOT in your context.

Steps:

1. **Read target skill.** Folder `$skill_id` in skills dir — bracketed path prefix on this prompt names *this* skill's folder; target sits beside it. `read_file` its `skill.json` + `SKILL.md` → match voice, do not restate what `SKILL.md` says.
2. **Command or tool?** Command = user types `/name`. Tool = model calls it unprompted. Brief says "when I ask" / "let me run" → command, continue here. Brief says "so the model can" / "automatically" → stop, use `/add-tool $skill_id` instead.
3. **Design signature.** `name` must not collide inside that skill. `params`: positional in schema order. `required` for values command cannot work without. `rest: true` (last param only) for trailing free text. Declared param over hand-parsing `\$ARGUMENTS`.
4. **Write body** as `commands/<name>.md`, `\$name` placeholders. Body *replaces* user's message → write as direct instruction. Body sent alone → if it needs `$skill_id`'s guide, tell it to `read_file` that `SKILL.md` first. Escape any placeholder body only discusses.
5. **Style: Caveman Ultra.** Match target skill's prose. Fragments, one idea per line, no articles/filler. Code, JSON, tool names, paths, error strings verbatim.
6. **Reinstall whole skill** with `install_skill`: id `$skill_id` + every file — updated `skill.json`, unchanged `SKILL.md`, unchanged `tools.json` if it has one, all pre-existing command + tool body files, new one. Command whose file missing from payload → rejected.
7. **Report** new usage line, e.g. `/name <required> [optional...]`.
