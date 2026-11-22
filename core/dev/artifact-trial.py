#!/usr/bin/env python3
"""Do small models give a file to keep (an artifact) when they should, and only then? (research note 23)

Drives a real Demido server, so the tools are listed and asked about exactly as in a session: a scratch data
folder that shares the real runtime and models (as core/dev/real-home.sh does), on port 7399. Nothing here
touches the sessions or settings of the server a person is using, and nothing is deleted from the models
folder.

Each request is asked in a new session, with thinking off and reading from the internet off. Requests that
should give a file (a page, a letter, a script, a checklist), requests that should not (a fact, a sum, a
greeting), one where the person asks for a file in the session's folder (that is write_file's work), and a
change to a page given before (a new version of the same file). The requests that should not give a file,
and the folder one, are asked with artifacts on and off: what the new tool takes from the other choices.

    python3 core/dev/artifact-trial.py [model ...]

Small numbers: direction, not a benchmark. Takes a few minutes and uses the graphics card."""
import json, os, secrets, subprocess, sys, tempfile, time, urllib.error, urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = os.path.join(REPO, "core/target/debug/demido")
REAL = os.environ.get("DEMIDO_REAL_HOME") or os.path.expanduser("~/.local/share/demido")
BASE = "http://127.0.0.1:7399/api/v1"
MODELS = sys.argv[1:] or ["Qwen3-0.6B-Q8_0", "Qwen3-1.7B-Q8_0", "granite-3.3-2b-instruct-Q4_K_M", "Qwen3-4B-Q4_K_M"]

GIVE = [
    ("page", "Write a one-page HTML site for my bakery, Sol, with our hours: 7 to 13, Monday to Saturday."),
    ("letter", "Write a cover letter of about 250 words for a junior Rust developer job at a small studio."),
    ("script", "Write a Python script that renames every .txt file in a folder to .md."),
    ("checklist", "Make me a packing list for a week of hiking in the Alps, as a Markdown checklist."),
]
KEEP = [
    ("capital", "What is the capital of Portugal?"),
    ("mutex", "Explain in two sentences what a mutex is."),
    ("sum", "What is 17 times 23?"),
    ("hello", "Say hello in French."),
]
FOLDER = ("folder", "Add a file called notes.md to the folder with three ideas for a blog post.")
CHANGE = "Change the hours to 8 to 14."


def ask(method, path, body=None, key=None):
    req = urllib.request.Request(BASE + path, method=method, data=None if body is None else json.dumps(body).encode(), headers={"authorization": f"Bearer {key}", "content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=600) as res:
            return json.loads(res.read() or b"null")
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{method} {path}: {e.code} {e.read()[:300]!r}")


def turn(key, session, text):
    """Says `text`, waits for the answer, and gives it back."""
    ask("POST", f"/sessions/{session}/messages", {"content": text}, key)
    for _ in range(1200):
        detail = ask("GET", f"/sessions/{session}", key=key)
        if not detail.get("answering"):
            return detail["messages"][-1]
        time.sleep(0.5)
    raise RuntimeError("no answer in ten minutes")


def one(key, model, text, folder=None, then=None):
    session = ask("POST", "/sessions", {"model": model, "title": text[:40]}, key)["id"]
    if folder:
        ask("PATCH", f"/sessions/{session}", {"folder": folder}, key)
    answers = [turn(key, session, text)]
    if then:
        answers.append(turn(key, session, then))
    return session, answers


def described(answer):
    steps = answer.get("steps") or []
    gave = [s for s in steps if s["tool"] == "give_file"]
    return {
        "tools": [s["tool"] + ("" if s["state"] == "done" else f"({s['state']})") for s in steps],
        "gave": [(s.get("artifact") or {}).get("name") for s in gave if s["state"] == "done"],
        "version": [(s.get("artifact") or {}).get("n") for s in gave if s["state"] == "done"],
        "said": len(answer.get("content") or ""),
        "refused": [(s["result"][:120], json.dumps(s.get("args"))[:240]) for s in steps if s["state"] == "refused"],
        "state": answer.get("state"),
    }


def main():
    if not os.access(D, os.X_OK):
        sys.exit("Build first: cargo build --manifest-path core/Cargo.toml")
    scratch = tempfile.mkdtemp(prefix="demido-artifact-trial-")
    os.symlink(os.path.join(REAL, "runtimes"), os.path.join(scratch, "runtimes"))
    os.symlink(os.path.join(REAL, "models"), os.path.join(scratch, "models"))
    env = dict(os.environ, DEMIDO_HOME=scratch, DEMIDO_VAULT="file")
    server = subprocess.Popen([D, "serve", "--listen", "127.0.0.1:7399"], env=env, stdout=open(os.path.join(scratch, "server.log"), "w"), stderr=subprocess.STDOUT)
    rows = []
    try:
        for _ in range(50):
            if os.path.exists(os.path.join(scratch, "key")):
                break
            time.sleep(0.2)
        time.sleep(1)
        key = open(os.path.join(scratch, "key")).read().strip()
        ask("PUT", "/settings/web.use", {"value": False}, key)
        for model in MODELS:
            work = tempfile.mkdtemp(prefix="demido-artifact-folder-", dir=scratch)
            # TRIAL_ONLY=give: only the requests that should give a file, with artifacts on.
            for on in ((True,) if os.environ.get("TRIAL_ONLY") == "give" else (True, False)):
                # Always, to see what a model does with the tool in a session with a folder too.
                ask("PUT", "/settings/tools.artifacts", {"value": "always" if on else "never"}, key)
                cases = ([(n, t, None, None) for n, t in GIVE] + [("change", GIVE[0][1], None, CHANGE)] if on else []) + ([] if os.environ.get("TRIAL_ONLY") == "give" else [(n, t, None, None) for n, t in KEEP] + [(FOLDER[0], FOLDER[1], work, None)])
                for name, text, folder, then in cases:
                    started = time.time()
                    try:
                        _, answers = one(key, model, text, folder, then)
                        got = [described(a) for a in answers]
                    except RuntimeError as e:
                        got = [{"tools": [], "gave": [], "version": [], "said": 0, "state": f"error: {e}"}]
                    row = {"model": model, "artifacts": on, "case": name, "turns": got, "seconds": round(time.time() - started, 1)}
                    rows.append(row)
                    print(json.dumps(row), flush=True)
    finally:
        server.terminate()
        server.wait(timeout=30)
        out = os.path.join(REPO, "core/dev/artifact-trial.jsonl") if os.environ.get("TRIAL_KEEP") else None
        if out:
            open(out, "w").write("".join(json.dumps(r) + "\n" for r in rows))
        subprocess.run(["rm", "-rf", scratch])


if __name__ == "__main__":
    main()
