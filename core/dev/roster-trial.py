#!/usr/bin/env python3
"""Can a small model choose which model should do a piece of work? (docs/DECISIONS.md O8, plan item 7.)

Before agents may pick models for themselves (O2, O3), Demido has to know whether small models choose well
among models at all. Each model on this computer plays the one that hands out work. It is shown a short list
of models (the roster) and one piece of work, and is asked which model should do it, its reply held to the
list's names (O3). Eighteen pieces of work: six mechanical (the fastest should do them), six that need care
(the most careful), six that need eyes (the one that reads images). The roster is shown four ways:

  labels          a plain label for each, worked out from the facts (O2): the fastest, the most careful, reads images
  facts           only the numbers: size, speed, what it takes in. The model has to work out the labels
  labels, moved   the labels, in another order: does the order decide?
  labels, or me   the labels, and "yourself" as a fourth choice: what does it keep for itself?
  roles           the choices named by what each is for (fastest, most-careful, reads-images), not by the
                  model; and the same moved. Demido would turn the role into the model

The roster's models are named as models would be; the one that reads images is not on this computer (it
does not have to be: nothing is sent to it, only its name is chosen). Temperature 0, thinking off, as
Demido's own quiet questions are asked. Small numbers: direction, not a benchmark.

    python3 core/dev/roster-trial.py          TRIAL_MODELS=4 for all four; TRIAL_SHOW_WRONG=1 prints the wrong picks

Needs a runtime installed by Demido and GGUF models in its models folder. Downloads nothing."""
import glob, json, os, secrets, signal, subprocess, sys, tempfile, time, urllib.request, urllib.error

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models"); WORK = tempfile.mkdtemp(prefix="demido-roster-trial-")
PORT, KEY = 7461, secrets.token_hex(24); BASE = f"http://127.0.0.1:{PORT}"
notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"])); LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
MODEL_IDS = [os.path.basename(f)[:-5] for f in files][:int(os.environ.get("TRIAL_MODELS", "4"))]

FAST, CAREFUL, EYES, ME = "Qwen3-0.6B", "Qwen3-4B", "Gemma-4-E4B", "yourself"
LABELS = {FAST: "the fastest. For short, mechanical work: copying, sorting, reformatting, pulling things out of a text.",
          CAREFUL: "the most careful, and slow. For work that needs thinking: reasoning, judging, writing that matters.",
          EYES: "reads images: photos, screenshots, scans. The only one that can see a picture."}
ROLE = {FAST: "fastest", CAREFUL: "most-careful", EYES: "reads-images"}
FACTS = {FAST: "0.6 billion parameters. Writes about 300 words a second. Takes text.",
         CAREFUL: "4 billion parameters. Writes about 60 words a second. Takes text.",
         EYES: "4 billion parameters. Writes about 70 words a second. Takes text and images."}
