#!/usr/bin/env python3
"""Capture specimen.html with headless Chromium.

  tools/capture.py --sans inter,figtree --mono jetbrains-mono --views all,ui --scales 1,2 [--theme dark] [--sansvar ROND:100]

--sans / --mono accept "all" (every candidate of that kind in fonts.js).
Files land in out/shots/<view>/<sans>__<mono>__<theme>@<scale>x.png
The command line is the one fixed by the brief, plus a throw-away profile dir and a virtual-time budget so that
web fonts have loaded before the screenshot is taken.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.parse
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SIZES = {  # CSS px
    "all": (1440, 2600), "lockups": (1440, 860), "lockup-tile": (720, 336), "chat": (720, 548),
    "ui": (720, 368), "settings": (720, 400), "legibility": (720, 300), "mono": (720, 372), "display": (780, 200), "wordmark": (1440, 1200), "glyphs": (720, 372),
}


def fonts():
    src = open(os.path.join(ROOT, "fonts.js")).read()
    return json.loads(src[src.index("{"):src.rindex("}") + 1])


def shot(job):
    sans, mono, theme, view, scale, sansvar, force, tw, itv, extra = job
    w, h = SIZES[view]
    tag = (sansvar.replace(":", "") if sansvar else "") + ("tw" + tw.replace(",", "-") if tw else "")
    if extra:
        tag += "".join(k + v.replace(",", "-") for k, v in sorted(extra.items()) if k in ("lw", "wc"))
    mtag = "~mw" + extra["mw"].replace(",", "-") if extra and extra.get("mw") else ""
    name = f"{sans}{'~' + tag if tag else ''}__{mono}{mtag}__{theme}@{scale}x.png"
    out = os.path.join(ROOT, "out", "shots", view, name)
    if os.path.exists(out) and not force:
        return out
    os.makedirs(os.path.dirname(out), exist_ok=True)
    q = dict(sans=sans, mono=mono, theme=theme, view=view, shot=1)
    if sansvar:
        q["sansvar"] = sansvar
    if tw:
        q["tw"] = tw
    if itv:
        q["itv"] = itv
    q.update(extra or {})
    url = "file://" + urllib.parse.quote(os.path.join(ROOT, "specimen.html")) + "?" + urllib.parse.urlencode(q)
    # throw-away profile; CAPTURE_SCRATCH may point at a scratch folder (default: the system temp dir)
    prof = tempfile.mkdtemp(prefix="chr-", dir=os.environ.get("CAPTURE_SCRATCH"))
    env = {k: v for k, v in os.environ.items() if k != "TMPDIR"}  # Chromium's singleton socket needs a short temp path
    try:
        subprocess.run([
            "/usr/bin/chromium", "--headless=new", "--hide-scrollbars", f"--screenshot={os.path.relpath(out, ROOT)}",
            f"--window-size={w},{h}", f"--force-device-scale-factor={scale}",
            f"--user-data-dir={prof}", "--virtual-time-budget=5000", url,
        ], check=True, cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
    finally:
        shutil.rmtree(prof, ignore_errors=True)
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--sans", default="inter")
    ap.add_argument("--mono", default="jetbrains-mono")
    ap.add_argument("--views", default="all")
    ap.add_argument("--scales", default="1")
    ap.add_argument("--theme", default="dark")
    ap.add_argument("--sansvar", default="")
    ap.add_argument("--tw", default="", help="text weights regular,medium,semibold")
    ap.add_argument("--itv", default="", help="extra variation settings for sans italics, e.g. CRSV:1")
    ap.add_argument("--mw", default="", help="mono weights regular,medium")
    ap.add_argument("--lw", default="", help="lockup weights replacing 300,400,500,600,700")
    ap.add_argument("--ww", default="", help="wordmark study: weights to compare")
    ap.add_argument("--wc", default="", help="wordmark study: chosen weight")
    ap.add_argument("--ws", default="", help="wordmark study: weight of Studio")
    ap.add_argument("--wk", default="", help="wordmark study: symbol height as a multiple of cap height")
    ap.add_argument("--force", action="store_true")
    ap.add_argument("-j", type=int, default=6)
    a = ap.parse_args()
    F = fonts()
    sans = [k for k, v in F.items() if v["group"] != "mono"] if a.sans == "all" else a.sans.split(",")
    mono = [k for k, v in F.items() if v["group"] == "mono"] if a.mono == "all" else a.mono.split(",")
    extra = {k: getattr(a, k) for k in ("lw", "ww", "wc", "ws", "wk", "mw") if getattr(a, k)}
    jobs = [(s, m, t, v, int(sc), a.sansvar, a.force, a.tw, a.itv, extra)
            for s in sans for m in mono for t in a.theme.split(",") for v in a.views.split(",") for sc in a.scales.split(",")]
    with ThreadPoolExecutor(a.j) as ex:
        for o in ex.map(shot, jobs):
            pass
    print(f"{len(jobs)} shots")
