#!/usr/bin/env python3
"""What shape should a turn with tools have, for a small model, inside a real conversation?

The first trial (`tool-call-trial.py`) settled how to ask for one call: few words, and "no tool" is one of the
tools. It asked single questions and let the model put its answer inside the JSON reply. A chat is different:
there is a conversation before the request, answers are long and have lines and code in them, and a tool's
result has to be used afterwards. This tries three shapes of turn on the models on this computer:

  one reply      Tools in the first message. Every reply is one JSON object; with no tool, the answer is inside it.
  decide, then   The model is first asked only "which tool, or none", in a few tokens. Then it writes the
  answer         answer as ordinary text. The tools are listed with the request (after it, or before it),
                 or in the first message of the conversation. The decision is either left out of the
                 conversation afterwards, or kept in it.

and counts, for each: decisions that are right, answers that can be used (ordinary text that says the thing,
not a stray tool call), the size of a turn's prompts in tokens, the time for a whole turn, and, after a tool's
result, whether the next step is right (another tool, or the answer). It then tries three ways of describing
the same tools. Small numbers: direction, not a benchmark.

    python3 core/dev/tool-turn-trial.py

The tools here are stand-ins for the trial (a file reader and a web search do not exist yet; their results
are made up). The tools Demido has, and their words, are in core/crates/demido-core/src/tools.rs.
TRIAL_MODELS=4 takes the four smallest models; TRIAL_ASK picks the wording of the question;
TRIAL_ONLY=<part of a row's name> runs only those rows; TRIAL_SHOW_WRONG=<part of a row's name> prints what went wrong.

Needs a runtime installed by Demido and GGUF models in its models folder. Downloads nothing."""
import glob, json, os, re, secrets, signal, subprocess, sys, tempfile, time, urllib.request, urllib.error

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models"); WORK = tempfile.mkdtemp(prefix="demido-turn-trial-")
PORT, KEY = 7457, secrets.token_hex(24); BASE = f"http://127.0.0.1:{PORT}"
notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"])); LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
MODEL_IDS = [os.path.basename(f)[:-5] for f in files][:int(os.environ.get("TRIAL_MODELS", "2"))]

INSTRUCTIONS = "Answer first. Then numbered steps, only if steps are needed. One idea per sentence. Exact values with units. No filler, no praise. Do not repeat the question."
# Three ways of describing the same tools. "plain" says what each does. "when" also says what the model cannot
# do without it: small models do not know their own limits. "example" is "when" with an example of use.
PLAIN = [("time", "", "The date and time now."),
         ("calculate", "expression: text", "Exact arithmetic."),
         ("read_file", "path: text", "The text of a file on this computer."),
         ("web_search", "query: text", "Search the web. For news, prices, anything recent."),
         ("plan", "steps: list of text", "Split work that needs several steps into a list.")]
WHEN = [("time", "", "The date and time now. You do not know them without it."),
        ("calculate", "expression: text", "Exact arithmetic. Use it for every sum; do not work sums out yourself."),
        ("read_file", "path: text", "The text of a file on this computer. You cannot see a file without it."),
        ("web_search", "query: text", "Search the web. For news, prices, anything recent or that you are not sure of."),
        ("plan", "steps: list of text", "For work that needs several steps: write the steps as a list first.")]
