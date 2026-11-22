# 19. Settings for each model: how it picks its words

Written 2026-11-22, directly, before building settings for each model (brief 4.2, "per-model tuning"; plan
item 6). The question: what should a model on this computer be asked with (its temperature and the rest of
its sampling), who says so, and how does Demido find out for a file it is given?

How sure each line is: **Read** (read today in the source linked), **Tried** (on the owner's computer).

## What Demido did until now

| Fact | How sure |
|---|---|
| Demido sent no sampling settings with an answer. Only its own quiet questions set one: temperature 0 for a decision held to a shape (which tool), 0.2 for writing the account of an older part of a conversation | Read (`chat.rs`, `chat/turn.rs`) |
| So every answer used llama.cpp's own defaults: temperature 0.8, top-k 40, top-p 0.95, min-p 0.05, no presence penalty, repeat penalty 1.0 | Read (llama.cpp `tools/server/README.md`) |

## What llama.cpp does with a model's own settings

| Fact | How sure |
|---|---|
| A GGUF file can carry its maker's settings, as `general.sampling.temp`, `.top_k`, `.top_p`, `.min_p`, `.penalty_repeat`, `.penalty_last_n`, `.xtc_*`, `.mirostat*` and `.sequence`. The converter writes them from the original model's `generation_config.json` | Read (`gguf-py/gguf/constants.py`, `metadata.py`) |
| When it loads a model, llama.cpp takes each of these for a setting it was not given on its command line (`common_init_sampler_from_model`); a request may still set each one | Read (`common/common.cpp`) |
| The chat endpoint takes llama.cpp's own settings beside OpenAI's: `top_k`, `min_p`, `presence_penalty`, `repeat_penalty` and the rest | Read (server README, "`/completion`-specific features … are also supported") |
| None of the four test models carries them: their files were made without (Qwen3 0.6B, 1.7B, 4B from Qwen's own GGUF repositories; Granite 3.3 2B from IBM's) | Tried (`core/dev/gguf-keys.py`) |

So a file may say for itself, and llama.cpp honours it; most files in use today do not.

## What the makers say

Read today from each model's card on Hugging Face (and `generation_config.json`, where it has the numbers).
Temperature, top-p, top-k, min-p, presence penalty. Where a card gives sets for particular kinds of work
(precise coding, pictures), the one for general text is taken.

| Models | Thinking | Not thinking | Read |
|---|---|---|---|
| Qwen3 0.6B, 1.7B, 4B, 8B, 14B, 32B, 30B-A3B | 0.6, 0.95, 20, 0 | 0.7, 0.8, 20, 0 | Each card. "DO NOT use greedy decoding" when thinking: it "can lead to performance degradation and endless repetitions". A presence penalty of up to 1.5 or 2 for endless repetition, with a warning that it can mix languages |
| Qwen3.5 0.8B, 2B | 1.0, 0.95, 20, 0, presence 1.5 | 1.0, 1.0, 20, 0, presence 2.0 | Each card ("for text tasks") |
| Qwen3.5 4B, 9B, 27B | 1.0, 0.95, 20, 0, presence 1.5 | 0.7, 0.8, 20, 0, presence 1.5 | Each card ("for general tasks") |
| Qwen3.6 35B-A3B | 1.0, 0.95, 20, 0, presence 1.5 | 0.7, 0.8, 20, 0, presence 1.5 | Its card |
| Qwen3.8 27B | 1.0, 0.95, 20, 0, presence 0 | 0.7, 0.8, 20, 0, presence 1.5 | Its card |
| Gemma 4 E2B, E4B, 12B, 26B-A4B, 31B (instruction-tuned) | 1.0, 0.95, 64 | the same | Each `generation_config.json`; the 12B card says the same |
| Granite 3.3 2B instruct | nothing stated | nothing stated | Its card and its `generation_config.json`, which holds no sampling |
| gpt-oss 20B | nothing stated | nothing stated | Its card and its `generation_config.json` |

Two things follow. The recommendations differ between sizes of one family (Qwen3.5 2B and 4B do not
agree), so they are kept model by model, not by family. And they differ between thinking and not thinking,
which Demido knows for every answer (the session's thinking switch).

## Telling which model a file is

| Fact | How sure |
|---|---|
| A GGUF names its architecture (`qwen3`, `qwen35`, `qwen35moe`, `gemma4`, `granite`, `gpt-oss`). It does not tell Qwen3.5 from Qwen3.8: both are `qwen35` | Read (Hugging Face's own reading of each repository's GGUF) |
| A GGUF's `general.basename` and `general.size_label` name the model ("Qwen3", "1.7B"); `general.finetune` says what it was trained for ("Instruct", "Instruct-awq"); some name their base model's repository (`general.base_model.0.repo_url`: Granite's names its *base* model, not the instruct one) | Tried on the four test models |
| A GGUF repository's card names the model it was made from (`base_model`: `Qwen/Qwen3.5-9B` for `unsloth/Qwen3.5-9B-GGUF`) | Read (Hugging Face's API) |

So: for a model downloaded through Demido, the repository's `base_model` says which model it is, exactly.
For a file Demido was given, its own names are the best there is.

## A context for each model

| Fact | How sure |
|---|---|
| In router mode, llama.cpp takes each model's settings from a presets file (`--models-preset`, INI): `[*]` for every model, a section named after a model for that one; a command-line option wins over both | Read (server README, "Model presets") |
| A model's own `c` in its section is what it is loaded with: Qwen3 0.6B got a context of 4096 while `[*]` said 8192 | Tried (installed b12359, `/props` after loading) |
| A change to the presets file while the router runs is not taken: loaded again, the model still had 4096 after its section said 2048 | Tried |
| Through Demido: Qwen3 0.6B needed 1,814 MiB at 8192 and 1,361 MiB at its own 4096 | Tried |

## What was decided

In `docs/DECISIONS.md`, K26 to K32.
