#!/usr/bin/env python3
"""Two questions Demido's rule for agents rests on (docs/DECISIONS.md O4; research note 21).

O4: "work that the task list marks as mechanical and not needing the conversation goes to the fastest model
that is already loaded or fits beside the main one." That needs two things to be true:

  1. telling   Can a small model tell a mechanical task from one that needs judgement or the conversation?
               Each model is shown a request and one of its tasks, and asked, three ways: is it mechanical;
               does it need judgement (most do); is it copying or thinking work. Held to the answer's
               shape. Twelve tasks with known answers.
  2. doing     Does the fastest model, given only the task (a fresh context), do mechanical work as right as
               a bigger one? Ten mechanical tasks with answers that can be checked exactly, on every model:
               on a fresh context, and again at the end of a conversation about something else, as a
               session's task arrives today. Does a fresh context help the same model?

Temperature 0, thinking off. Small numbers: direction, not a benchmark.

    python3 core/dev/mechanical-trial.py      TRIAL_SHOW_WRONG=1 prints what went wrong

Needs a runtime installed by Demido and GGUF models in its models folder. Downloads nothing."""
import glob, json, os, re, secrets, signal, subprocess, sys, tempfile, time, urllib.request, urllib.error

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models"); WORK = tempfile.mkdtemp(prefix="demido-mechanical-trial-")
PORT, KEY = 7463, secrets.token_hex(24); BASE = f"http://127.0.0.1:{PORT}"
notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"])); LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
MODEL_IDS = [os.path.basename(f)[:-5] for f in files][:int(os.environ.get("TRIAL_MODELS", "4"))]

# 1. Telling: (request, task, mechanical?)
TELL = [
    ("Plan my week from the notes in notes.txt.", "Pull every date out of notes.txt.", True),
    ("Plan my week from the notes in notes.txt.", "Decide which of the deadlines matters most.", False),
    ("Write to Sam about the delay we discussed.", "Write the e-mail to Sam in the tone we agreed on earlier.", False),
    ("Tidy up this CSV for the report.", "Sort the rows by the second column.", True),
    ("Tidy up this CSV for the report.", "Rename the columns to snake_case.", True),
    ("Tidy up this CSV for the report.", "Choose which columns the report should show.", False),
    ("Find out why the build fails.", "Read the last 50 lines of build.log.", True),
    ("Find out why the build fails.", "Work out from the log what caused the failure.", False),
    ("Translate my letter to Italian.", "Translate the letter to Italian, keeping its warmth.", False),
    ("Make a list of all the books in these notes.", "Copy every book title out of the notes, one a line.", True),
    ("Get these invoices ready.", "Add up the totals of the five invoices.", True),
    ("Get these invoices ready.", "Say which invoice looks wrong, and why.", False),
]
SHOWN = "A request, and one task of the plan for it:\n<request>\n{r}\n</request>\n<task>\n{t}\n</task>\n\n"
# Three wordings of the question: asked as "mechanical?", asked the other way round ("needs judgement?", most
# do), and as a choice between two kinds of work. Each answer is turned into "mechanical or not".
TELL_ASKS = {
    "mechanical?": (SHOWN + "Is the task mechanical: something a quick helper could do from the task alone, with no judgement and nothing of the conversation? "
                    "Reply with one JSON object: {{\"mechanical\": true}} or {{\"mechanical\": false}}",
                    {"type": "object", "properties": {"mechanical": {"type": "boolean"}}, "required": ["mechanical"]}, lambda j: j.get("mechanical")),
    "judgement?": (SHOWN + "Does the task need judgement, taste, or anything said earlier in the conversation? Most tasks do. "
                   "Reply with one JSON object: {{\"needs_judgement\": true}} or {{\"needs_judgement\": false}}",
                   {"type": "object", "properties": {"needs_judgement": {"type": "boolean"}}, "required": ["needs_judgement"]}, lambda j: None if j.get("needs_judgement") is None else not j["needs_judgement"]),
    "which kind": (SHOWN + "Which kind of work is the task?\n- copying: moving, sorting, counting or reformatting what is already there\n- thinking: deciding, judging, writing in a voice, working out a cause\n"
                   "Reply with one JSON object: {{\"kind\": \"copying\"}} or {{\"kind\": \"thinking\"}}",
                   {"type": "object", "properties": {"kind": {"enum": ["copying", "thinking"]}}, "required": ["kind"]}, lambda j: None if j.get("kind") is None else j["kind"] == "copying"),
}

