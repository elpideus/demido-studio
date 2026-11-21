# 13. The chat loop, and making small models behave

Written 2026-11-21, directly, before building the part of Demido that talks to a loaded model. The brief asks
for a researched, proposed full set of techniques that make small models behave (sections 3 and 4.3), and
names three: self-sanitizing tool requests, compressed prompts, and vector reasoning in place of language
reasoning.

How sure each line is, said every time:

- **Tried**: measured on the owner's computer, with the script named. The first trials used two test
  models (Qwen3 0.6B and 1.7B, Q8). From the trial of turns with tools on, four: those two, Qwen3 4B
  (Q4_K_M), and Granite 3.3 2B (Q4_K_M), a second family, because every table before it carried the
  warning "both models are one family". All four are Apache-2.0, from their makers' own pages.
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

## Seen: the runtime says what each loaded model can do

Asked of the running runtime on 2026-11-21 (`GET /props?model=…`, for the 1.7B test model). It answers, for
a model that is loaded:

- its chat template, word for word (4,100 characters for Qwen3, and it names `enable_thinking`);
- what the template supports: a system message, tool calls, several tool calls at once, levels of
  reasoning effort (none for Qwen3: thinking is on or off);
- what the model takes in besides text: pictures, video, sound (none of them for this one);
- the sampling settings it runs with when nothing is said (temperature 0.8, top-k 40, top-p 0.95, min-p 0.05).

So whether a model can think, takes tools, or reads pictures does not have to be guessed from its name. It
is read from the runtime the first time the model is loaded, and kept. That is where the switch for thinking
(brief 4.9) and the roster's "what it can take in" (DECISIONS O1) will get their facts.

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

## Tried: how much of a conversation is read again each turn

The runtime keeps what it has read, so an unchanged beginning of a prompt costs nothing the next time.
On a real two-turn session with the 1.7B model: the first prompt was 64 tokens; the second was 301, of which
60 were not read again and 241 were. The 241 are the first answer (222 tokens, written by the model a moment
before) and the new question.

So everything older than the last answer is free, and the last answer is read once more. The reason is in the
model's template: with thinking off it writes an empty thinking mark when it starts an answer, and that mark
is not there when the same answer comes back as history, so the two no longer match from that point. Three
ways of writing the history were tried (plain, with an empty thought, with the mark as text); none avoided
it. The cost is small: reading runs at about 3,000 tokens a second on this card and writing at about 150, so
re-reading an answer takes about a twentieth of the time it took to write.

## Tried: how to ask a small model for an account of a conversation

Compaction (brief 4.15) needs the model to write a short account of the older part of a conversation. The
first wording I built put the instruction in the system message and the conversation after it. On the real
1.7B model the result was wrong in a way that matters: it answered the conversation's last question and
called a trip in May a trip in October.

So five ways of asking were measured, `core/dev/account-trial.py`: three conversations (a trip, a bug, a
purchase), each ending in a request, with five facts each that an account must keep. Counted: how many of the
15 facts are in the account, and in how many of the 3 the model went on with the conversation and did not
write an account. Temperature 0.2, thinking off, at most 400 tokens.

| Way of asking | 0.6B: facts kept | 0.6B: went on | 1.7B: facts kept | 1.7B: went on |
|---|---|---|---|---|
| Instruction first (system), then the conversation | 1 of 15 | 2 of 3 | 3 of 15 | 2 of 3 |
| The conversation fenced, then the instruction | 12 | 1 | 13 | 1 |
| A one-line role first, the conversation fenced, the instruction last | **13** | 1 | **14** | **0** |
| The same, asking for a form described in words | 12 | 2 | 13 | 0 |
| The same, with the form forced by a schema | 10 | 2 | 13 | 0 |

What this says:

- **Where the instruction stands decides nearly everything.** A small model treats the last thing it read as
  the thing to do. If that is a person's request inside the material, it carries the request out.
- **A form does not help, and forcing it costs** the smaller model facts (10 of 15). The same finding as for
  tool calls above: the shape is not what these models get wrong.
- **It is not solved.** With the best wording the 0.6B model still went on with the conversation once in
  three. Nothing in Demido checks an account yet.

A small measurement: three conversations, two models of one family, one run each. Enough to choose a
wording; not enough to call the rule proven for other families.

