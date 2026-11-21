#!/usr/bin/env python3
"""Tries llama-server's router mode on the runtime Demido installed, and prints what it sees.

Demido leans on how the runtime loads, unloads and fits models (docs/DECISIONS.md K22). This script is how
that was checked (docs/research/12-running-models.md), and how to check it again when the runtime changes:

    python3 core/dev/router-trial.py

Needs: a runtime installed by `demido runtime install`, two GGUF models in Demido's models folder, and an
NVIDIA card (it reads memory with nvidia-smi). It downloads nothing. The runtime listens on 127.0.0.1 with
a key made for the run. Takes about a minute."""
import json, os, signal, subprocess, sys, threading, time, urllib.request, urllib.error, secrets, shutil

import glob, tempfile

HOME = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
MODELS = os.path.join(HOME, "models")
WORK = tempfile.mkdtemp(prefix="demido-router-trial-")
PORT = 7450
BASE = f"http://127.0.0.1:{PORT}"
KEY = secrets.token_hex(24)

# The newest runtime Demido installed that starts, and where its serving program is.
notes = []
for path in glob.glob(os.path.join(HOME, "runtimes/llama.cpp/*/demido-runtime.json")):
    note = json.load(open(path))
    if note.get("server") and note.get("version"): notes.append((note["installed_at"], os.path.dirname(path), note))
if not notes: sys.exit("No runtime that starts is installed. Run: demido runtime install")
_, root, note = max(notes, key=lambda n: n[0])
RT = os.path.dirname(os.path.join(root, note["server"]))
LIBS = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))

# Two models: the smallest (A) and the next (B).
files = sorted(glob.glob(os.path.join(MODELS, "*.gguf")), key=os.path.getsize)
if len(files) < 2: sys.exit(f"Two GGUF models are needed in {MODELS}; found {len(files)}.")
A, B = (os.path.basename(f)[:-5] for f in files[:2])
print(f"runtime: {note['name']} ({note['build']}); models: {A} ({os.path.getsize(files[0])>>20} MB), {B} ({os.path.getsize(files[1])>>20} MB)")

def say(title): print(f"\n=== {title}", flush=True)

def ask(method, path, body=None, key=True, timeout=120):
    req = urllib.request.Request(BASE + path, method=method, data=None if body is None else json.dumps(body).encode())
    if key: req.add_header("Authorization", f"Bearer {KEY}")
    if body is not None: req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            text = r.read().decode("utf-8", "replace")
            try: return r.status, json.loads(text)
            except Exception: return r.status, text[:300]
    except urllib.error.HTTPError as e:
        text = e.read().decode("utf-8", "replace")
        try: return e.code, json.loads(text)
        except Exception: return e.code, text[:300]
    except Exception as e:
        return 0, f"{type(e).__name__}: {e}"

def vram():
    out = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"], capture_output=True, text=True).stdout.strip()
    return int(out.splitlines()[0]) if out else -1

def children(pid):
    out = subprocess.run(["pgrep", "-P", str(pid), "-a"], capture_output=True, text=True).stdout.strip().splitlines()
    return [(int(l.split()[0]), " ".join(l.split()[1:])[:160]) for l in out if l]

def status_of(model):
    code, body = ask("GET", "/models")
    if code != 200: return f"({code})"
    for m in body.get("data", []):
        if m.get("id") == model: return (m.get("status") or {}).get("value")
    return "(not listed)"

def wait_status(model, wanted, seconds=180):
    t0 = time.time()
    while time.time() - t0 < seconds:
        s = status_of(model)
        if s in wanted: return s, time.time() - t0
        time.sleep(0.25)
    return status_of(model), time.time() - t0

events = []
def listen():
    req = urllib.request.Request(BASE + "/models/sse"); req.add_header("Authorization", f"Bearer {KEY}")
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            for raw in r:
                line = raw.decode("utf-8", "replace").strip()
                if line.startswith("data:"):
                    events.append((time.time(), line[5:].strip()[:400]))
    except Exception as e:
        events.append((time.time(), f"(stream ended: {type(e).__name__})"))