ROSTERS = {
    "labels": [(m, LABELS[m]) for m in (FAST, CAREFUL, EYES)],
    "facts": [(m, FACTS[m]) for m in (FAST, CAREFUL, EYES)],
    "labels, moved": [(m, LABELS[m]) for m in (EYES, CAREFUL, FAST)],
    "labels, or me": [(m, LABELS[m]) for m in (FAST, CAREFUL, EYES)] + [(ME, "you, the model handing out the work. You see the whole conversation.")],
    # The choice named by what each is for, not by the model: Demido would turn the role into the model.
    "roles": [(ROLE[m], LABELS[m]) for m in (FAST, CAREFUL, EYES)],
    "roles, moved": [(ROLE[m], LABELS[m]) for m in (EYES, CAREFUL, FAST)],
}
WORK_ITEMS = [
    (FAST, "Turn this list of names into one line, separated by commas: Ana, Bo, Cleo, Dev, Eli, Fay, Gus, Hal."),
    (FAST, "Pull every e-mail address out of the text below and list them, one a line. [3 pages of meeting notes follow]"),
    (FAST, "Rewrite every date in this table from 21/11/2026 to 2026-11-21. [a table of 200 rows follows]"),
    (FAST, "Sort these 40 file names alphabetically. [40 file names follow]"),
    (FAST, "Make the first letter of each line upper case. [a poem of 30 lines follows]"),
    (FAST, "Count how many lines of this log say ERROR. [a log of 500 lines follows]"),
    (CAREFUL, "Find the bug in this function, explain why it happens, and fix it. [a 60-line Rust function follows]"),
    (CAREFUL, "Write a careful reply to a customer who is angry that their refund is three weeks late."),
    (CAREFUL, "Which of these two contracts costs less over five years, counting the fees? Give your reasons. [two contracts follow]"),
    (CAREFUL, "Read this report and say where its argument is weakest. [a 12-page report follows]"),
    (CAREFUL, "Plan a three-week study schedule for an exam in statistics, for someone who works full time."),
    (CAREFUL, "Is this proof correct? If not, which step is wrong? [a page of algebra follows]"),
    (EYES, "Say what is in this photo. [a photo is attached]"),
    (EYES, "Read the total off this receipt. [a picture of a receipt is attached]"),
    (EYES, "Does this screenshot show an error message? What does it say? [a screenshot is attached]"),
    (EYES, "Copy the text from this scanned letter. [a scan is attached]"),
    (EYES, "Which of the two charts in this picture shows the higher sales in March? [a picture is attached]"),
    (EYES, "Describe this diagram of a network so that someone who cannot see it understands it. [a diagram is attached]"),
]
SYSTEM = "You hand out work to other models. Answer only with the JSON asked for."

def ask(path, body=None, timeout=180):
    req = urllib.request.Request(BASE + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r: return r.status, json.loads(r.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e: return e.code, {"error": e.read().decode("utf-8", "replace")[:300]}

def choose(model, roster, work):
    names = [n for n, _ in roster]
    listed = "\n".join(f"- {n}: {d}" for n, d in roster)
    content = f"Models you can give work to:\n{listed}\n\n<work>\n{work}\n</work>\n\nWhich model should do this work? Reply with one JSON object: {{\"model\": \"name\"}}"
    body = {"model": model, "temperature": 0, "seed": 7, "max_tokens": 40, "chat_template_kwargs": {"enable_thinking": False}, "reasoning_effort": "none",
            "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": content}],
            "response_format": {"type": "json_schema", "schema": {"type": "object", "properties": {"model": {"enum": names}}, "required": ["model"]}}}
    code, res = ask("/v1/chat/completions", body)
    if code != 200: return None
    try: picked = json.loads(res["choices"][0]["message"]["content"])["model"]
    except Exception: return None
    # A role stands for its model.
    return next((m for m, r in ROLE.items() if r == picked), picked)

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
    print(f"runtime: {note['name']} ({note['build']}); {len(WORK_ITEMS)} pieces of work, 6 for each model on the roster; temperature 0; thinking off\n")
    print(f"{'hands out':34} {'roster':15} {'right':>9} {'mechanical':>11} {'careful':>8} {'eyes':>5}   picks (fast/careful/eyes/self)")
    for model in MODEL_IDS:
        ask("/models/load", {"model": model})
        for _ in range(600):
            code, b = ask("/models")
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in b.get("data", [])): break
            time.sleep(0.2)
        for label, roster in ROSTERS.items():
            got = [(want, choose(model, roster, work), work) for want, work in WORK_ITEMS]
            right = lambda kind: sum(1 for want, pick, _ in got if want == kind and pick == want)
            total = sum(1 for want, pick, _ in got if pick == want)
            picks = "/".join(str(sum(1 for _, p, _ in got if p == n)) for n in (FAST, CAREFUL, EYES, ME))
            print(f"{model:34} {label:15} {total:>3} of {len(got)} {right(FAST):>8}/6 {right(CAREFUL):>5}/6 {right(EYES):>2}/6   {picks}", flush=True)
            wrongs += [(model[:12], label, want, pick, work[:50]) for want, pick, work in got if pick != want]
        ask("/models/unload", {"model": model}); time.sleep(0.5)
    if os.environ.get("TRIAL_SHOW_WRONG"):
        for w in wrongs: print("  " + " | ".join(str(x) for x in w))
finally:
    router.send_signal(signal.SIGTERM)
    try: router.wait(timeout=15)
    except subprocess.TimeoutExpired: router.kill()
