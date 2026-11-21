# Later

Plans the owner wants to try further down the road. Each is set aside on purpose, not forgotten.

How this differs from the other files: `PLAN.md` is the order of the work being done now. `IDEAS.md` is
every idea raised along the way, most of them small. This file holds the few that the owner has said he
wants tried, written out far enough that they can be picked up cold.

## 1. Words chosen for each model

Set aside by the owner on 2026-11-21: "keep the per-model wording in a file with plans that I want to try
later down the road." It was IDEAS 98.

### What it is

The first time a model is used, Demido runs a short fixed trial on it, keeps the wording that suits that
model, and from then on talks to it in those words. The person sees the score.

### Why

Measured on 2026-11-21 (research note 13, "The question itself"): the same question, in five wordings, to
four models. Decisions right, of 26:

| Asked | Qwen3 0.6B | Granite 2B | Qwen3 1.7B | Qwen3 4B |
|---|---|---|---|---|
| "Which tool does the request need?" | 22 | 17 | 22 | 25 |
| "Does the request need a tool? Most do not." (what Demido uses) | 18 | 21 | 21 | 26 |
| "Can you answer from what you know? Then the tool is none…" | 10 | 22 | 10 | 24 |

The two families fail in opposite directions on the same words. Granite, asked "which tool", uses one for
nearly everything. The small Qwens, asked "can you answer from what you know", never use a tool again.
There is no wording that is right for both. Demido ships the one that errs towards doing nothing, which
costs the smallest Qwen four right decisions and still leaves Granite using a tool where none is needed
half the time.

So the gain is real and its size is known: on these four models, picking the best of the three wordings
for each would have given 22, 22, 22 and 26 of 26, against 18, 21, 21 and 26 with one wording for all.

### How it could work

1. **A fixed trial.** About twenty short requests with known right answers: some need a tool, some need
   none, some need two in a row. `core/dev/tool-turn-trial.py` is the start of it. A few seconds on a
   graphics card.
2. **A handful of wordings**, not a search: the ones above, and whatever later measurements add.
3. **Run once per model**, the first time it is loaded, or when the person asks ("Try this model"). The
   result is kept with what Demido already knows about the model (`engine/known.json`, DECISIONS K24), and
   thrown away when the model's file or the runtime changes.
4. **Shown, not hidden**: "This model uses tools right 21 times in 26, asked this way." A model that scores
   badly however it is asked gets a plain warning, and tools can be off for it by default.
5. **The same trial can choose more than the tool question**: whether the model plans when asked, whether
   it needs steps cut down to instructions, how it likes a result handed to it. Each is a switch Demido
   already has or could have.

### What it needs first

- Tools with real arguments (plan item 5), so that the trial measures what matters and not only a clock
  and a calculator.
- The work on settings per model (brief 4.2, plan item 6): this is the same idea reaching further than
  temperature, and the two should be one feature.
- A decision on what a model may be told about its own score (DECISIONS O, the roster): a model that is
  poor with tools should not be given tool work by an orchestrating model.

### What I do not know

- Whether twenty requests are enough to tell a real difference from noise. The same row measured twice
  differed by one or two of 26. A trial this short can pick the wrong wording by chance.
- Whether the best wording holds across quantizations of the same model (Q4 against Q8). Not measured.
- Whether it holds in other languages. Every trial so far was in English.
