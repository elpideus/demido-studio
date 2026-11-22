# 20. Can a small model choose which model does a piece of work?

Written 2026-11-22, directly, before agents may pick their own model (DECISIONS O2, O3, O8; plan item 7).
The question: given a short list of the models it may hand work to, does a small model pick the right one
for a piece of work? And in what words should the list be told?

How sure each line is: **Tried** (measured on the owner's computer with `core/dev/roster-trial.py`).

## The trial

Each of the four models on this computer plays the one that hands out work. It is shown a roster of three
models and one piece of work, and asked which should do it; its reply is held to the roster's names (O3),
at temperature 0 with thinking off, as Demido asks its own quiet questions. Eighteen pieces of work: six
mechanical (the fastest should do them: reformatting, sorting, counting), six that need care (the most
careful: a bug, a contract, a proof, a careful letter), six that need eyes (the one that reads images: a
photo, a receipt, a screenshot). The model that reads images is not on this computer; it does not have to
be, since only its name is chosen. Small numbers: direction, not a benchmark.

The roster was told six ways:

| Roster | What the model sees |
|---|---|
| labels | Each model's name with a plain label worked out from its facts (O2): "the fastest. For short, mechanical work…", "the most careful, and slow…", "reads images…" |
| facts | Only the numbers: size, words a second, what it takes in |
| labels, moved | The labels in another order |
| labels, or me | The labels, and "yourself" as a fourth choice |
| roles | The choices named by what each is for ("fastest", "most-careful", "reads-images"), not by the model |
| roles, moved | The same in another order |

## What came of it

Right picks, of 18 (mechanical / careful / eyes, of 6 each):

| Hands out | labels | facts | labels, moved | labels, or me | roles | roles, moved |
|---|---|---|---|---|---|---|
| Qwen3 0.6B | 9 (3/6/0) | 6 | 5 | 8 | 8 | 7 |
| Granite 3.3 2B | **17** (6/5/6) | 11 | 14 | 10 | **17** | 16 |
| Qwen3 1.7B | 14 (4/5/5) | 7 | 9 | 14 | 11 | 7 |
| Qwen3 4B | **17** (5/6/6) | 12 | **17** | 15 | **17** | 15 |

All Tried, 22 November 2026, llama.cpp b12359 on the owner's card.

1. **Two of the four choose well, two do not.** Granite 2B and Qwen3 4B pick right 17 times in 18 with plain
   labels. Qwen3 1.7B manages 14, and Qwen3 0.6B 9: it never once sent a picture to the model that reads
   images.
2. **Raw facts are much worse than labels, for every model**: 3 to 7 fewer right. A small model does not work
   out "the fastest" from "writes 300 words a second" well. O2's plain labels are not a nicety.
3. **The small Qwens follow the list's order.** With the labels moved, Qwen3 0.6B and 1.7B picked the first
   model listed 15 times in 18. Naming the choices by role did not cure it: they then leaned on
   "most-careful" (17 of 18, moved). Granite and Qwen3 4B change by one to three.
4. **"Yourself" is a trap for one of them.** Offered, Granite kept 8 of the 18 for itself, mostly work that
   another model should have had; the others hardly took it.

## What it decides

In `docs/DECISIONS.md`, O9 to O12. In short: whether a model may choose is decided for each model by this
trial, the first time it would hand out work; the roster is always told in plain labels; a picture goes to a
model that reads images by a rule in code; and "yourself" is not offered as a choice.
