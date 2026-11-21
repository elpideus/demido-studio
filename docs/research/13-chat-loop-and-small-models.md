# 13. The chat loop, and making small models behave

Written 2026-11-21, directly, before building the part of Demido that talks to a loaded model. The brief asks
for a researched, proposed full set of techniques that make small models behave (sections 3 and 4.3), and
names three: self-sanitizing tool requests, compressed prompts, and vector reasoning in place of language
reasoning.

How sure each line is, said every time:

- **Tried**: measured on the owner's computer, on the two test models (Qwen3 0.6B and 1.7B, Q8), with the
  script named.
- **Read**: read by me in llama.cpp's own documentation at the stable build (`b12359`), or printed by the
  installed program itself.
- **Search**: a web search result (title and summary of a paper). I did not read the papers in full. Treat
  as a pointer, not as a fact.
- **Memory**: what I know from before, up to mid-2026. Not checked today.

## What the runtime offers for talking to a model (Read)

| Thing | How |
|---|---|
| Chat, streamed | `POST /v1/chat/completions` with `stream: true`. Each reply carries `timings`: tokens read from cache, tokens processed, tokens written, and tokens a second. |
| Its own tool calls | `tools` in the request. Uses the model's own tool format when it knows it (Qwen, Llama 3.x, Hermes, Mistral Nemo and others), a generic one otherwise. Several calls at once only where the model's template allows. |
| Forced shapes | `response_format` with a JSON schema, or a grammar. The reply cannot leave the shape. |
| Thinking | Per request: `chat_template_kwargs` (for Qwen3, `enable_thinking`), `reasoning_effort`. Per start: `--reasoning on|off|auto`, a token budget for thinking. A running thought can be ended from outside (`/v1/chat/completions/control`). Thinking comes back apart from the answer (`reasoning_content`). |
| Counting tokens before sending | `POST /v1/chat/completions/input_tokens`. |
| Reusing work | The start of a prompt that is the same as last time is not processed again (`--cache-prompt`, on by default). `timings.cache_n` says how many tokens were reused. |
| Pictures and sound in a message | Typed content parts, for models that take them. |
| Steering without words | `--control-vector` (a file of directions inside the model), `--lora`. |

## Tried: how to ask a small model for a tool call

