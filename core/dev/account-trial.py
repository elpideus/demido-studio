#!/usr/bin/env python3
"""How should Demido ask a small model for an account of a conversation?

Asked the obvious way, a small model answers the conversation's last question in place of writing the account,
and facts from the beginning are lost. This tries several ways of asking, on the models on this computer,
with three conversations, and counts for each: how many of the facts that matter are in the account, and
whether the model wrote an account at all or went on with the conversation.

    python3 core/dev/account-trial.py

Needs a runtime installed by Demido and GGUF models in its models folder. Downloads nothing."""
import glob, json, os, secrets, signal, subprocess, sys, tempfile, time, urllib.request, urllib.error

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models"); WORK = tempfile.mkdtemp(prefix="demido-account-trial-")
PORT, KEY = 7455, secrets.token_hex(24); BASE = f"http://127.0.0.1:{PORT}"
notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"])); LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
MODEL_IDS = [os.path.basename(f)[:-5] for f in files][:2]

# Three conversations. Each: the turns, the facts an account must keep, and words that give away that the
# model went on with the conversation (answered its last request) and did not write an account.
TALKS = [
    dict(turns=[("Person", "My name is Ada. I am planning a trip to Lisbon in May with a budget of 900 euros. What should I see?"),
                ("Model", "See the Belem Tower, the Alfama quarter and the tile museum. Take tram 28 once. A day trip to Sintra is worth it."),
                ("Person", "Good. I do not eat meat. What should I eat there?"),
                ("Model", "Try grilled sardines, bacalhau, pasteis de nata and caldo verde without the sausage."),
                ("Person", "And in about 150 words, where should I stay?")],
         facts=["ada", "lisbon", "may", "900", "meat"], gone_on=["i recommend staying", "you should stay", "consider staying", "great places to stay", "hostel", "hotel"]),
    dict(turns=[("Person", "I am fixing the login page of our shop. The bug is that the session cookie expires after 5 minutes. We use Django 5."),
                ("Model", "Check SESSION_COOKIE_AGE in settings.py. The default is two weeks, so something sets it to 300."),
                ("Person", "Found it: a colleague set it to 300 for testing. We decided to make it 86400. Deploy is on Thursday."),
                ("Model", "Then set SESSION_COOKIE_AGE = 86400 and add a test that reads the setting."),
                ("Person", "Write the test for me.")],
         facts=["cookie", "django", "86400", "thursday", "300"], gone_on=["def test", "import ", "assert", "class "]),
    dict(turns=[("Person", "We are choosing a laptop for my daughter Mira, who studies architecture. Budget 1500 dollars."),
                ("Model", "For architecture she needs a strong graphics card and 32 GB of memory. A 16 inch screen helps."),
                ("Person", "She wants it light, under 2 kg. We ruled out gaming laptops because of the noise."),
                ("Model", "Then look at thin workstations with an RTX 4060 class card, which stay under 2 kg."),
                ("Person", "List five models with prices.")],
         facts=["mira", "architecture", "1500", "2 kg", "gaming"], gone_on=["1.", "$1", "model 1", "here are five"]),
]

OLD = "Write an account of the conversation below for someone who must continue it. Keep: what the person wants, facts and numbers given, decisions made, what is still open. Short sentences. No filler. Do not answer the conversation."
AFTER = "Above is a conversation between a person and a model. Do not continue it and do not answer it. Write notes on it for someone who takes over: who the person is, what they want, every fact and number they gave, what was decided, and what is still open. Short lines."
FORM_ASK = "Above is a conversation between a person and a model. Do not continue it and do not answer it. Fill in the form about it."
FORM = {"type": "object", "additionalProperties": False, "required": ["person", "wants", "facts", "decided", "open"], "properties": {
    "person": {"type": "string", "maxLength": 120}, "wants": {"type": "string", "maxLength": 200},
    "facts": {"type": "array", "items": {"type": "string", "maxLength": 120}, "maxItems": 12},
    "decided": {"type": "array", "items": {"type": "string", "maxLength": 120}, "maxItems": 8},
    "open": {"type": "array", "items": {"type": "string", "maxLength": 120}, "maxItems": 6}}}
FORM_HINT = FORM_ASK + ' Reply with one JSON object: {"person": "who they are", "wants": "what they want overall", "facts": ["each fact or number they gave"], "decided": ["each decision"], "open": ["what is still to do"]}'

