#!/usr/bin/env python3
"""How should Demido ask a small model for a tool call? A small measurement on the models on this computer.

Recent papers disagree with the obvious answer ("force the schema"): for models under 3B, a hard schema makes
every reply valid and can make fewer of them right. This tries five ways of asking on 26 small requests
(20 that need one of four tools, 6 that need none) and counts, for each: replies that can be read, replies
that are right, tokens, and time. Small numbers: it shows direction, it is not a benchmark.

    python3 core/dev/tool-call-trial.py

Needs a runtime installed by Demido and GGUF models in its models folder. Downloads nothing."""
import glob, json, os, re, secrets, signal, subprocess, sys, tempfile, time, urllib.request, urllib.error

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models")
WORK = tempfile.mkdtemp(prefix="demido-tool-trial-")
PORT, KEY = 7451, secrets.token_hex(24)
BASE = f"http://127.0.0.1:{PORT}"

notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"]))
LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
if not files: sys.exit(f"No GGUF model in {MODELS}.")
MODEL_IDS = [os.path.basename(f)[:-5] for f in files][:int(os.environ.get("TRIAL_MODELS", "2"))]

TOOLS = {
    "get_weather": {"desc": "Current weather for a city.", "args": {"city": {"type": "string"}, "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}}},
    "set_timer": {"desc": "Start a timer.", "args": {"minutes": {"type": "integer"}, "label": {"type": "string"}}},
    "add_task": {"desc": "Add a task to the task list.", "args": {"title": {"type": "string"}, "priority": {"type": "string", "enum": ["low", "normal", "high"]}}},
    "search_files": {"desc": "Search for files by text.", "args": {"query": {"type": "string"}, "folder": {"type": "string"}}},
}
# (what the person says, the tool it needs or None, what each argument must be: exact for numbers and choices, "contains" for text)
TASKS = [
    ("What's the weather in Rome in celsius?", "get_weather", {"city": "rome", "unit": "celsius"}),
    ("Weather in New York, fahrenheit please.", "get_weather", {"city": "new york", "unit": "fahrenheit"}),
    ("Is it raining in Tokyo? Use celsius.", "get_weather", {"city": "tokyo", "unit": "celsius"}),
    ("Tell me the temperature in Buenos Aires in fahrenheit.", "get_weather", {"city": "buenos aires", "unit": "fahrenheit"}),
    ("How hot is it in Cairo right now? Celsius.", "get_weather", {"city": "cairo", "unit": "celsius"}),
    ("Set a timer for 10 minutes for the pasta.", "set_timer", {"minutes": 10, "label": "pasta"}),
    ("Timer: 45 minutes, label it laundry.", "set_timer", {"minutes": 45, "label": "laundry"}),
    ("Wake me in 5 minutes, call it nap.", "set_timer", {"minutes": 5, "label": "nap"}),
    ("Start a 90 minute timer named deep work.", "set_timer", {"minutes": 90, "label": "deep work"}),
    ("Remind me in 25 minutes: stretch.", "set_timer", {"minutes": 25, "label": "stretch"}),
    ("Add a task: buy milk, low priority.", "add_task", {"title": "buy milk", "priority": "low"}),
    ("Create a high priority task to call the bank.", "add_task", {"title": "call the bank", "priority": "high"}),
    ("Add 'renew passport' to my tasks with normal priority.", "add_task", {"title": "renew passport", "priority": "normal"}),
    ("New task, high priority: fix the login bug.", "add_task", {"title": "fix the login bug", "priority": "high"}),
    ("Put 'water the plants' on the task list, low priority.", "add_task", {"title": "water the plants", "priority": "low"}),
    ("Find files about invoices in the Documents folder.", "search_files", {"query": "invoice", "folder": "documents"}),
    ("Search the Downloads folder for 'tax 2025'.", "search_files", {"query": "tax 2025", "folder": "downloads"}),
    ("Look in Projects for files mentioning kubernetes.", "search_files", {"query": "kubernetes", "folder": "projects"}),
    ("Search for 'meeting notes' in the Work folder.", "search_files", {"query": "meeting notes", "folder": "work"}),
    ("In the Photos folder, find files named sunset.", "search_files", {"query": "sunset", "folder": "photos"}),
    ("What is 7 times 8?", None, {}),
    ("Say hello in French.", None, {}),
    ("What colour do you get when you mix blue and yellow?", None, {}),
    ("Thanks, that's all.", None, {}),
    ("What is the capital of Italy?", None, {}),
    ("Spell the word 'timer' backwards.", None, {}),
]

def lines():
    return "\n".join(f"- {n}({', '.join(k + ': ' + ('|'.join(v['enum']) if 'enum' in v else v['type']) for k, v in t['args'].items())}): {t['desc']}" for n, t in TOOLS.items())

SHORT = ("Tools:\n" + lines() + "\nReply with one JSON object, nothing else.\nNeed a tool: {\"tool\": \"name\", \"args\": {...}}\nNo tool needed: {\"answer\": \"text\"}")
LONG = ("You are a helpful, friendly and knowledgeable assistant. You have access to a number of tools that you may use whenever you believe they would help you "
        "to fulfil what the user is asking for. Please think carefully about whether a tool is really necessary before you decide to use one, because many questions "
        "can simply be answered from your own general knowledge without calling anything at all. The tools that are available to you in this conversation are listed "
        "below, each with its name, the arguments that it accepts together with their types or their permitted values, and a short description of what it does:\n" + lines() +
        "\nWhen you would like to use one of these tools, you must respond with a single JSON object and absolutely nothing else before or after it, in exactly this form: "
        "{\"tool\": \"name\", \"args\": {...}} where name is the name of the tool and args contains every argument. When no tool is needed, you must instead respond with a "
        "single JSON object of the form {\"answer\": \"text\"} containing your answer to the user. Do not add explanations, greetings, markdown formatting or code fences.")
NATIVE_SYSTEM = "Use a tool when the request needs one. Otherwise answer directly."

def native_tools():
    return [{"type": "function", "function": {"name": n, "description": t["desc"], "parameters": {"type": "object", "properties": t["args"], "required": list(t["args"])}}} for n, t in TOOLS.items()]

def hard_schema():
    one = [{"type": "object", "properties": {"tool": {"const": n}, "args": {"type": "object", "properties": t["args"], "required": list(t["args"]), "additionalProperties": False}}, "required": ["tool", "args"], "additionalProperties": False} for n, t in TOOLS.items()]
    one.append({"type": "object", "properties": {"answer": {"type": "string"}}, "required": ["answer"], "additionalProperties": False})
    return {"oneOf": one}

def hard_none_schema():
    # Everything forced, and "no tool" is one of the choices of the same shape as the others.
    one = [{"type": "object", "properties": {"tool": {"const": n}, "args": {"type": "object", "properties": t["args"], "required": list(t["args"]), "additionalProperties": False}}, "required": ["tool", "args"], "additionalProperties": False} for n, t in TOOLS.items()]
    one.append({"type": "object", "properties": {"tool": {"const": "none"}, "answer": {"type": "string"}}, "required": ["tool", "answer"], "additionalProperties": False})
    return {"oneOf": one}

def light_schema():
    # Only the choice of tool is forced to be one that exists. The arguments are left to the model.
    return {"type": "object", "properties": {"tool": {"enum": list(TOOLS) + ["none"]}, "args": {"type": "object"}, "answer": {"type": "string"}}, "required": ["tool"]}
LIGHT_SHORT = SHORT.replace("No tool needed: {\"answer\": \"text\"}", "No tool needed: {\"tool\": \"none\", \"answer\": \"text\"}")

def ask(path, body=None, method="POST", timeout=180):
    req = urllib.request.Request(BASE + path, method=method if body is not None else "GET", data=None if body is None else json.dumps(body).encode())
    req.add_header("Authorization", f"Bearer {KEY}"); req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r: return r.status, json.loads(r.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e:
        return e.code, {"error": e.read().decode("utf-8", "replace")[:300]}

def read_json(text):
    """The first JSON object in a reply, tolerating code fences and words around it. None if there is none."""
    text = re.sub(r"<think>.*?</think>", "", text or "", flags=re.S)
    start = text.find("{")
    while start != -1:
        depth = 0
        for i in range(start, len(text)):
            depth += text[i] == "{"; depth -= text[i] == "}"
            if depth == 0:
                try: return json.loads(text[start:i + 1])
                except Exception: break
        start = text.find("{", start + 1)
    return None

def schema_problem(call):
    """What is wrong with a tool call, judged only by the tools' own descriptions (as Demido can, without knowing the right answer)."""
    if not isinstance(call, dict): return "the reply is not a JSON object"
    tool = call.get("tool")
    if tool in (None, "none"): return None if isinstance(call.get("answer"), str) and call["answer"].strip() else "there is neither a tool nor an answer"
    if tool not in TOOLS: return f"there is no tool called \"{tool}\". The tools are: {', '.join(TOOLS)}"
    args = call.get("args")
    if not isinstance(args, dict): return "\"args\" must be an object"
    for name, spec in TOOLS[tool]["args"].items():
        if name not in args: return f"the argument \"{name}\" is missing"
        v = args[name]
        if spec["type"] == "integer" and not (isinstance(v, int) and not isinstance(v, bool)): return f"\"{name}\" must be a whole number, not {json.dumps(v)}"
        if spec["type"] == "string" and not isinstance(v, str): return f"\"{name}\" must be text"
        if "enum" in spec and v not in spec["enum"]: return f"\"{name}\" must be one of: {', '.join(spec['enum'])}. It was {json.dumps(v)}"
    extra = [k for k in args if k not in TOOLS[tool]["args"]]
    return f"\"{extra[0]}\" is not an argument of {tool}" if extra else None

def judge(call, want_tool, want_args):
    """(readable, right tool, right arguments) against what the request really needed."""
    if call is None: return False, False, False
    tool = call.get("tool") if isinstance(call, dict) else None
    if tool in (None, "none"):
        answered = isinstance(call.get("answer"), str) and bool(call["answer"].strip())
        return answered, want_tool is None and answered, want_tool is None and answered
    if tool != want_tool: return True, False, False
    args = call.get("args") if isinstance(call.get("args"), dict) else {}
    ok = True
    for k, want in want_args.items():
        got = args.get(k)
        ok &= (got == want) if isinstance(want, int) else (isinstance(got, str) and want in got.lower())
    return True, True, ok

def run(model, mode, text, think=False):
    body = {"model": model, "temperature": 0, "seed": 7, "max_tokens": 700 if think else 220, "chat_template_kwargs": {"enable_thinking": think}}
    if mode == "native":
        body |= {"messages": [{"role": "system", "content": NATIVE_SYSTEM}, {"role": "user", "content": text}], "tools": native_tools()}
    else:
        system = LONG if mode == "json-long" else LIGHT_SHORT if mode in ("light", "json-none", "hard-none") else SHORT
        body |= {"messages": [{"role": "system", "content": system}, {"role": "user", "content": text}]}
        if mode == "hard": body["response_format"] = {"type": "json_schema", "schema": hard_schema()}
        if mode == "light": body["response_format"] = {"type": "json_schema", "schema": light_schema()}
        if mode == "hard-none": body["response_format"] = {"type": "json_schema", "schema": hard_none_schema()}
    t0 = time.time(); code, res = ask("/v1/chat/completions", body); ms = (time.time() - t0) * 1000
    if code != 200: return None, 0, 0, ms, body, str(res)[:200]
    msg = res["choices"][0]["message"]; usage = res.get("usage", {})
    if mode == "native":
        calls = msg.get("tool_calls") or []
        if calls:
            try: call = {"tool": calls[0]["function"]["name"], "args": json.loads(calls[0]["function"]["arguments"] or "{}")}
            except Exception: call = None
        else: call = {"answer": msg.get("content") or ""}
    else: call = read_json(msg.get("content") or "")
    return call, usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0), ms, body, (msg.get("content") or "")[:160]

def repair(model, body, said, problem):
    """One more turn: tell the model what was wrong with its reply, in plain words, and let it try again."""
    body = dict(body); body["messages"] = body["messages"] + [{"role": "assistant", "content": said or "(nothing)"}, {"role": "user", "content": f"That reply cannot be used: {problem}. Reply again with only the JSON object."}]
    t0 = time.time(); code, res = ask("/v1/chat/completions", body); ms = (time.time() - t0) * 1000
    if code != 200: return None, 0, ms
    return read_json(res["choices"][0]["message"].get("content") or ""), res.get("usage", {}).get("completion_tokens", 0), ms

keyfile = os.path.join(WORK, "key.txt"); open(keyfile, "w").write(KEY + "\n"); os.chmod(keyfile, 0o600)
router = subprocess.Popen([os.path.join(RT, "llama-server"), "--models-dir", MODELS, "--host", "127.0.0.1", "--port", str(PORT), "--api-key-file", keyfile, "--no-models-autoload", "--no-webui",
                           "--models-max", "0", "--models-memory-margin", "900", "-c", "4096", "--log-file", os.path.join(WORK, "router.log")],
                          cwd=RT, env=dict(os.environ, LD_LIBRARY_PATH=LIBS + ":" + os.environ.get("LD_LIBRARY_PATH", "")), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(200):
        try:
            if urllib.request.urlopen(BASE + "/health", timeout=1).status == 200: break
        except Exception: time.sleep(0.05)
    print(f"runtime: {note['name']} ({note['build']}); {len(TASKS)} requests, {sum(1 for t in TASKS if t[1])} needing a tool; temperature 0; thinking off unless said\n")
    results = {}
    for model in MODEL_IDS:
        ask("/models/load", {"model": model})
        for _ in range(600):
            code, body = ask("/models")
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in body.get("data", [])): break
            time.sleep(0.2)
        print(f"## {model}")
        print(f"{'way of asking':34} {'readable':>8} {'right':>6} {'tool':>5} {'args':>5} {'no-tool':>8} {'prompt':>7} {'reply':>6} {'time':>7}")
        for label, mode, think, fix in [("native tools", "native", False, False), ("native tools, thinking on", "native", True, False), ("JSON, asked in few words", "json", False, False),
                                        ("JSON, asked in many words", "json-long", False, False), ("JSON + check and one repair", "json", False, True),
                                        ("schema forced: everything", "hard", False, False),
                                        ("'none' is a tool; nothing forced", "json-none", False, False), ("'none' is a tool; + one repair", "json-none", False, True),
                                        ("'none' is a tool; name forced", "light", False, False), ("'none' is a tool; all forced", "hard-none", False, False)]:
            n = dict(read=0, right=0, tool=0, args=0, none=0, pt=0, ct=0, ms=0, repaired=0, wrongs=[])
            for text, want_tool, want_args in TASKS:
                call, pt, ct, ms, body, said = run(model, mode, text, think)
                if fix:
                    problem = "it is not a JSON object" if call is None else schema_problem(call)
                    if problem:
                        call, ct2, ms2 = repair(model, body, said, problem); ct += ct2; ms += ms2; n["repaired"] += 1
                r, t, a = judge(call, want_tool, want_args)
                n["read"] += r; n["tool"] += t; n["args"] += a; n["right"] += (t and a); n["none"] += (want_tool is None and t); n["pt"] += pt; n["ct"] += ct; n["ms"] += ms
                if not (t and a): n["wrongs"].append((text[:38], json.dumps(call)[:110] if call else said[:110]))
            N = len(TASKS); none_n = sum(1 for t in TASKS if t[1] is None)
            print(f"{label:34} {n['read']:>5}/{N} {n['right']:>3}/{N} {n['tool']:>5} {n['args']:>5} {n['none']:>5}/{none_n} {n['pt']//N:>7} {n['ct']//N:>6} {n['ms']/N:>5.0f}ms" + (f"   ({n['repaired']} repaired)" if fix else ""))
            results[(model, label)] = n
        ask("/models/unload", {"model": model}); time.sleep(0.5); print()
    if os.environ.get("TRIAL_SHOW_WRONG"):
        for (model, label), n in results.items():
            for text, got in n["wrongs"][:4]: print(f"  {model[:14]} | {label[:26]:26} | {text:38} -> {got}")
finally:
    router.send_signal(signal.SIGTERM)
    try: router.wait(timeout=15)
    except subprocess.TimeoutExpired: router.kill()
