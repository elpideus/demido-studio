# 21. Mechanical work, the fastest model, and fresh contexts

Written 2026-11-22, directly, right after note 20, before building agents (plan item 7). DECISIONS O4 says:
"Work that the task list marks as mechanical and not needing the conversation goes to the fastest model that
is already loaded or fits beside the main one." Two things have to hold for that rule to help: a small model
has to be able to tell which tasks are mechanical, and the fastest model has to do mechanical work about as
well as a bigger one. The brief adds a third promise (4.17): "agents work on fresh contexts, which could be
extremely useful for making models behave."

How sure each line is: **Tried** (measured on the owner's computer with `core/dev/mechanical-trial.py`,
22 November 2026, llama.cpp b12359, temperature 0, thinking off). Small numbers: direction, not a benchmark.

## 1. Can a small model tell a mechanical task?

Twelve tasks of four requests (six mechanical: pull the dates out, sort the rows, add up the totals; six that
need judgement or the conversation: decide which deadline matters, write in the tone agreed earlier, say
which invoice looks wrong). Each model was asked of each task, three ways, its reply held to the shape.

| Asked | Qwen3 0.6B | Granite 2B | Qwen3 1.7B | Qwen3 4B |
|---|---|---|---|---|
| "Is the task mechanical?" (yes/no) | 6 of 12 (all yes) | 7 | 7 | 6 (all yes) |
| "Does it need judgement? Most tasks do." (yes/no) | 6 (all yes) | 8 | 8 | 7 |
| "Which kind of work: copying or thinking?" (each described) | 8 | 9 | 9 | **10** (thinking 6/6) |

Asked yes or no, a small model gives one answer for every task: Qwen3 0.6B called all twelve mechanical, and
asked the other way round, all twelve not. Asked to pick between two kinds of work, each described, they do
better, and Qwen3 4B errs on the safe side (it kept every task that needed thinking). None is reliable.

## 2. Does the fastest model do mechanical work as well?

Ten mechanical tasks with answers checked exactly (sort five words, one a line; rewrite three dates; count the
ERROR lines of a log; add up four amounts; turn a sentence into JSON…), given on a fresh context: only the task.

| | Qwen3 0.6B | Granite 2B | Qwen3 1.7B | Qwen3 4B |
|---|---|---|---|---|
| Done right, fresh | 5 of 10 | 3 | 7 | **8** |
| Time for a task | 76 ms | 110 ms | 112 ms | 134 ms |

The fastest model is fastest by about 60 ms a task, and gets three more of ten wrong than Qwen3 4B: commas
where lines were asked for, a wrong date, a count of 0, sums worked out at length instead of the total.
Granite's misses are mostly the shape (a numbered list, commas, an e-mail address it made up).

## 3. Does a fresh context help the same model?

The same ten tasks, given at the end of a six-turn conversation about something else (planning a week in
Lisbon), as a session's task arrives today:

| | Qwen3 0.6B | Granite 2B | Qwen3 1.7B | Qwen3 4B |
|---|---|---|---|---|
| In a conversation | 3 of 10 | 2 | 6 | 7 |
| Fresh | 5 | 3 | 7 | 8 |

Every model did better on a fresh context, by one or two tasks of ten.

## What it suggests

1. **O4, as written, would make answers worse on these models**: the models cannot mark mechanical tasks
   reliably, and the fastest model gets more of them wrong. The time saved is small for short tasks. It should
   not be on by default for models of this size; it may suit a pair where the fast model is still good (it
   would be tried the same way, per pair, like O9).
2. **The fresh context is the part that helps**, and it costs nothing in memory: the same model, given a task
   with only what the task needs, does better. A first piece of agents could be exactly that: a task the
   planner marks as copying work (asked as "which kind", the best of the three wordings) is done by the same
   model on a fresh context, with the request, the plan and the results it needs, and without the
   conversation before it. Whether that helps on real plans, and not only on these short tasks, would be
   tried first with `core/dev/tool-turn-trial.py`'s cases.

These are proposals for the owner (plan item 7 is "the hardest design left"): DECISIONS O13 and O14. Decided
on 2026-11-22: O13 as proposed (off by default); O14 on hold, with the rest of agents.