EXAMPLE = [t if t[0] != "calculate" else (t[0], t[1], t[2] + " Example: (17 - 5) * 1.2") for t in WHEN]
# Two more wordings for "plan", which the models rarely choose: said as a fact about the request, and the same listed first.
PARTS = [t if t[0] != "plan" else (t[0], t[1], "The request has several parts or needs several tools. List the steps first.") for t in WHEN]
PARTS_FIRST = [PARTS[-1]] + PARTS[:-1]
NAMES = [t[0] for t in PLAIN] + ["none"]
def listed(tools): return "\n".join(f"- {n}({a}): {d}" for n, a, d in tools) + "\n- none(): No tool is needed. Answer from what you know."
REPLY = "Reply with one JSON object, nothing else: {\"tool\": \"name\", \"args\": {...}}"
# The question itself, in three wordings. The first takes for granted that a tool is needed.
ASKS = {
    "which": ("Which tool does the request need? " + REPLY, "Above is the tool's result. Which tool is needed next? " + REPLY),
    "whether": ("Does the request need a tool? Most do not. " + REPLY, "Above is the tool's result. Is another tool needed? Mostly not. " + REPLY),
    "can you": ("Can you answer the request from what you know? Then the tool is \"none\". If not, which tool does it need? " + REPLY,
                "Above is the tool's result. Can you answer the request now? Then the tool is \"none\". If not, which tool is needed next? " + REPLY),
    # Both replies shown as forms, so that "no tool" is as visible as a tool.
    "forms": ("Does the request need a tool? Reply with one JSON object, nothing else.\nNeeds a tool: {\"tool\": \"name\", \"args\": {...}}\nNeeds no tool: {\"tool\": \"none\", \"args\": {}}",
              "Above is the tool's result. Is another tool needed? Reply with one JSON object, nothing else.\nAnother tool: {\"tool\": \"name\", \"args\": {...}}\nNo more tools: {\"tool\": \"none\", \"args\": {}}"),
    "forms, none first": ("Does the request need a tool? Reply with one JSON object, nothing else.\nNeeds no tool: {\"tool\": \"none\", \"args\": {}}\nNeeds a tool: {\"tool\": \"name\", \"args\": {...}}",
              "Above is the tool's result. Is another tool needed? Reply with one JSON object, nothing else.\nNo more tools: {\"tool\": \"none\", \"args\": {}}\nAnother tool: {\"tool\": \"name\", \"args\": {...}}"),
}
ASK, ASK_NEXT = ASKS[os.environ.get("TRIAL_ASK", "whether")]
USE_RESULT = "Above is the tool's result. Answer the request with it."
NO_TOOL, ANSWER_NOW = json.dumps({"tool": "none", "args": {}}), "Answer the request."
NAME_FORCED = {"type": "object", "properties": {"tool": {"enum": NAMES}, "args": {"type": "object"}}, "required": ["tool", "args"]}
ONE_FORCED = {"type": "object", "properties": {"tool": {"enum": NAMES}, "args": {"type": "object"}, "answer": {"type": "string"}}, "required": ["tool"]}

COOK = [("How long do I boil an egg for a soft yolk?", "Six minutes from boiling water. Cool it under cold water.")]
TRIP = [("I'm planning a trip to Lisbon in May with three friends.", "Good month for it. Book the flights early; May fills up.")]
FILE = [("My report is at /home/ada/report.md. I wrote it last night.", "Understood. Tell me what you want done with it.")]
PRICE = [("The laptop I like costs 1299 euros before tax.", "Noted: 1299 euros before tax.")]
RECIPE = [("Give me a quick pasta recipe.", "Boil 200 g spaghetti. Fry 100 g bacon. Mix with 2 eggs and 50 g cheese off the heat.")]
# A past turn that used a tool: (question, call, result, answer).
CLOCK = [("What time is it?", {"tool": "time", "args": {}}, "Saturday, 21 November 2026, 14:05", "It is 14:05 on Saturday, 21 November 2026.")]

