#!/usr/bin/env node
// Regenerate the in-app Demido skill bundle from the canonical AGENTS.md.
// One source of truth (AGENTS.md); this copies it into skills/demido-dev/ so
// Demido's own agent can load it. ponytail: manual regen, add a watcher only if
// drift actually bites.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "skills", "demido-dev");
const content = readFileSync(join(root, "AGENTS.md"), "utf8");

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "SKILL.md"), content);
writeFileSync(
  join(outDir, "skill.json"),
  JSON.stringify(
    {
      id: "demido-dev",
      name: "Demido Studio Dev Guide",
      description:
        "Work on the Demido Studio codebase. Use when editing this repo — Rust/Tauri backend, React frontend, skills, providers, tools, DB. Repo map, facts LLMs get wrong, dev/verify loop. Read this skill's SKILL.md before editing; only this metadata is in context.",
      version: JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version,
      commands: [],
    },
    null,
    2,
  ) + "\n",
);

console.log("Wrote skills/demido-dev/{skill.json,SKILL.md} from AGENTS.md");