# 2. Doing: (task, check)
def lines_are(*want): return lambda out: [l.strip(" -*•\t") for l in out.strip().splitlines() if l.strip()] == list(want)
def has_all(*words): return lambda out: all(w.lower() in out.lower() for w in words)
def number_is(n): return lambda out: re.search(rf"(?<![\d.]){re.escape(str(n))}(?![\d.])", out.replace(",", "")) is not None
DO = [
    ("Sort these words alphabetically, one a line, nothing else: pear, apple, mango, banana, cherry", lines_are("apple", "banana", "cherry", "mango", "pear")),
    ("Write these names on one line, separated by commas, nothing else: Ana\nBo\nCleo\nDev", lambda o: o.strip().rstrip(".") in ("Ana, Bo, Cleo, Dev", "Ana,Bo,Cleo,Dev")),
    ("Copy every e-mail address out of this text, one a line, nothing else: Ask ana@example.com or the desk (desk@shop.example), not bo at example dot com.", lines_are("ana@example.com", "desk@shop.example")),
    ("Rewrite each date as YYYY-MM-DD, one a line, nothing else: 21/11/2026, 03/01/2027, 15/08/2026", lines_are("2026-11-21", "2027-01-03", "2026-08-15")),
    ("How many lines of this log say ERROR? Reply with the number only.\nINFO start\nERROR disk\nWARN slow\nERROR net\nINFO done\nERROR disk", number_is(3)),
    ("Make each line start with a capital letter, nothing else:\nthe sea\nthe sky\nthe sand", lines_are("The sea", "The sky", "The sand")),
    ("Add up these amounts and reply with the total only: 12.50, 7.25, 30.00, 0.75", number_is(50.5)),
    ("Turn this into JSON with the keys name and age, nothing else: Ada is 36.", lambda o: (lambda j: j is not None and j.get("name") == "Ada" and j.get("age") in (36, "36"))(first_json(o))),
    ("Write the words of this sentence in reverse order, nothing else: one two three four", lambda o: o.strip().rstrip(".").lower() == "four three two one"),
    ("Remove the duplicates from this list and keep the order, one a line, nothing else: red, blue, red, green, blue", lines_are("red", "blue", "green")),
]

# A conversation the task arrives at the end of, as a session's task would: about something else, long enough to matter.
BEFORE = [
    {"role": "user", "content": "I'm planning a week in Lisbon in March. What should I see first?"},
    {"role": "assistant", "content": "Start with Alfama: the castle, the viewpoints at Santa Luzia and Graça, and the tram 28 line. Belém is a half day: the tower, the monastery, and the pastéis. Keep a day for Sintra; go early, the palaces fill by ten. In the evening, Bairro Alto for food and fado in Alfama."},
    {"role": "user", "content": "How do I get from the airport to the centre, and is a transport card worth it?"},
    {"role": "assistant", "content": "The metro red line runs from the airport to the centre in about 25 minutes, with one change for most hotels. Get a Navegante card: you load it with single trips or a 24-hour pass. If you plan Sintra and Cascais, the 24-hour pass that includes the trains pays off on those days."},
    {"role": "user", "content": "Any food I shouldn't miss, and where?"},
    {"role": "assistant", "content": "Bacalhau à Brás, grilled sardines (in March they are not at their best; try them anyway in a tasca), bifana sandwiches, and ginjinha, a cherry liqueur sold by the glass near Rossio. The Time Out market is easy but busy; the tascas in Mouraria are cheaper and better."},
]

def first_json(text):
    m = re.search(r"\{.*\}", text or "", re.S)
    try: return json.loads(m.group(0)) if m else None
    except Exception: return None

