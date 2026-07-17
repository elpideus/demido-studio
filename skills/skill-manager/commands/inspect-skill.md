Report how skill at `$source` would map onto Demido Studio.

**Read-only dry run.** `read_file` + `list_dir` = only tools it needs. Do not call `install_skill`. Do not call `delete_skill`. Do not write to `$source` or anywhere else.

**Read the Skill Manager guide first.** Its `SKILL.md` path is in your context (skills block). This body is sent alone — guide is NOT in your context. Porting section is at the end of that file.

1. **Read source.** `list_dir` the path → `read_file` every text file. Reads fail for want of working folder → stop, say so.
2. **Classify** format from structure, using recognition table. No `commands/` folder → still invocable `/<name> [args]` in its own harness → report the command that must be materialised. Report only what you read. Never guess at contents.
3. **Report only:**
   - **Format** — what it is + evidence on disk.
   - **Proposed id / name / description** for `skill.json`. Description keeps source trigger wording, Caveman Ultra.
   - **On-demand knowledge** — which source text lands in `SKILL.md`. Note: `SKILL.md` is not injected per message; only `skill.json` is. Length is cheap.
   - **Commands** — one row each: name, source file, `params` schema, usage line.
   - **Tools** — one row each: name, `type` (`prompt` / `mcp`), what triggers it. mcp entry → name the command line, and say install will prompt.
   - **Rewrites needed** — tool names, shell dialect, placeholder dialect, frontmatter, prose → Caveman Ultra.
   - **Compression risk** — passages whose meaning depends on wording → must stay long. Name them.
   - **Losses** — no Demido equivalent: per-skill tool limits, model pins, hooks, auto-invocation by description or glob, pre-execution syntax, tools backed by real code.
   - **Verdict** — clean port / port with losses / not portable. One line.

End by offering `/convert-skill $source`.