The rule taken from it (DECISIONS L13): material first and fenced, what to do with it last. It applies
beyond accounts, to everything a model is handed: pages, files, tool results. That is also where technique
13 below starts.

## Tried: the shape of a turn with tools, inside a conversation

The first trial asked single questions and let the model put its answer inside the JSON reply. A chat is
different: there is a conversation before the request, answers are long and have lines and code in them, and
a tool's result has to be used afterwards. `core/dev/tool-turn-trial.py`: 26 requests, each with a short
conversation before it; 16 need a tool, 10 need none; four models; temperature 0; thinking off. Five tools
are in view (a clock, a calculator, a file reader, a web search, a plan). The file reader and the search
do not exist yet: their results are stood in, since the trial is about deciding, not doing.

Counted: decisions that are right (of 104 over the four models), and answers that can be used, measured
apart from the decision (of 88): ordinary text that says the thing, not a stray tool call or broken JSON.

| Shape of turn | Decided right | Answers usable |
|---|---|---|
| One reply: every reply is a JSON object, the answer inside it. Read as kindly as possible | 87 | 81 |
| …with the tool's name forced | **93** | 80 |
| Decide, then answer as text. Tools listed after the request | 70 | 86 |
| …tools listed before the request, the request fenced | 81 | 86 |
| …the same, nothing forced | 79 | 86 |
| …tools before the request, the request not fenced (what is built) | 84 | **86** |
| …tools in the first message of the conversation | 82 | 81 |
| …in the first message, and the decisions kept in the conversation | 83 | 83 |

Per model, for the shape that is built: 17, 21, 21 and 25 of 26 decided right (Qwen3 0.6B, Granite 2B,
Qwen3 1.7B, Qwen3 4B). The same row measured twice differs by one or two: that is the noise, and
differences of that size mean nothing.

What it shows:

1. **No shape decides best and answers best.** One JSON reply decides a little better. Two questions keep
   the answers. I chose the answers, and what the split gives besides: the request for the answer is word for
   word a plain chat request, the answer is streamed as text, and thinking can be on for the answer only.
2. **Where the tools are listed matters less than I expected, and "material first, instruction last" did
   not carry over to it.** Listed after the request (so that "none" is the last thing read before the
   question), models chose "none" far too often: 70.
3. **The 4B model is right nearly every time in every shape** (23 to 26 of 26). The design is for the models
   below it.
4. **After a tool's result, knowing what comes next is weak** on the small ones: 1, 3, 5 and 6 of 6. Two
   mistakes are caught mechanically (asking for the same thing again; a request that cannot be used). One
   is not: stopping too early and working the rest out in its head.

### The question itself

The same shape, five wordings of the question, decisions right of 26 per model:

| Asked | 0.6B | Granite 2B | 1.7B | 4B | All |
|---|---|---|---|---|---|
| "Which tool does the request need?" | 22 | 17 | 22 | 25 | 86 |
| "Does the request need a tool? Most do not." (built) | 18 | 21 | 21 | 26 | 86 |
| "Can you answer from what you know? Then the tool is none. If not, which…" | 10 | 22 | 10 | 24 | 66 |
| Both replies shown as forms, tool first | 14 | 20 | 21 | 26 | 81 |
| Both replies shown as forms, "none" first | 21 | 20 | 17 | 25 | 83 |

The two families fail in opposite directions. Asked "which tool", Granite uses one for nearly everything
(1 of 10 plain requests left alone: it searches the web to say hello in French). Asked "can you answer from
what you know", the small Qwens never use a tool again (0 of 16). **No wording is right for every model.**
I took the one that errs towards not acting. The real answer is a wording chosen per model, from a short
trial like this one when a model is first used. That is not built.

### How a tool is described

| Described by | Decided right, of 104 |
|---|---|
| What the tool does | 79 |
| …and what the model cannot do without it ("do not work sums out yourself") | 84 |
| …and an example of use | 83 |

The example bought nothing and was copied: `(17 - 5) * 1.2` came back as the arguments of requests that had
nothing to do with it, and once as the whole answer to "give me a packing list".

### Who plans