# Each: what came before, the request, the tools that are right (None for "no tool"), words the arguments must
# hold, the tool's result (stood in), and words a usable answer must hold.
CASES = [
    (COOK,   "What's 18% of 2,340?",                                             ["calculate"], ["2340", "18"], "421.2", ["421.2"]),
    ([],     "What time is it?",                                                 ["time"], [], "Saturday, 21 November 2026, 14:05", ["14:05"]),
    ([],     "What's the date today?",                                           ["time"], [], "Saturday, 21 November 2026, 14:05", ["21", "november"]),
    (TRIP,   "If the flight is 437 euros each, how much for 4 of us plus one 35 euro bag?", ["calculate"], ["437", "4", "35"], "1783", ["1783"]),
    ([],     "Search the web for the latest llama.cpp release.",                 ["web_search"], ["llama.cpp"], "llama.cpp v1.0.0 (build b12359) was released on 14 November 2026.", ["1.0.0"]),
    ([],     "Read the file notes.txt and tell me what's in it.",                ["read_file"], ["notes.txt"], "Buy stamps. Call Mira about Friday. Renew the domain by 3 December.", ["mira"]),
    (COOK,   "How many seconds are in 3 days and 7 hours?",                      ["calculate"], ["3", "7"], "284400", ["284400", "284,400"]),
    ([],     "Divide 98765 by 432.",                                             ["calculate"], ["98765", "432"], "228.6227", ["228.6"]),
    (TRIP,   "What day of the week is it?",                                      ["time"], [], "Saturday, 21 November 2026, 14:05", ["saturday"]),
    (FILE,   "Open it and summarise it in one sentence.",                        ["read_file"], ["report.md"], "Q3 sales rose 12% to 4.1 million euros. Returns fell. The Lisbon shop opened in August.", ["12"]),
    (PRICE,  "What is that with 22% VAT added?",                                 ["calculate"], ["1299"], "1584.78", ["1584.78", "1,584.78"]),
    ([],     "Look up who won the 2026 World Cup.",                              ["web_search"], ["world cup"], "Spain won the 2026 FIFA World Cup, beating Argentina 2-1 in the final on 19 July 2026.", ["spain"]),
    ([],     "First find the current price of bitcoin, then convert 0.35 BTC to euros, then write me a note about it.", ["plan", "web_search"], [], None, None),
    ([],     "Do three things: read todo.txt, add up the hours in it, and tell me the total.", ["plan", "read_file"], [], None, None),
    ([],     "Compare the weather in Rome and Oslo today and tell me which is warmer.", ["plan", "web_search"], [], None, None),
    ([],     "Read a.txt and b.txt, then tell me which one is longer.",          ["plan", "read_file"], [], None, None),
    ([],     "Say hello in French.",                                             None, [], None, ["bonjour", "salut"]),
    (COOK,   "What is the capital of Italy?",                                    None, [], None, ["rome"]),
    (TRIP,   "Thanks, that's all.",                                              None, [], None, []),
    (CLOCK,  "Why is the sky blue? Two sentences.",                              None, [], None, ["scatter", "light"]),
    ([],     "Write a Python function that reverses a string.",                  None, [], None, ["def "]),
    (TRIP,   "Give me a packing list for a weekend hike, five items.",           None, [], None, ["\n"]),
    ([],     "What does 'ephemeral' mean?",                                      None, [], None, ["short", "brief", "last", "tempor"]),
    (RECIPE, "Make that vegetarian.",                                            None, [], None, ["spaghetti", "pasta", "bacon", "egg", "cheese"]),
    (CLOCK,  "Explain in two sentences what a time zone is.",                    None, [], None, ["zone", "region", "area"]),
    (PRICE,  "Is that a lot for a laptop? One sentence.",                        None, [], None, []),
]

# After a tool's result: is another tool needed, or is it time to answer? (the request, the call made, its
# result, the tools that are right next or None, words the next call's arguments must hold)
NEXT = [
    ("How many days are left until 25 December?", {"tool": "time", "args": {}}, "Saturday, 21 November 2026, 14:05", ["calculate"], ["25"]),
    ("Read prices.txt and add up the numbers in it.", {"tool": "read_file", "args": {"path": "prices.txt"}}, "Bread 2.40\nMilk 1.15\nCheese 6.80", ["calculate"], ["2.4", "1.15", "6.8"]),
    ("What time is it?", {"tool": "time", "args": {}}, "Saturday, 21 November 2026, 14:05", None, []),
    ("What's 18% of 2,340?", {"tool": "calculate", "args": {"expression": "2340 * 0.18"}}, "421.2", None, []),
    ("Search the web for the latest llama.cpp release.", {"tool": "web_search", "args": {"query": "latest llama.cpp release"}}, "llama.cpp v1.0.0 (build b12359) was released on 14 November 2026.", None, []),
    ("Read the file notes.txt and tell me what's in it.", {"tool": "read_file", "args": {"path": "notes.txt"}}, "Buy stamps. Call Mira about Friday. Renew the domain by 3 December.", None, []),
]

