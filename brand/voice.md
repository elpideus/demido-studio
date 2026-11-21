# Voice and writing

Demido writes in three registers. Each has one job. Do not mix them.

| Register | Where | Job |
|---|---|---|
| **Manual** | Everything a person reads: answers, UI text, errors, docs | Be understood in one pass |
| **Caveman** | Everything only a model reads: system prompts, tool guidance, thinking, messages between agents and sessions | Spend the fewest tokens that keep the meaning |
| **Reference** | Settings descriptions, tool pages, changelogs | Be complete and exact |

## 1. Manual: text for people

Written for a reader whose attention is short and valuable. The model for it is an aircraft operating manual.

**Rules**

1. Answer first. The first line is the answer or the outcome.
2. One idea per sentence. Aim for under 20 words.
3. Procedures are numbered steps. One action per step. Start with a verb.
4. Prefer a list to a paragraph, and a table to a list of comparisons.
5. Use exact values with units: `6.2 GB`, `38 tokens/s`, `in 4 min`. Never "a lot" or "soon".
6. Say what happened, then what to do next.
7. No filler, no praise, no apology, no exclamation marks, no emoji.
8. Depth is offered, never forced: a short answer plus a **Details** disclosure. Nothing is hidden; nothing is dumped.

**Signal words.** Three levels, used exactly, always with an icon and a label (never colour alone):

| Word | Use when |
|---|---|
| **NOTE** | The reader benefits from knowing this. Nothing breaks if they skip it. |
| **CAUTION** | Work, time or money can be lost. |
| **WARNING** | The action cannot be undone, or it exposes the machine or private data. |

**Example**

Not this:

> Great question! It looks like the model you selected might be a little too large for your graphics card, so you may run into some slowdowns. You might want to consider trying a smaller quantization if that's something you're open to.

This:

> **This model does not fit your GPU.**
> It needs 9.8 GB. You have 7.6 GB free.
>
> 1. Pick `Q4_K_M` (6.1 GB). Recommended.
> 2. Or keep `Q6_K` and run 11 of 33 layers on the CPU. Slower: about 9 tokens/s.
>
> CAUTION: Option 2 leaves no room for an agent or a voice model.

## 2. Caveman: text for models

Small models lose the thread in long prose. Every internal text is written short and literal. People can always read it (nothing is hidden), but it is not written for them.

**Rules**

1. Drop articles, pleasantries, hedges and connective filler.
2. One fact or one instruction per line.
3. Imperative verbs. Positive form first: say what to do, then what never to do.
4. Keep names, paths, numbers, code and error text exact. Never abbreviate an identifier.
5. Use `→` for cause or sequence, `=` for definitions, `!` only for hard limits.
6. No examples unless the model fails without one. Then exactly one, minimal.
7. If a sentence still works with a word removed, remove it.

**Example**

Not this:

> You are a helpful assistant with access to a file editing tool. Before you make any changes to a file, it's important that you first read the file so that you understand its contents. Please be careful not to guess file paths.

This:

```
Edit files with edit_file.
Read file first → then edit.
Path unknown → list_dir. Never guess path.
```

**Scope.** Caveman applies to: system and task prompts, tool guidance (the "skill" part of a Tool), tool index entries, thinking, compaction summaries, agent briefs and reports, session-to-session messages. It never applies to anything rendered for a person as an answer.

## 3. Reference: settings and tool pages

Every setting and every Tool has a description a person can read. It is complete, literal and short.

1. First sentence: what it does.
2. Second sentence: when to change it.
3. Then the facts: default, range, unit, what it costs (memory, speed, tokens, money).
4. Name the trade-off. Every setting exists because of one.

> **Idle unload** · Default `5 min` · Range `0–120 min`, or Never
> Unloads the model after this long without use. Lower frees memory sooner; higher avoids reload waits (about 4 s for this model).

## 4. Interface text

- Sentence case everywhere. No Title Case, no ALL CAPS except signal words and micro-labels.
- Buttons are verbs and name their object: **Load model**, **Unload**, **Lend to agent**. Never "OK", "Submit", "Click here".
- A confirmation states the consequence and its button repeats the verb: "Delete 3 sessions? This cannot be undone." → **Delete 3 sessions** / **Cancel**. Never "Are you sure?".
- Errors: what happened + why (if known) + the next step. Never "Oops", never "Something went wrong" alone.
- Empty states say what will appear here and offer the one action that fills it.
- Numbers use tabular figures. Sizes in GB with one decimal. Speeds in tokens/s. Durations in the largest sensible unit.
- Time: relative under 24 h ("12 min ago"), then date. Schedules always show the next run in absolute time.
- Not implemented yet is said plainly: a **Not built yet** marker, never a silent dead control.

## 5. Vocabulary

One word per thing, everywhere: UI, docs, prompts, code.

| Term | Means | Do not say |
|---|---|---|
| **Model** | Weights that can be loaded | LLM, AI, engine |
| **Runtime** | The program that runs a model (for example llama.cpp) | Backend, engine |
| **Provider** | A remote service or CLI that runs models elsewhere | API, cloud |
| **Session** | One conversation and its files | Chat, thread |
| **Agent** | A helper with a fresh context, started by a session | Sub-agent, worker |
| **Task** | One item of a session's task list; may hold sub-tasks | To-do, step |
| **Tool** | Functionality plus guidance in one package | Skill, MCP, plugin |
| **External** | A Tool converted from an MCP server | |
| **Deprecated** | A classic MCP server or classic skill, still supported | Legacy |
| **Character** | A named persona: prompt, model, settings | Persona, bot |
| **Schedule** | Work that runs at set times | Cron, job |
| **Monitor** | A schedule that watches for a change | Watcher, alert |
| **Artifact** | A versioned file a model made for the session | Attachment, canvas |
| **Loaded / Unloaded** | Model is in memory / is not | Active, running, cold |
| **Lent** | Model is temporarily used by another session, agent or schedule | Borrowed, shared |
| **Permission level** | Cautious, Balanced, Autonomous, Smart | Mode |

## 6. The name

- First mention: **Demido Studio**. After that: **Demido**.
- Never: DemidoStudio, DEMIDO, DS, Demido AI.
- The command is `demido`.
- Line we use under the name: **Make small models behave.**