With `plan` among the tools, on four requests that plainly had several parts: the three smaller models
chose it 0 times of 12; the 4B model two to four times of four. Two wordings of its description and listing
it first changed nothing. Asked directly ("list the steps needed to do the request"), the models listed
steps for 4, 4, 1 and 4 of the four. Asked the same of 22 requests with one part, they wrongly made a list
for 8, 5, 0 and 2. Asking "does it have several parts, yes or no" was not dependable either: the smallest
model never said yes, and the 1.7B said yes to 10 of the 22 requests with one part.

So: a small model does not decide to plan, and cannot be trusted to say when a plan is needed, but lists
steps when told to. Demido asks for the steps when the person asks for a plan.

## Tried for real: eight requests on four models, through Demido itself

After building it. Each request in a new session, through `demido ask`, with the built tools (clock,
calculator, plan). Right means the person got a right answer.

| Request | 0.6B | 1.7B | Granite 2B | 4B |
|---|---|---|---|---|
| What day is it today? | yes | yes | yes | yes |
| What time is it? | yes | yes | no: made up | yes |
| 437 euros each, for 4 of us plus one 35 euro bag | no: no tool, 1805 | no: asked for 437 × 5 | no: 437 × 5 | yes |
| Say hello in French | yes | yes | yes, after asking the calculator | yes |
| A packing list, three items | yes | yes | yes, through a needless plan | yes |
| A call at 17:30. How long until then? | no | no | yes | no: read the clock, then guessed |
| How many days until 25 December? | no | yes | no: made up | yes |
| Three steps, with Plan first: add hours, work out pay, one line | no: 373.5 | yes | yes | yes |
| Right | 4 of 8 | 6 of 8 | 5 of 8 | 7 of 8 |

Getting there changed the design five times in two hours, each time because a real model did something
the trial had not shown:

- A sum that could not be worked out became 7,900 tokens of one repeated line. Now a model that goes
  round is stopped (DECISIONS L23).
- "How many days until 25 December": the 4B model made up today's date and counted from it. Now the date
  is in the first message (L20), and the calculator counts days between dates, since "25 December 2026 -
  21 November 2026" is what a model asked it for, unprompted (L24).
- With the clock as a tool, two of three models made the time up. Writing the time on every message cured
  that and broke more: one model went on as if writing a chat log, another saw digits and asked the
  calculator to say hello. Taken out again. The time is a tool; the smallest models still sometimes guess it.
- `17:30 - 10:19` was read as two divisions and came back as 0.04, and a model told the person the call was
  in four minutes. A colon is no longer a division, and the calculator knows the time between two times.
- Planned steps came back with the sums already done in their titles, wrongly, and the final answer
  trusted the title over the calculator. Steps are now checked: no number that the request did not have
  (L22).

(Run again after the work on files, research note 14, with the request said again after a result: 3, 6,
4 and 8 of 8. The 4B model then got the call at 17:30 right. Answers are not written at temperature 0, so a
request or two moves from run to run.)

