---
name: Writing skills
description: How to turn a task you just did into a reusable skill when the user asks for one.
---

When the user asks to "make this a skill", "remember how to do this" or similar, call `create_skill`.

- **name**: short and specific ("SMA crossover report", not "Analysis").
- **description**: one sentence saying *when* to use it, because that is what future conversations read first.
- **instructions**: numbered steps another assistant can follow without seeing this conversation: which tools to call, with which parameters, and how to present the result. Replace the specific values you used (a symbol, a date) with placeholders and say where they come from.
- **Code**: if you wrote a Python script, make it take its inputs as command-line arguments (`sys.argv`) and pass it in `include_files` (or in `files`). In the steps, run it with `run_python` and `file: "skill:<skill-id>/<script>.py"` and `args`.
- **Commands** (optional): to let the user start the skill by typing `/name`, pass `commands`, e.g. `{"name": "volatility", "description": "Weekly volatility of a pair", "args": "<symbol>", "prompt": "Report the weekly volatility of $ARGUMENTS."}`. `$ARGUMENTS` is what the user types after the command; `$1`, `$2` are its words.
- Keep it short: the instructions are read whenever the skill is used, so every line should earn its place.

After creating it, tell the user the skill's name and how to use it.
