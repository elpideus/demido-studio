#!/usr/bin/env python3
"""Does the model writing slow Demido's voice? (DECISIONS V8: the first sound of an answer came after 3.3 s in one
try, against 0.7 s alone.) The voice engine says five sentences, three times over: with the model idle, while
Qwen3 4B writes a long answer on the card, and while it writes with the runtime started with extra arguments
(TRIAL_LLAMA_ARGS, default "--poll 0": llama.cpp's threads then wait for work without spinning). Prints the time
to each sentence's first sound and to its end.

    python3 core/dev/voice-contention-trial.py      (needs ~/.cache/demido-voice-files and the installed runtime)

Uses the card and the processor for about two minutes. Downloads nothing."""
import glob, json, os, secrets, signal, struct, subprocess, sys, tempfile, threading, time, urllib.request

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
DEMIDO = os.path.join(REPO, "core", "target", "debug", "demido")
VOICE_DIR = os.path.expanduser(os.environ.get("TRIAL_VOICE_DIR", "~/.cache/demido-voice-files"))
HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
EXTRA = os.environ.get("TRIAL_LLAMA_ARGS", "--poll 0").split()
MODEL = "Qwen3-4B-Q4_K_M"
SENTENCES = ["It will be sunny in Rome tomorrow, with a high of nineteen degrees.", "In the evening a light wind comes from the sea.",
             "The day after, clouds return in the afternoon.", "Rain is likely on Thursday, so take an umbrella.", "By the weekend it is warm again."]

notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"):
        notes.append((note["installed_at"], os.path.dirname(path), note))
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"]))
LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))


def runtime(extra):
    work, port, key = tempfile.mkdtemp(prefix="demido-contention-"), 7468, secrets.token_hex(24)
    keyfile = os.path.join(work, "key.txt")
    open(keyfile, "w").write(key + "\n")
    os.chmod(keyfile, 0o600)
    p = subprocess.Popen([os.path.join(RT, "llama-server"), "--models-dir", os.path.join(HOME, "models"), "--host", "127.0.0.1", "--port", str(port), "--api-key-file", keyfile,
                          "--no-models-autoload", "--no-webui", "--models-max", "0", "--models-memory-margin", "900", "-c", "4096", *extra],
                         cwd=RT, env=dict(os.environ, LD_LIBRARY_PATH=LIBS), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{port}"
    call = lambda path, body=None, timeout=300: urllib.request.urlopen(urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"}), timeout=timeout)
    for _ in range(200):
        try:
            if urllib.request.urlopen(base + "/health", timeout=1).status == 200:
                break
        except Exception:
            time.sleep(0.05)
    call("/models/load", {"model": MODEL})
    for _ in range(600):
        if any(m["id"] == MODEL and m["status"]["value"] == "loaded" for m in json.load(call("/models"))["data"]):
            break
        time.sleep(0.2)
    return p, call


def keep_writing(call, until):
    while not until.is_set():
        body = {"model": MODEL, "stream": True, "max_tokens": 700, "chat_template_kwargs": {"enable_thinking": False},
                "messages": [{"role": "user", "content": "Write a long, detailed story about a heron who travels across Europe."}]}
        with call("/v1/chat/completions", body) as r:
            for _ in r:
                if until.is_set():
                    break


class Engine:
    def __init__(self):
        self.p = subprocess.Popen([DEMIDO, "voice-engine", "--dir", VOICE_DIR, "--voice", os.path.join(VOICE_DIR, "voices", "2181.wav")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.events, self.cv = [], threading.Condition()
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        while True:
            head = self.p.stdout.read(5)
            if len(head) < 5:
                return
            n = struct.unpack("<I", head[1:])[0]
            payload = self.p.stdout.read(n)
            if head[0] == ord("e"):
                with self.cv:
                    self.events.append(json.loads(payload))
                    self.cv.notify_all()

    def send(self, kind, payload=b""):
        self.p.stdin.write(bytes([kind]) + struct.pack("<I", len(payload)) + payload)
        self.p.stdin.flush()

    def say(self, n, text):
        start = time.perf_counter()
        self.send(ord("s"), struct.pack("<I", n) + text.encode())
        with self.cv:
            self.cv.wait_for(lambda: any(e.get("event") == "said" and e.get("sentence") == n for e in self.events), timeout=60)
        first = next((e.get("first_ms") for e in self.events if e.get("event") == "speaking" and e.get("sentence") == n), None)
        return first, (time.perf_counter() - start) * 1000

    def close(self):
        self.send(ord("q"))
        self.p.wait(30)


def run(label, call=None):
    engine = Engine()
    engine.say(0, "Hello.")  # the voice is read once, first
    until = threading.Event()
    writer = None
    if call:
        writer = threading.Thread(target=keep_writing, args=(call, until), daemon=True)
        writer.start()
        time.sleep(2)
    times = [engine.say(i + 1, s) for i, s in enumerate(SENTENCES)]
    until.set()
    engine.close()
    firsts = [f for f, _ in times if f is not None]
    print(f"{label:44} first sound: median {sorted(firsts)[len(firsts) // 2]:5.0f} ms, slowest {max(firsts):5.0f} ms;  a whole sentence median {sorted(w for _, w in times)[len(times) // 2]:5.0f} ms", flush=True)


print(f"llama.cpp {note['build']}, {MODEL}; voice engine {DEMIDO}\n")
p, call = runtime([])
try:
    run("model idle")
    run("while the model writes (as Demido starts it)", call)
finally:
    p.send_signal(signal.SIGTERM)
    p.wait(15)
p, call = runtime(EXTRA)
try:
    run(f"while it writes, started with {' '.join(EXTRA)}", call)
finally:
    p.send_signal(signal.SIGTERM)
    p.wait(15)
