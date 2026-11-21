#!/usr/bin/env python3
"""Build contact sheets from out/shots with ImageMagick montage (one label per tile).

  tools/sheets.py <out-name> --view ui --fonts inter,figtree,... [--kind sans|mono] [--fixed jetbrains-mono]
                  [--scale 1] [--cols 3] [--theme dark] [--zoom 100]

--fonts accepts group names: neutral, round, extra-sans, mono, or "all-sans".
The label under each tile is the family name plus the facts that matter for that sheet.
Sheets are kept under 2400 px wide; tiles are never resampled unless --zoom is given
(--zoom 200 uses nearest-neighbour, to inspect 1x pixel rendering).
"""
import argparse
import json
import os
import re
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FACTS = json.load(open(os.path.join(ROOT, "out", "facts.json")))
LABEL_FONT = "DejaVu-Sans"  # label only; not a candidate


def expand(spec):
    out = []
    for tok in spec.split(","):
        base = tok.split("~")[0]
        if tok == "neutral":
            out += [k for k, v in FACTS.items() if v["group"] == "neutral" and not v["extra"]]
        elif tok == "round":
            out += [k for k, v in FACTS.items() if v["group"] == "round" and not v["extra"]]
        elif tok == "extra-sans":
            out += [k for k, v in FACTS.items() if v["group"] != "mono" and v["extra"]]
        elif tok == "all-sans":
            out += [k for k, v in FACTS.items() if v["group"] != "mono"]
        elif tok == "mono":
            out += [k for k, v in FACTS.items() if v["group"] == "mono"]
        elif base in FACTS:
            out.append(tok)
        else:
            raise SystemExit(f"unknown font {tok}")
    return out


def pretty(var):
    """Turn a capture tag such as tw250-350-450lw200-270-350-450-550 or ROND100 into words."""
    out = []
    m = re.search(r"tw(\d+)-(\d+)-(\d+)", var)
    if m:
        out.append("text %s/%s/%s" % m.groups())
    m = re.search(r"mw(\d+)-(\d+)", var)
    if m:
        out.append("weights %s/%s" % m.groups())
    m = re.search(r"lw(\d+)-(\d+)-(\d+)-(\d+)-(\d+)", var)
    if m:
        out.append("lockups %s/%s/%s" % m.groups()[2:])
    m = re.search(r"(ROND|SHRP|GRAD)(\d+)", var)
    if m:
        out.append("%s %s" % m.groups())
    return ", ".join(out) or var


def label(fid):
    base, _, var = fid.partition("~")
    f = FACTS[base]
    cov = "+".join(n for n, k in (("Cyr", "cyrillic"), ("Grk", "greek")) if f["coverage"][k] >= 0.99) or "no Cyrillic, no Greek"
    it = {"italic file": "italics", "italic files (static)": "italics", "slnt axis (oblique)": "oblique (slnt axis)",
          "slnt + CRSV axes (designed italic)": "italic (slnt+CRSV axes)", "none": "no italics"}[f["italic_kind"]]
    extra = " (added)" if f["extra"] else ""
    var = f"   [{pretty(var)}]" if var else ""
    # two lines, so that long labels never run into the neighbouring tile
    return f"{f['family']}{extra}{var}\n{cov} · {it} · {'variable' if f['variable'] else 'static only'}"


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--view", required=True)
    ap.add_argument("--fonts", required=True)
    ap.add_argument("--kind", default="sans")
    ap.add_argument("--fixed", default="jetbrains-mono")
    ap.add_argument("--scale", default="1")
    ap.add_argument("--cols", type=int, default=3)
    ap.add_argument("--theme", default="dark")
    ap.add_argument("--zoom", type=int, default=100)
    ap.add_argument("--crop", default="")
    a = ap.parse_args()
    cmd = ["magick", "montage", "-background", "#000000", "-fill", "#d8d8d4", "-font", LABEL_FONT,
           "-pointsize", str(15 * int(a.scale) if a.zoom == 100 else 17)]
    for fid in expand(a.fonts):
        pair = f"{fid}__{a.fixed}" if a.kind == "sans" else f"{a.fixed}__{fid}"
        src = os.path.join(ROOT, "out", "shots", a.view, f"{pair}__{a.theme}@{a.scale}x.png")
        if not os.path.exists(src):
            raise SystemExit(f"missing {src}")
        cmd += ["-label", label(fid), src]
    pre = []
    if a.crop:
        pre += ["-crop", a.crop, "+repage"]
    if a.zoom != 100:
        pre += ["-filter", "point", "-resize", f"{a.zoom}%"]
    cmd += pre + ["-tile", f"{a.cols}x", "-geometry", "+6+6", os.path.join(ROOT, "out", a.name)]
    subprocess.run(cmd, check=True)
    r = subprocess.run(["magick", "identify", "-format", "%wx%h", os.path.join(ROOT, "out", a.name)], capture_output=True, text=True)
    print(a.name, r.stdout)