What is still wrong, plainly: two of four models read "4 of us plus one bag" as five tickets, which no
tool can mend, and a third did the sum in its head and got it wrong. The call at 17:30 came out right on
one model of four, and that one by writing both steps into a single request: a chain of two tools (read
the clock, then subtract) did not once come about. Granite 2B uses tools where none is needed about half
the time.

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
| 1 | **"No tool" is a tool** | Demido's own short tool format: a few lines of tools, one JSON object back, and declining is an entry in the same list. Works on models without a tool format of their own. | Tried twice: 26 of 26 on single questions; inside conversations it holds on three of four models and one family over-uses tools | Built (item 4). Tools that reach outside come with item 5 |
| 2 | **Check, then one repair** (the brief's "self-sanitizing tool requests") | Every request is checked against the tool's own description before anything runs. If it does not fit, the model is told what is wrong in plain words and tries once more. | Tried: mends what a check can see. Built, with silent mending of what is plain | Built (item 4) |
| 3 | **Force the name, not the rest** | The model can only name a tool that exists. Arguments are checked afterwards, not forced. | Tried twice. Once `time()` is read as `time`, forcing the name changes little (81 against 79 of 104: noise). Kept, because the reply is then always short and always readable. Search: forcing everything can cost correctness; not tried on hard arguments | Built (item 4). Measured again with the first tools that take real arguments (item 5) |
| 4 | **Few tools in view** | The model is shown the handful of tools that fit the request, picked by meaning, not all of them. | Memory, and it follows from every count of tokens above. Not tried | Item 5, with embeddings (brief 4.8) |
| 5 | **Cut words, never choices** (the brief's "compressed prompts", and caveman) | Internal text is short. Every option the model has stays visible in it. | Tried: the two short prompts above | A rule from now on |
| 6 | **Thinking only where it pays** | Off for mechanical decisions. A switch or levels where the model has them (brief 4.9), with a budget, and a way to end a long thought. | Tried: five times slower, no gain on easy calls. Read: the controls exist | Item 4, the switch with the session screen |
| 7 | **A task list the model keeps** (brief 4.6) | The work is split into tasks and sub-tasks. Each turn the model is shown where it is: the task in hand, what is done, what is next. | Tried on four models: they do not plan by themselves; asked, they list steps; walked by Demido, three of four got a three-step sum right | Built (item 4). Open: showing the plan for approval before it is walked |
| 8 | **A fresh start for each piece** (brief 4.17) | A sub-task that does not need the conversation is given to an agent with an empty context. | Memory. Not tried | Item 7 |
| 9 | **The same beginning every time** | The prompt always starts with the same bytes (instructions, tools), and history is only ever added to. The runtime then does not read it again. | Read: the cache exists and reports itself. To be seen on the first real session | Item 4, from the first turn |
| 10 | **Compaction** (brief 4.15) | When the conversation nears what the model can hold, the oldest part is replaced by a short account of it. By hand or by itself, with a size worked out from the model and the memory. | Built and tried on the real models, by hand and by itself. The size is never asked for: the runtime's count of the last answer is counted on from. The wording of the request was measured (above) | Done in item 4. Open: a check that an account is a good one; a default size worked out per model |
| 11 | **Each model its own settings** (brief 4.2) | Temperature and the rest as the model's makers recommend, not one default for all. And, since the trial of turns: the wording of the tool question, chosen per model from a short trial when the model is first used. | Research owed for the settings. Tried for the wording: no one wording is right for two families | With the models work (item 6) |
| 12 | **Catching a model that loops** | A reply that repeats itself or runs on is stopped, and the turn says so. | Tried: the first real failure was 7,900 tokens of one line. Built for answers | Built (item 4). Open: the same for thinking |
| 13 | **Outside text is data** (brief 4.4) | What comes from pages, files, mail and tools is marked as material, never as instructions. | Research note 05, not yet fact-checked. Tried here for one case: material fenced and put before the instruction (above) | Item 5 |
| 14 | **A model for each job** (the owner's addition) | Mechanical work goes to the fastest model that fits; the careful one plans. | Designed (DECISIONS O); not tried | Item 7 |

## "Vector reasoning": what is feasible, honestly (withdrawn by the owner; kept for the record)

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

So my proposal was: build 1 as part of the tools work, give 2 a single measured trial later, leave 3 alone.

**The owner's answer (2026-11-21): it was the third that was meant, and it is withdrawn.** "Forget about the
vector thing then." Nothing is built or tried under this name. Choosing tools by meaning (1) stays as
technique 4 above, for its own reasons. The trial of control vectors (2) is dropped: it was only a guess at
what the term meant.

## Decided for the first slice of the chat loop

Recorded in `docs/DECISIONS.md`, section L. In short: sessions and messages are kept in the core's own
storage; a turn is run by the core, streamed as news that is told and not kept, and stored when it is
complete; the beginning of the prompt never changes; thinking is off until its switch exists; what a model's
answer says about its own speed is kept for the roster. No tools yet: the mechanism above is built with the
first real tools.

Added since: the switch for thinking and "Answer now" (L6); making a long conversation shorter (L11 to L14);
turns with tools, the task list, the date, and stopping a model that goes round (L15 to L24).

## Sources

- <https://github.com/ggml-org/llama.cpp/blob/b12359/tools/server/README.md>
- <https://github.com/ggml-org/llama.cpp/blob/b12359/docs/function-calling.md>
- Search results (not read in full): <https://arxiv.org/abs/2605.26128>, <https://arxiv.org/abs/2608.13959>,
  <https://arxiv.org/abs/2510.07248>, <https://arxiv.org/abs/2609.23742>,
  <https://www.ijcai.org/proceedings/2026/0267.pdf>, <https://arxiv.org/abs/2507.06203>,
  <https://arxiv.org/abs/2604.02029>, <https://huggingface.co/blog/qwen-3-chat-template-deep-dive>