def system(way):
    if way["shape"] == "one": return INSTRUCTIONS + "\n\nTools:\n" + listed(way["tools"]) + "\nReply with one JSON object, nothing else.\nNeed a tool: {\"tool\": \"name\", \"args\": {...}}\nNo tool needed: {\"tool\": \"none\", \"answer\": \"text\"}"
    if way["shape"] in ("first", "kept"): return INSTRUCTIONS + "\n\nTools you can ask for:\n" + listed(way["tools"])
    return INSTRUCTIONS

def before(way, turns):
    """What came before the request, written the way this shape of turn would have left it."""
    out, one = [], way["shape"] == "one"
    for t in turns:
        if way["shape"] == "kept":
            # The decisions stay in the conversation, so every turn is the one before it with more at the end.
            out += [{"role": "user", "content": asked(way, t[0])}]
            if len(t) == 4: out += [{"role": "assistant", "content": json.dumps(t[1])}, {"role": "user", "content": f"<result>\n{t[2]}\n</result>\n\n{ASK_NEXT}"}]
            out += [{"role": "assistant", "content": NO_TOOL}, {"role": "user", "content": ANSWER_NOW}, {"role": "assistant", "content": t[-1]}]
        elif len(t) == 2:
            out += [{"role": "user", "content": t[0]}, {"role": "assistant", "content": json.dumps({"tool": "none", "answer": t[1]}) if one else t[1]}]
        else:
            q, call, result, answer = t
            out += [{"role": "user", "content": q}, {"role": "assistant", "content": json.dumps(call)}]
            out += [{"role": "user", "content": f"Result: {result}"}, {"role": "assistant", "content": json.dumps({"tool": "none", "answer": answer})}] if one else \
                   [{"role": "user", "content": f"<result>\n{result}\n</result>\n\n{USE_RESULT}"}, {"role": "assistant", "content": answer}]
    return out

def asked(way, q):
    """The request with the question of which tool it needs after it."""
    return ("Tools you can ask for:\n" + listed(way["tools"]) + "\n\n" if way["shape"] == "before" else "") + (f"<request>\n{q}\n</request>" if way["fenced"] else q) + "\n\n" + ("Tools:\n" + listed(way["tools"]) + "\n" if way["shape"] == "last" else "") + ASK

def decide_body(way, turns, q):
    return [{"role": "system", "content": system(way)}] + before(way, turns) + [{"role": "user", "content": q if way["shape"] == "one" else asked(way, q)}]

def answer_body(way, turns, q, call=None, result=None):
    """The request for the answer itself: after "no tool", or after a tool's result."""
    if way["shape"] == "kept":
        out = decide_body(way, turns, q)
        if call is not None: out += [{"role": "assistant", "content": json.dumps(call)}, {"role": "user", "content": f"<result>\n{result}\n</result>\n\n{ASK_NEXT}"}]
        return out + [{"role": "assistant", "content": NO_TOOL}, {"role": "user", "content": ANSWER_NOW}]
    out = [{"role": "system", "content": system(way)}] + before(way, turns) + [{"role": "user", "content": q}]
    if call is not None:
        out += [{"role": "assistant", "content": json.dumps(call)}, {"role": "user", "content": f"Result: {result}" if way["shape"] == "one" else f"<result>\n{result}\n</result>\n\n{USE_RESULT}"}]
    return out

def next_body(way, q, call, result):
    """After a tool's result, the question of what comes next."""
    tools = "Tools you can ask for:\n" + listed(way["tools"]) + "\n\n" if way["shape"] == "before" else ""
    return [{"role": "system", "content": system(way)}, {"role": "user", "content": asked(way, q) if way["shape"] == "kept" else q}, {"role": "assistant", "content": json.dumps(call)}, {"role": "user", "content": f"{tools}<result>\n{result}\n</result>\n\n{ASK_NEXT}"}]