def ask(path, body=None, timeout=180):
    req = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r: return r.status, json.loads(r.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e: return e.code, {"error": e.read().decode("utf-8", "replace")[:300]}

def say(model, content, schema=None, most=200, before=()):
    body = {"model": model, "temperature": 0, "seed": 7, "max_tokens": most, "chat_template_kwargs": {"enable_thinking": False}, "reasoning_effort": "none",
            "messages": list(before) + [{"role": "user", "content": content}]}
    if schema: body["response_format"] = {"type": "json_schema", "schema": schema}
    t0 = time.time(); code, res = ask("/v1/chat/completions", body); ms = (time.time() - t0) * 1000
    return (res["choices"][0]["message"].get("content") or "") if code == 200 else "", ms

keyfile = os.path.join(WORK, "key.txt"); open(keyfile, "w").write(KEY + "\n"); os.chmod(keyfile, 0o600)
router = subprocess.Popen([os.path.join(RT, "llama-server"), "--models-dir", MODELS, "--host", "127.0.0.1", "--port", str(PORT), "--api-key-file", keyfile, "--no-models-autoload", "--no-webui",
                           "--models-max", "0", "--models-memory-margin", "900", "-c", "4096", "--log-file", os.path.join(WORK, "router.log")],
                          cwd=RT, env=dict(os.environ, LD_LIBRARY_PATH=LIBS + ":" + os.environ.get("LD_LIBRARY_PATH", "")), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
wrongs = []
try:
    for _ in range(200):
        try:
            if urllib.request.urlopen(BASE + "/health", timeout=1).status == 200: break
        except Exception: time.sleep(0.05)
    mech = sum(1 for *_, m in TELL if m)
    print(f"runtime: {note['name']} ({note['build']}); temperature 0; thinking off")
    print(f"telling: {len(TELL)} tasks, {mech} mechanical; doing: {len(DO)} mechanical tasks with exact answers, each on a fresh context\n")
    print(f"{'model':34} {'asked':12} {'told right':>11} {'mechanical':>11} {'not':>5} {'done fresh':>11} {'a task':>8} {'in a conversation':>18}")
    for model in MODEL_IDS:
        ask("/models/load", {"model": model})
        for _ in range(600):
            code, b = ask("/models")
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in b.get("data", [])): break
            time.sleep(0.2)
        tellings = {}
        for wording, (asked, schema, read) in TELL_ASKS.items():
            told = []
            for r, t, want in TELL:
                out, _ = say(model, asked.format(r=r, t=t), schema, 20)
                got = read(first_json(out) or {})
                told.append((want, got))
                if got != want: wrongs.append((model[:12], f"telling ({wording})", t[:50], f"said mechanical={got}"))
            tellings[wording] = told
        done, ms = 0, 0.0
        for task, check in DO:
            out, took = say(model, task)
            ms += took
            ok = bool(check(out))
            done += ok
            if not ok: wrongs.append((model[:12], "doing", task[:50], out.strip().replace("\n", " | ")[:80]))
        # The same tasks at the end of a conversation about something else, as a session's task arrives now.
        inside = 0
        for task, check in DO:
            out, _ = say(model, task, before=BEFORE)
            ok = bool(check(out))
            inside += ok
            if not ok: wrongs.append((model[:12], "doing, in a conversation", task[:50], out.strip().replace("\n", " | ")[:80]))
        for wording, told in tellings.items():
            right = sum(1 for w, g in told if w == g)
            doing = f"{done:>5} of {len(DO)} {ms / len(DO):>6.0f}ms {inside:>5} of {len(DO)}" if wording == "mechanical?" else ""
            print(f"{model:34} {wording:12} {right:>5} of {len(TELL)} {sum(1 for w, g in told if w and g):>7}/{mech} {sum(1 for w, g in told if not w and g is False):>3}/{len(TELL) - mech} {doing}", flush=True)
        ask("/models/unload", {"model": model}); time.sleep(0.5)
    if os.environ.get("TRIAL_SHOW_WRONG"):
        for w in wrongs: print("  " + " | ".join(str(x) for x in w))
finally:
    router.send_signal(signal.SIGTERM)
    try: router.wait(timeout=15)
    except subprocess.TimeoutExpired: router.kill()