def start(margin, extra=()):
    keyfile = os.path.join(WORK, "key.txt"); open(keyfile, "w").write(KEY + "\n"); os.chmod(keyfile, 0o600)
    args = [os.path.join(RT, "llama-server"), "--models-dir", MODELS, "--host", "127.0.0.1", "--port", str(PORT), "--api-key-file", keyfile,
            "--no-models-autoload", "--no-webui", "--models-max", "0", "--models-memory-margin", str(margin), "--log-file", os.path.join(WORK, f"router-{margin}.log"), *extra]
    env = dict(os.environ, LD_LIBRARY_PATH=LIBS + ":" + os.environ.get("LD_LIBRARY_PATH", ""))
    t0 = time.time()
    p = subprocess.Popen(args, cwd=RT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while time.time() - t0 < 60:
        code, _ = ask("GET", "/health", key=False, timeout=2)
        if code == 200: break
        if p.poll() is not None: print("router exited at once, code", p.returncode); sys.exit(1)
        time.sleep(0.05)
    return p, time.time() - t0

def stop(p):
    t0 = time.time(); p.send_signal(signal.SIGTERM)
    try: p.wait(timeout=20)
    except subprocess.TimeoutExpired: p.kill(); p.wait(); return "had to be killed", time.time() - t0
    return f"exit {p.returncode}", time.time() - t0

base_vram = vram()
say("1. Starting the router (no margin)")
p, took = start(0)
print(f"ready in {took:.2f} s; pid {p.pid}; children now: {children(p.pid)}; VRAM in use before anything: {base_vram} MiB")
threading.Thread(target=listen, daemon=True).start()

say("2. What it answers without the key, and whether the web UI is off")
for path in ["/health", "/models", "/", "/props", "/v1/models"]:
    code, body = ask("GET", path, key=False, timeout=5)
    print(f"  no key  GET {path:12} -> {code}  {json.dumps(body)[:110] if not isinstance(body, str) else body[:110]!r}")
code, body = ask("GET", "/", timeout=5); print(f"  with key GET /            -> {code}  {str(body)[:80]!r}")

say("3. The list of models, as it really comes")
time.sleep(1.0)
code, body = ask("GET", "/models")
print("status", code, "; ids:", [m.get("id") for m in body.get("data", [])])
if body.get("data"):
    m = dict(body["data"][0]); print(json.dumps(m, indent=1)[:1500])

say("4. The memory measurements, without a margin")
time.sleep(3)
code, body = ask("GET", "/models")
print("after 3 s:", {m["id"]: (m.get("memory") or {}).get("status") for m in body.get("data", [])}, "(nothing is measured unless a margin is set; see part 10)")

say("5. Loading the small model, asking it something, unloading it")
before = vram(); t0 = time.time()
print("load ->", ask("POST", "/models/load", {"model": A}))
s, took = wait_status(A, ("loaded",)); kids = children(p.pid)
print(f"status {s} after {took:.1f} s; children: {[(k[0], k[1][:90]) for k in kids]}; VRAM {before} -> {vram()} MiB")
t0 = time.time()
code, body = ask("POST", "/v1/chat/completions", {"model": A, "messages": [{"role": "user", "content": "Reply with exactly: ready. /no_think"}], "max_tokens": 24, "temperature": 0})
if code == 200:
    msg = body["choices"][0]["message"]; tm = body.get("timings", {})
    print(f"answer in {time.time()-t0:.2f} s: {msg.get('content')!r}; {tm.get('predicted_per_second', 0):.0f} tokens a second; prompt {tm.get('prompt_n')} tokens")
else: print("chat failed:", code, str(body)[:300])
code, body = ask("GET", "/models")
for m in body.get("data", []):
    if m["id"] == A:
        eff = (m.get("memory") or {}).get("effective")
        print("  effective:", None if not eff else (eff.get("n_ctx"), [(d["name"], round((d.get("model",0)+d.get("context",0)+d.get("compute",0))/1048576)) for d in eff.get("devices", [])]))
kid_pids = [k[0] for k in kids]
print("unload ->", ask("POST", "/models/unload", {"model": A}))
s, took = wait_status(A, ("unloaded",), 30)
time.sleep(0.5)
alive = [pid for pid in kid_pids if os.path.exists(f"/proc/{pid}")]
print(f"status {s} after {took:.2f} s; child processes still alive: {alive}; VRAM now {vram()} MiB (was {before} before loading)")

say("6. A request for a model that is not loaded, with autoload off")
code, body = ask("POST", "/v1/chat/completions", {"model": A, "messages": [{"role": "user", "content": "hi"}], "max_tokens": 4})
print(code, json.dumps(body)[:260])

say("7. A new file in the folder: is it seen without a restart?")
link = os.path.join(MODELS, "Trial-Copy-Q8_0.gguf")
if os.path.exists(link): os.remove(link)
os.link(os.path.join(MODELS, A + ".gguf"), link)
code, body = ask("GET", "/models"); print("  plain list:      ", [m["id"] for m in body.get("data", [])])
code, body = ask("GET", "/models?reload=1"); print("  with ?reload=1:  ", [m["id"] for m in body.get("data", [])])
os.remove(link)
code, body = ask("GET", "/models?reload=1"); print("  after removing it:", [m["id"] for m in body.get("data", [])])

say("8. A child that crashes")
ask("POST", "/models/load", {"model": A}); s, took = wait_status(A, ("loaded",)); kids = children(p.pid)
print(f"loaded ({s}) in {took:.1f} s; killing child {kids[0][0] if kids else None}")
if kids: os.kill(kids[0][0], signal.SIGKILL)
for i in range(8):
    time.sleep(0.5); print(f"  {0.5*(i+1):.1f} s after: status {status_of(A)!r}", end="; ")
print()
code, body = ask("POST", "/v1/chat/completions", {"model": A, "messages": [{"role": "user", "content": "hi"}], "max_tokens": 4}); print("  a request afterwards:", code, json.dumps(body)[:200])
print("  router still alive:", p.poll() is None, "; VRAM", vram(), "MiB")
ask("POST", "/models/unload", {"model": A}); time.sleep(1)

say("9. Stopping the router with a model loaded")
ask("POST", "/models/load", {"model": A}); wait_status(A, ("loaded",)); kids = [k[0] for k in children(p.pid)]
how, took = stop(p); time.sleep(0.7)
print(f"{how} in {took:.2f} s; children left alive: {[k for k in kids if os.path.exists(f'/proc/{k}')]}; VRAM {vram()} MiB (start: {base_vram})")

say("10. With a margin that leaves room for one model only")
total = int(subprocess.run(["nvidia-smi", "--query-gpu=memory.total", "--format=csv,noheader,nounits"], capture_output=True, text=True).stdout.split()[0])
free_now = total - vram()
margin = max(0, free_now - 2600)   # leave about 2.6 GB usable: either model fits alone, both do not
print(f"card total {total} MiB, free now {free_now} MiB, margin {margin} MiB (about 2600 MiB usable)")
events.clear()
p, took = start(margin, extra=("-c", "4096"))
threading.Thread(target=listen, daemon=True).start()
t0 = time.time()
while time.time() - t0 < 120:
    code, body = ask("GET", "/models")
    st = [(m.get("memory") or {}).get("status") for m in body.get("data", [])]
    if st and all(s in ("measured", "failed") for s in st): break
    time.sleep(0.5)
print("load small ->", ask("POST", "/models/load", {"model": A})[0], wait_status(A, ("loaded",))[0], "VRAM", vram())
t0 = time.time(); print("load larger ->", ask("POST", "/models/load", {"model": B})[0]); s, took = wait_status(B, ("loaded",), 120)
print(f"larger: {s} after {took:.1f} s; small is now: {status_of(A)!r}; VRAM {vram()} MiB; children: {len(children(p.pid))}")
code, body = ask("POST", "/v1/chat/completions", {"model": B, "messages": [{"role": "user", "content": "Reply with exactly: ready. /no_think"}], "max_tokens": 24, "temperature": 0})
print("  the larger one answers:", code, (body["choices"][0]["message"].get("content") if code == 200 else str(body)[:200]))
print("load small again ->", ask("POST", "/models/load", {"model": A})[0], wait_status(A, ("loaded",), 60)[0], "; larger is now:", repr(status_of(B)), "VRAM", vram())
print("events seen on /models/sse in this part:")
for t, e in events[:14]: print("   ", e[:230])
how, took = stop(p); time.sleep(0.7)
print(f"stopped: {how} in {took:.2f} s; VRAM {vram()} MiB")