def transcript(turns): return "\n\n".join(f"{who}: {text}" for who, text in turns)

def body_for(way, turns):
    t = transcript(turns)
    if way == "as now: asked first": return {"messages": [{"role": "system", "content": OLD}, {"role": "user", "content": t}]}
    if way == "asked after, fenced": return {"messages": [{"role": "user", "content": f"<conversation>\n{t}\n</conversation>\n\n{AFTER}"}]}
    if way == "asked before and after": return {"messages": [{"role": "system", "content": "You write notes on conversations. You never continue or answer them."}, {"role": "user", "content": f"<conversation>\n{t}\n</conversation>\n\n{AFTER}"}]}
    if way == "a form, described": return {"messages": [{"role": "user", "content": f"<conversation>\n{t}\n</conversation>\n\n{FORM_HINT}"}]}
    if way == "a form, forced": return {"messages": [{"role": "user", "content": f"<conversation>\n{t}\n</conversation>\n\n{FORM_HINT}"}], "response_format": {"type": "json_schema", "schema": FORM}}
    raise ValueError(way)

def ask(path, body=None, timeout=180):
    req = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r: return r.status, json.loads(r.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e: return e.code, {"error": e.read().decode("utf-8", "replace")[:300]}

keyfile = os.path.join(WORK, "key.txt"); open(keyfile, "w").write(KEY + "\n"); os.chmod(keyfile, 0o600)
router = subprocess.Popen([os.path.join(RT, "llama-server"), "--models-dir", MODELS, "--host", "127.0.0.1", "--port", str(PORT), "--api-key-file", keyfile, "--no-models-autoload", "--no-webui",
                           "--models-max", "0", "--models-memory-margin", "900", "-c", "4096", "--log-file", os.path.join(WORK, "router.log")],
                          cwd=RT, env=dict(os.environ, LD_LIBRARY_PATH=LIBS + ":" + os.environ.get("LD_LIBRARY_PATH", "")), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
WAYS = ["as now: asked first", "asked after, fenced", "asked before and after", "a form, described", "a form, forced"]
try:
    for _ in range(200):
        try:
            if urllib.request.urlopen(BASE + "/health", timeout=1).status == 200: break
        except Exception: time.sleep(0.05)
    print(f"runtime: {note['name']} ({note['build']}); {len(TALKS)} conversations, {sum(len(t['facts']) for t in TALKS)} facts to keep; temperature 0.2; thinking off\n")
    shown = {}
    for model in MODEL_IDS:
        ask("/models/load", {"model": model})
        for _ in range(600):
            code, b = ask("/models")
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in b.get("data", [])): break
            time.sleep(0.2)
        print(f"## {model}")
        print(f"{'way of asking':26} {'facts kept':>11} {'went on instead':>16} {'tokens':>7} {'time':>7}")
        for way in WAYS:
            kept = gone = tokens = ms = 0
            for talk in TALKS:
                body = {"model": model, "temperature": 0.2, "seed": 7, "max_tokens": 400, "chat_template_kwargs": {"enable_thinking": False}, "reasoning_effort": "none", **body_for(way, talk["turns"])}
                t0 = time.time(); code, res = ask("/v1/chat/completions", body); ms += (time.time() - t0) * 1000
                text = res["choices"][0]["message"].get("content") or "" if code == 200 else str(res)
                tokens += res.get("usage", {}).get("completion_tokens", 0) if code == 200 else 0
                low = text.lower()
                kept += sum(1 for f in talk["facts"] if f in low)
                went = any(g in low for g in talk["gone_on"])
                gone += went
                shown.setdefault((model, way), []).append(text.strip().replace("\n", " ⏎ ")[:230])
            n = len(TALKS); total = sum(len(t["facts"]) for t in TALKS)
            print(f"{way:26} {kept:>6} of {total} {gone:>11} of {n} {tokens//n:>7} {ms/n:>5.0f}ms")
        ask("/models/unload", {"model": model}); time.sleep(0.5); print()
    if os.environ.get("TRIAL_SHOW"):
        for (model, way), texts in shown.items():
            print(f"--- {model[:14]} | {way}"); [print("   ", t) for t in texts[:1]]
finally:
    router.send_signal(signal.SIGTERM)
    try: router.wait(timeout=15)
    except subprocess.TimeoutExpired: router.kill()