`core/dev/tool-call-trial.py`. 26 short requests: 20 need one of four tools, 6 need none ("What is 7 times
8?"). Temperature 0, thinking off unless said. "Right" means the right tool with the right arguments, or
rightly no tool. Small numbers: this shows direction, it is not a benchmark, and both models are one family.

| Way of asking | 0.6B right | 1.7B right | Prompt tokens | Time per reply (0.6B) |
|---|---|---|---|---|
| The runtime's own tool calls | 26 of 26 | 25 of 26 | 412 | 88 ms |
| The same, thinking on | 25 | 26 | 408 | 414 ms |
| A JSON reply, asked in many words | 25 | 26 | 320 | 75 ms |
| A JSON reply, asked in few words | **20** | **22** | 139 | 91 ms |
| …with every part of the shape forced | **20** | **22** | 139 | 93 ms |
| …with a check and one repair turn | 20 | 24 | 139 | 92 ms |
| Few words, and **"no tool" is one of the tools** | **26** | **26** | 145 | 88 ms |
| …with only the tool's name forced | 26 | 26 | 145 | 88 ms |
| …with every part forced | 26 | 26 | 145 | 89 ms |

What it shows:

1. **The failure is not broken JSON. It is not knowing when to leave the tools alone.** Asked in few words,
   the 0.6B model used a tool for all six requests that needed none ("Say hello in French" became a weather
   lookup for Paris). Every one of its misses was that.
2. **Forcing the shape did not fix it.** With the whole reply forced into a schema, the same six were wrong:
   valid, and wrong. This matches what the papers below report.
3. **Making "no tool" a choice like the others fixed it completely**, with or without forcing, at a third of
   the prompt of the runtime's own tool format. The only change was the shape of "no": from a different
   kind of reply (`{"answer": …}`) to one more entry in the same list (`{"tool": "none", "answer": …}`).
4. **Fewer words is not the same as better.** The short prompt that lost six dropped a sentence the long one
   had ("many questions need no tool"). The short prompt that lost none kept every choice and dropped only
   words. The rule for the brief's "compressed prompts": **cut words, never choices.**
5. **Thinking cost five times the time and bought nothing here.** These are easy requests; it may pay on
   hard ones. That is not measured.
6. **A repair turn mends what a check can see** (a missing argument, a tool that does not exist) and nothing
   else. It recovered two of four on the 1.7B model. It cannot know that a valid call is the wrong one.

What it does not show: anything about many tools, nested arguments, several steps, other model families, or
other languages. On this easy set most ways reach the ceiling, so it cannot say whether forcing costs
correctness on harder work, which is what the papers below claim.

## What others report (Search; not read in full)

- "The Constraint Tax" (arXiv 2605.26128, May 2026): for models under 3B, a hard tool-call schema made every
  reply valid and far fewer right (one model: 91.5% with a prompt alone, 48% under the schema).
- "Repair, Not Improvement" (arXiv 2608.13959): a grammar over the tool names alone removes malformed calls;
  the gain on small models is large; the hard part is abstaining.
- "Don't Adapt Small Language Models for Tools; Adapt Tool Schemas to the Models" (arXiv 2510.07248).
- StructureBench (IJCAI 2026): forced shapes make tool calls dependable on small devices, and the smallest
  models degrade under complicated grammars.
- Latent reasoning: several surveys (2025 to 2026). Methods like Coconut need the model to be trained for
  it. One 2026 analysis ("The Illusion of Superposition?") finds the gain may not come from the latent
  thoughts at all.
- Qwen3 with thinking on has been reported to put tool calls inside its thinking, where they are not read
  as tool calls. Community reports; our build parsed all 26 correctly with thinking on.

## The full set, proposed

Each with how sure it is and when it gets built. "Item" is the number in `docs/PLAN.md`.

| # | Technique | What it is | How sure | When |
|---|---|---|---|---|
| 1 | **"No tool" is a tool** | Demido's own short tool format: a few lines of tools, one JSON object back, and declining is an entry in the same list. Works on models without a tool format of their own. | Tried: 26 of 26 on both models at a third of the tokens | The mechanism now (item 4), tools with item 5 |
| 2 | **Check, then one repair** (the brief's "self-sanitizing tool requests") | Every request is checked against the tool's own description before anything runs. If it does not fit, the model is told what is wrong in plain words and tries once more. | Tried: mends what a check can see | Item 4 |
| 3 | **Force the name, not the rest** | The model can only name a tool that exists. Arguments are checked afterwards, not forced. | Tried: no difference on the easy set. Search: forcing everything can cost correctness. So: provisional, and measured again on harder work | Item 5 |
| 4 | **Few tools in view** | The model is shown the handful of tools that fit the request, picked by meaning, not all of them. | Memory, and it follows from every count of tokens above. Not tried | Item 5, with embeddings (brief 4.8) |
| 5 | **Cut words, never choices** (the brief's "compressed prompts", and caveman) | Internal text is short. Every option the model has stays visible in it. | Tried: the two short prompts above | A rule from now on |
| 6 | **Thinking only where it pays** | Off for mechanical decisions. A switch or levels where the model has them (brief 4.9), with a budget, and a way to end a long thought. | Tried: five times slower, no gain on easy calls. Read: the controls exist | Item 4, the switch with the session screen |
| 7 | **A task list the model keeps** (brief 4.6) | The work is split into tasks and sub-tasks. Each turn the model is shown where it is: the task in hand, what is done, what is next. | Memory. Not tried | Item 4, after plain chat |
| 8 | **A fresh start for each piece** (brief 4.17) | A sub-task that does not need the conversation is given to an agent with an empty context. | Memory. Not tried | Item 7 |
| 9 | **The same beginning every time** | The prompt always starts with the same bytes (instructions, tools), and history is only ever added to. The runtime then does not read it again. | Read: the cache exists and reports itself. To be seen on the first real session | Item 4, from the first turn |
| 10 | **Compaction** (brief 4.15) | When the conversation nears what the model can hold, the oldest part is replaced by a short account of it. By hand or by itself, with a size worked out from the model and the memory. | Read: tokens can be counted before sending. Not tried | Item 4, after plain chat |
| 11 | **Each model its own settings** (brief 4.2) | Temperature and the rest as the model's makers recommend, not one default for all. | Research owed | With the models work (item 6) |
| 12 | **Catching a model that loops** | A reply that repeats itself or runs on is stopped, and the turn says so. | Memory. Not tried | Item 4 |
| 13 | **Outside text is data** (brief 4.4) | What comes from pages, files, mail and tools is marked as material, never as instructions. | Research note 05, not yet fact-checked | Item 5 |
| 14 | **A model for each job** (the owner's addition) | Mechanical work goes to the fastest model that fits; the careful one plans. | Designed (DECISIONS O); not tried | Item 7 |

## "Vector reasoning": what is feasible, honestly

The brief names "vector reasoning instead of language reasoning, where applicable", marks it experimental,
and asks me to say what is feasible. Three different things could be meant.

1. **Deciding by meaning, not by asking.** Which tools fit a request, which memory is relevant, which model
   suits a job: these can be answered by comparing embeddings, with no words generated at all. It is fast,
   it does not use the model's context, and it is not fooled by phrasing. **Feasible today**, with a small
   embedding model. This is technique 4, and I would build it.
2. **Steering a model from inside.** The runtime can add a "control vector" to a model: a direction in its
   internals, for example "terse". **Feasible as an experiment**: the switch exists (Read), but each vector
   must be made for each model, and I have no measurement that it beats a good prompt. Worth one honest
   trial, not a promise.
3. **Reasoning in the model's own vectors and not in words** (Coconut and its relatives). This needs models
   that were trained to do it. The models people download are not. **Not feasible** for Demido now, and the
   one 2026 analysis I found (Search) doubts where the gains come from. I would not spend time here until
   such models are published in the formats the runtime loads.

So my proposal: build 1 as part of the tools work, give 2 a single measured trial later, leave 3 alone.
This is the owner's call; the brief asks for it to be discussed.

## Decided for the first slice of the chat loop

Recorded in `docs/DECISIONS.md`, section L. In short: sessions and messages are kept in the core's own
storage; a turn is run by the core, streamed as news that is told and not kept, and stored when it is
complete; the beginning of the prompt never changes; thinking is off until its switch exists; what a model's
answer says about its own speed is kept for the roster. No tools yet: the mechanism above is built with the
first real tools.

## Sources

- <https://github.com/ggml-org/llama.cpp/blob/b12359/tools/server/README.md>
- <https://github.com/ggml-org/llama.cpp/blob/b12359/docs/function-calling.md>
- Search results (not read in full): <https://arxiv.org/abs/2605.26128>, <https://arxiv.org/abs/2608.13959>,
  <https://arxiv.org/abs/2510.07248>, <https://arxiv.org/abs/2609.23742>,
  <https://www.ijcai.org/proceedings/2026/0267.pdf>, <https://arxiv.org/abs/2507.06203>,
  <https://arxiv.org/abs/2604.02029>, <https://huggingface.co/blog/qwen-3-chat-template-deep-dive>