def ask(path, body=None, timeout=180):
    req = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r: return r.status, json.loads(r.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e: return e.code, {"error": e.read().decode("utf-8", "replace")[:300]}

def say(model, messages, forced=None, most=400):
    body = {"model": model, "temperature": 0, "seed": 7, "max_tokens": most, "chat_template_kwargs": {"enable_thinking": False}, "reasoning_effort": "none", "messages": messages}
    if forced: body["response_format"] = {"type": "json_schema", "schema": forced}
    t0 = time.time(); code, res = ask("/v1/chat/completions", body); ms = (time.time() - t0) * 1000
    if code != 200: return "", 0, ms
    return res["choices"][0]["message"].get("content") or "", res.get("usage", {}).get("prompt_tokens", 0), ms

def read_json(text):
    start = (text or "").find("{")
    while start != -1:
        depth = 0
        for i in range(start, len(text)):
            depth += text[i] == "{"; depth -= text[i] == "}"
            if depth == 0:
                try: return json.loads(text[start:i + 1])
                except Exception: break
        start = text.find("{", start + 1)
    return None

def name_of(call):
    """The tool a reply names, read the way Demido's check would: "time()" and " Time " are "time"."""
    tool = call.get("tool") if isinstance(call, dict) else None
    return re.sub(r"\(.*\)$", "", tool.strip().lower()) if isinstance(tool, str) else None

def has(text, words): return not words or any(w in text.lower() for w in words)
def flat(v): return json.dumps(v).lower().replace(",", "") if v is not None else ""
def looks_like_a_call(text): return text.lstrip().startswith("{") or any(re.match(rf"\s*{n}\(", text) for n in NAMES)

def way(label, shape, forced=True, fenced=True, tools=WHEN): return dict(label=label, shape=shape, forced=forced, fenced=fenced, tools=tools)
SHAPES = [way("one reply", "one", forced=False), way("one reply, name forced", "one"),
          way("decide then answer: tools after the request", "last"), way("…tools before the request", "before"),
          way("…tools before it, nothing forced", "before", forced=False), way("…tools before it, request not fenced", "before", fenced=False),
          way("…tools in the first message", "first"), way("…in the first message, decisions kept", "kept", fenced=False)]
DESCRIBED = [way("what each tool does", "before", fenced=False, tools=PLAIN), way("…and what the model cannot do without it", "before", fenced=False, tools=WHEN),
             way("…and an example of use", "before", fenced=False, tools=EXAMPLE), way("…'plan' said as a fact about the request", "before", fenced=False, tools=PARTS),
             way("…and 'plan' listed first", "before", fenced=False, tools=PARTS_FIRST)]
wrongs = []

def turns_of(model, w):
    n = dict(right=0, tool=0, none=0, usable=0, answers=0, prompt=0, ms=0, planned=0, misplanned=0); one = w["shape"] == "one"
    for turns, q, want, arg_words, result, answer_words in CASES:
        # 1. The decision.
        text, prompt, ms = say(model, decide_body(w, turns, q), (ONE_FORCED if one else NAME_FORCED) if w["forced"] else None, most=400 if one else 160)
        call = read_json(text); tool = name_of(call)
        # In "one reply", a reply that is not JSON at all is read as the answer: the kindest reading of that shape.
        prose = one and call is None and bool(text.strip())
        if want is None:
            ok = prose or (tool == "none" and (not one or (isinstance(call.get("answer"), str) and bool(call["answer"].strip()))))
            n["none"] += ok
        else:
            ok = tool in want and all(x in flat(call.get("args")) for x in (arg_words if tool == want[0] else []))
            n["tool"] += ok
        n["right"] += ok
        parts = bool(want) and "plan" in want
        n["planned"] += parts and tool == "plan"; n["misplanned"] += tool == "plan" and not parts
        if not ok: wrongs.append((model[:10], w["label"], q[:44], text.strip().replace("\n", " ")[:120]))
        # 2. The answer, measured apart from the decision: after the right call and a stood-in result, or after "no tool".
        if answer_words is not None:
            n["answers"] += 1; text2 = text
            if want is None and one:
                said, prompt2, ms2 = (text if prose else call.get("answer") if isinstance(call, dict) and isinstance(call.get("answer"), str) else ""), 0, 0
            else:
                text2, prompt2, ms2 = say(model, answer_body(w, turns, q, None if want is None else {"tool": want[0], "args": {}}, result))
                if one:
                    j = read_json(text2); said = text2 if j is None else j.get("answer") if isinstance(j.get("answer"), str) and name_of(j) in (None, "none") else ""
                else:
                    said = "" if looks_like_a_call(text2) else text2
            usable = bool((said or "").strip()) and has(said, answer_words)
            n["usable"] += usable
            if not usable: wrongs.append((model[:10], w["label"] + " (answer)", q[:44], text2.strip().replace("\n", " ⏎ ")[:120]))
            prompt += prompt2; ms += ms2
        n["prompt"] += prompt; n["ms"] += ms
    return n

def next_of(model, w):
    right = 0
    for q, call, result, want, arg_words in NEXT:
        text, _, _ = say(model, next_body(w, q, call, result), NAME_FORCED, most=160)
        got = read_json(text); tool = name_of(got)
        ok = tool == "none" if want is None else tool in want and all(x in flat(got.get("args")) for x in arg_words)
        right += ok
        if not ok: wrongs.append((model[:10], w["label"] + " (next)", q[:44], text.strip().replace("\n", " ")[:120]))
    return right

keyfile = os.path.join(WORK, "key.txt"); open(keyfile, "w").write(KEY + "\n"); os.chmod(keyfile, 0o600)
router = subprocess.Popen([os.path.join(RT, "llama-server"), "--models-dir", MODELS, "--host", "127.0.0.1", "--port", str(PORT), "--api-key-file", keyfile, "--no-models-autoload", "--no-webui",
                           "--models-max", "0", "--models-memory-margin", "900", "-c", "4096", "--log-file", os.path.join(WORK, "router.log")],
                          cwd=RT, env=dict(os.environ, LD_LIBRARY_PATH=LIBS + ":" + os.environ.get("LD_LIBRARY_PATH", "")), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(200):
        try:
            if urllib.request.urlopen(BASE + "/health", timeout=1).status == 200: break
        except Exception: time.sleep(0.05)
    N = len(CASES); need = sum(1 for c in CASES if c[2]); none = N - need
    print(f"runtime: {note['name']} ({note['build']}); {N} requests inside conversations: {need} need a tool, {none} need none; temperature 0; thinking off\n")
    for model in MODEL_IDS:
        ask("/models/load", {"model": model})
        for _ in range(600):
            code, b = ask("/models")
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in b.get("data", [])): break
            time.sleep(0.2)
        print(f"## {model}")
        for title, ways in [("shape of turn", SHAPES), ("tools before the request, described by", DESCRIBED)]:
            print(f"{title:42} {'decided right':>14} {'tool':>6} {'no tool':>8} {'answer usable':>14} {'prompt':>7} {'turn':>7} {'next step':>10} {'planned':>8} {'wrongly':>8}")
            for w in ways:
                if os.environ.get("TRIAL_ONLY") and os.environ["TRIAL_ONLY"] not in w["label"]: continue
                n = turns_of(model, w)
                nxt = f"{next_of(model, w)} of {len(NEXT)}" if w["shape"] in ("first", "kept", "before") else ""
                print(f"{w['label']:42} {n['right']:>8} of {N} {n['tool']:>3}/{need} {n['none']:>5}/{none} {n['usable']:>8} of {n['answers']} {n['prompt']//N:>7} {n['ms']/N:>5.0f}ms {nxt:>10} {n['planned']:>3} of {sum(1 for c in CASES if c[2] and 'plan' in c[2])} {n['misplanned']:>8}")
            print()
        ask("/models/unload", {"model": model}); time.sleep(0.5)
    show = os.environ.get("TRIAL_SHOW_WRONG")
    for w in wrongs:
        if show and show in w[1]: print("  " + " | ".join(w))
finally:
    router.send_signal(signal.SIGTERM)
    try: router.wait(timeout=15)
    except subprocess.TimeoutExpired: router.kill()
