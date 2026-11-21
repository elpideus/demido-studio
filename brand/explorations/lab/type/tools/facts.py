#!/usr/bin/env python3
"""Collect hard facts about every installed Fontsource package and emit the specimen registry.

Reads only local files under node_modules.
Writes out/facts.json (everything measured) and fonts.js (what specimen.html needs).

Measured per family
  - license (metadata + LICENSE file), Reserved Font Name declarations
  - axes, weights, italic kind, subsets
  - vertical metrics, average advance (width economy), tabular digits by default
  - OpenType features present (tnum / zero / case / ss / cv)
  - coverage of Latin Extended, Cyrillic, Greek, Vietnamese and a few UI symbols (union of all subsets)
  - woff2 sizes
  - stem thickness of "l" at 400/500/600/700 as a share of cap height (to match the logo stroke)
  - roundness of the "o" bowl (aspect and squareness of the outer contour)
"""
import glob
import json
import os
import re

from fontTools.pens.areaPen import AreaPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.pointInsidePen import PointInsidePen
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.ttLib import TTFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NM = os.path.join(ROOT, "node_modules")

# ---------------------------------------------------------------- candidates
# group: neutral | round | mono ; extra = added by the type lead (max three in the final report)
CANDIDATES = [
    ("inter", "neutral", False), ("ibm-plex-sans", "neutral", False), ("mona-sans", "neutral", False),
    ("instrument-sans", "neutral", False), ("hanken-grotesk", "neutral", False),
    ("atkinson-hyperlegible-next", "neutral", False), ("geist", "neutral", False),
    ("wix-madefor-text", "neutral", False), ("public-sans", "neutral", False), ("reddit-sans", "neutral", False),
    ("figtree", "round", False), ("onest", "round", False), ("dm-sans", "round", False),
    ("manrope", "round", False), ("albert-sans", "round", False), ("outfit", "round", False),
    ("plus-jakarta-sans", "round", False),
    ("geologica", "round", True), ("google-sans-flex", "round", True),
    ("jetbrains-mono", "mono", False), ("geist-mono", "mono", False), ("commit-mono", "mono", False),
    ("ibm-plex-mono", "mono", False), ("martian-mono", "mono", False),
    ("atkinson-hyperlegible-mono", "mono", False), ("monaspace-neon", "mono", False),
    ("spline-sans-mono", "mono", False), ("red-hat-mono", "mono", False), ("dm-mono", "mono", False),
    ("lilex", "mono", True),
]

SAMPLE = "the quick brown fox jumps over the lazy dog and runs a small model on a twelve gigabyte card"
SETS = {
    "latin_ext": "ĄąĆćĘęŁłŃńŚśŹźŻżČčĎďĚěŇňŘřŠšŤťŮůŽžĂăȘșȚțŐőŰűĞğİıŞşĀāĒēĪīŌōŪū",
    "cyrillic": "".join(chr(c) for c in range(0x410, 0x450)) + "ЁёІіЇїЄєҐґ",
    "greek": "".join(chr(c) for c in list(range(0x391, 0x3A2)) + list(range(0x3A3, 0x3AA)) + list(range(0x3B1, 0x3CA))) + "άέήίόύώ",
    "vietnamese": "ẠạẢảẤấẦầẨẩẪẫẬậĐđƠơƯưỨứỪừỬửỮữỰự",
    "symbols": "→←↑↓✓×≥≤≠≈…·•°µ−–—",
}


def feat_tags(font):
    tags = set()
    for t in ("GSUB", "GPOS"):
        if t in font and font[t].table.FeatureList:
            for fr in font[t].table.FeatureList.FeatureRecord:
                tags.add(fr.FeatureTag)
    return sorted(tags)


def stem_of_l(font, loc, xh_units):
    """Horizontal thickness of the stem of 'l' at half x-height, in font units."""
    gs = font.getGlyphSet(location=loc) if loc else font.getGlyphSet()
    name = font.getBestCmap()[ord("l")]
    bp = BoundsPen(gs)
    gs[name].draw(bp)
    if not bp.bounds:
        return 0
    x0, _, x1, _ = bp.bounds
    y = xh_units * 0.5
    inside = 0
    x = x0 - 1
    while x <= x1 + 1:
        p = PointInsidePen(gs, (x + 0.5, y))
        gs[name].draw(p)
        if p.getResult():
            inside += 1
        x += 1
    return inside


def bowl_of_o(font, loc):
    """Outer contour of 'o': (width/height, area / bounding-box area). A circle gives (1.0, 0.785)."""
    gs = font.getGlyphSet(location=loc) if loc else font.getGlyphSet()
    name = font.getBestCmap()[ord("o")]
    rec = DecomposingRecordingPen(gs)
    gs[name].draw(rec)
    contours, cur = [], []
    for op, args in rec.value:
        if op == "moveTo" and cur:
            contours.append(cur)
            cur = []
        cur.append((op, args))
    if cur:
        contours.append(cur)
    best = None
    for c in contours:
        ap = AreaPen(gs)
        bp = BoundsPen(gs)
        for op, args in c:
            getattr(ap, op)(*args)
            getattr(bp, op)(*args)
        if not bp.bounds:
            continue
        a = abs(ap.value)
        if best is None or a > best[0]:
            best = (a, bp.bounds)
    a, (x0, y0, x1, y1) = best
    w, h = x1 - x0, y1 - y0
    return round(w / h, 3), round(a / (w * h), 3)


def dot_of_i(font, loc):
    """Squareness of the tittle of 'i' (area / bounding box). Circle 0.785, square 1.0."""
    gs = font.getGlyphSet(location=loc) if loc else font.getGlyphSet()
    name = font.getBestCmap()[ord("i")]
    rec = DecomposingRecordingPen(gs)
    gs[name].draw(rec)
    contours, cur = [], []
    for op, args in rec.value:
        if op == "moveTo" and cur:
            contours.append(cur)
            cur = []
        cur.append((op, args))
    if cur:
        contours.append(cur)
    best = None
    for c in contours:
        ap, bp = AreaPen(gs), BoundsPen(gs)
        for op, args in c:
            getattr(ap, op)(*args)
            getattr(bp, op)(*args)
        if not bp.bounds:
            continue
        x0, y0, x1, y1 = bp.bounds
        if best is None or y1 > best[0]:  # the highest contour is the dot
            best = (y1, abs(ap.value) / ((x1 - x0) * (y1 - y0)))
    return round(best[1], 3) if best and len(contours) > 1 else None


def avg_advance(font, loc):
    gs = font.getGlyphSet(location=loc) if loc else font.getGlyphSet()
    cmap = font.getBestCmap()
    return sum(gs[cmap[ord(c)]].width for c in SAMPLE) / font["head"].unitsPerEm / len(SAMPLE)


def measure(path):
    f = TTFont(path)
    upm = f["head"].unitsPerEm
    os2 = f["OS/2"]
    cmap = f.getBestCmap()
    hmtx = f["hmtx"]

    def adv(ch):
        g = cmap.get(ord(ch))
        return hmtx[g][0] if g else 0

    digits = sorted({adv(c) for c in "0123456789"})
    axes = [(a.axisTag, a.minValue, a.defaultValue, a.maxValue) for a in f["fvar"].axes] if "fvar" in f else []
    return f, dict(
        upm=upm,
        xheight=round(getattr(os2, "sxHeight", 0) / upm, 3),
        capheight=round(getattr(os2, "sCapHeight", 0) / upm, 3),
        asc=round(f["hhea"].ascent / upm, 3), desc=round(f["hhea"].descent / upm, 3),
        avg_adv_em=round(sum(adv(c) for c in SAMPLE) / upm / len(SAMPLE), 4),
        default_digits_tabular=len(digits) == 1,
        zero_adv=round(adv("0") / upm, 3),
        axes=axes,
        features=feat_tags(f),
        glyphs=len(f.getGlyphOrder()),
    )


out = {}
registry = {}
for pid, group, extra in CANDIDATES:
    cands = [os.path.join(NM, s, pid) for s in ("@fontsource-variable", "@fontsource")]
    p = next((c for c in cands if os.path.isdir(c)), None)
    if not p:
        print("MISSING", pid)
        continue
    scope = os.path.basename(os.path.dirname(p))
    meta = json.load(open(os.path.join(p, "metadata.json")))
    variable = bool(meta.get("variable"))
    files = sorted(glob.glob(os.path.join(p, "files", "*.woff2")))
    names = {os.path.basename(f)[len(pid) + 1:-6]: f for f in files}  # e.g. latin-wght-normal
    css_files = {os.path.basename(c) for c in glob.glob(os.path.join(p, "*.css"))}
    lic_text = open(os.path.join(p, "LICENSE")).read()
    lic_line = next((l.strip() for l in lic_text.splitlines() if "licensed under" in l.lower()), "")
    rfn = sorted(set(re.findall(r"Reserved Font Name[s]?\s+[\"“']?([^\"”'.\n)]+)", lic_text + " " + (meta["license"].get("attribution") or ""))))
    rfn = [r.strip() for r in rfn if "any" not in r.lower() and "refers" not in r.lower() and len(r) < 60]

    # ---- which CSS / axis set the specimen uses
    if not variable:
        key = "400"
        css = [w for w in ("400.css", "500.css", "600.css", "700.css") if w in css_files]
        css_italic = [w for w in ("400-italic.css",) if w in css_files]
        italic_kind = "italic files (static)" if css_italic else "none"
    else:
        if pid in ("google-sans-flex", "geologica"):
            key = "full"   # all axes in one file: ROND for Google Sans Flex, slnt + CRSV italics for Geologica
        elif "opsz.css" in css_files:
            key = "opsz"
        else:
            key = "wght"
        css = [f"{key}.css"]
        css_italic = [f"{key}-italic.css"] if f"{key}-italic.css" in css_files else []
        if css_italic:
            italic_kind = "italic file"
        elif pid == "geologica":
            italic_kind = "slnt + CRSV axes (designed italic)"
        elif "slnt" in (meta.get("variable") or {}):
            italic_kind = "slnt axis (oblique)"
        else:
            italic_kind = "none"
    rep = names[f"latin-{key}-normal"]
    font, m = measure(rep)
    upm = m["upm"]

    # ---- coverage over all subsets of the wght (or static 400) set
    wkey = "wght" if variable else "400"
    cover = set()
    for s in meta["subsets"]:
        fpath = names.get(f"{s}-{wkey}-normal")
        if fpath:
            cover |= set(TTFont(fpath, lazy=True).getBestCmap().keys())
    coverage = {k: round(sum(ord(c) in cover for c in v) / len(v), 2) for k, v in SETS.items()}
    missing_symbols = "".join(c for c in SETS["symbols"] if ord(c) not in cover)

    # ---- sizes
    size = lambda n: os.path.getsize(names[n]) if n in names else None
    all_up = sum(size(f"{s}-{wkey}-normal") or 0 for s in meta["subsets"])
    all_it = sum(size(f"{s}-{wkey}-italic") or 0 for s in meta["subsets"])
    all_rep = sum(size(f"{s}-{key}-normal") or 0 for s in meta["subsets"])

    # ---- stems and bowl
    axis_tags = {a[0]: a for a in m["axes"]}
    stems = {}
    bowl = None
    for w in (400, 500, 600, 700):
        try:
            if variable:
                lo, _, hi = axis_tags["wght"][1], axis_tags["wght"][2], axis_tags["wght"][3]
                if not (lo <= w <= hi):
                    continue
                loc = {"wght": w}
                if "opsz" in axis_tags:
                    loc["opsz"] = min(32, axis_tags["opsz"][3])
                st = stem_of_l(font, loc, m["xheight"] * upm)
                if w == 600:
                    bowl = bowl_of_o(font, loc)
            else:
                n = f"latin-{w}-normal"
                if n not in names:
                    continue
                fw = TTFont(names[n])
                st = stem_of_l(fw, None, m["xheight"] * upm)
                if w in (500, 600) :
                    bowl = bowl_of_o(fw, None)
            stems[w] = round(st / upm / m["capheight"], 4)
        except Exception as e:  # keep going, record why
            stems[w] = f"err {e}"

    stem_grid = {}
    if variable:
        lo, hi = axis_tags["wght"][1], axis_tags["wght"][3]
        for w in range(100, 901, 50):
            if lo <= w <= hi:
                loc = {"wght": w}
                if "opsz" in axis_tags:
                    loc["opsz"] = min(32, axis_tags["opsz"][3])
                stem_grid[w] = round(stem_of_l(font, loc, m["xheight"] * upm) / upm / m["capheight"], 4)
    else:
        stem_grid = {w: v for w, v in stems.items() if isinstance(v, float)}

    # width economy: at nominal 400, and at the weight whose stem equals 0.118 x cap height (equal colour)
    def weight_for(target):
        ks = sorted(stem_grid)
        for a, b in zip(ks, ks[1:]):
            if stem_grid[a] <= target <= stem_grid[b]:
                return a + (b - a) * (target - stem_grid[a]) / (stem_grid[b] - stem_grid[a])
        return None
    try:
        if variable:
            base = {"opsz": axis_tags["opsz"][1] if pid != "inter" else 14} if "opsz" in axis_tags else {}
            if "opsz" in axis_tags:
                base = {"opsz": max(axis_tags["opsz"][1], min(14, axis_tags["opsz"][3]))}
            w400 = min(max(400, axis_tags["wght"][1]), axis_tags["wght"][3])
            adv400 = round(avg_advance(font, dict(base, wght=w400)), 4)
            wn = weight_for(0.118)
            adv_norm = round(avg_advance(font, dict(base, wght=wn)), 4) if wn else None
            dot = dot_of_i(font, {"wght": min(max(500, axis_tags["wght"][1]), axis_tags["wght"][3])})
        else:
            f400 = TTFont(names["latin-400-normal"])
            adv400, wn, adv_norm = round(avg_advance(f400, None), 4), None, None
            dot = dot_of_i(f400, None)
    except Exception as e:
        adv400, wn, adv_norm, dot = None, None, None, f"err {e}"

    feats = m["features"]
    entry = dict(
        id=pid, package=f"{scope}/{pid}", family=meta["family"], group=group, extra=extra,
        variable=variable, fs_axes=sorted((meta.get("variable") or {}).keys()),
        css_family=meta["family"] + (" Variable" if variable else ""),
        css=css, css_italic=css_italic, italic_kind=italic_kind, spec_key=key,
        italic_var="CRSV:1" if pid == "geologica" else "",
        weights=[meta["weights"][0], meta["weights"][-1]], styles=meta["styles"], subsets=meta["subsets"],
        version=meta.get("version"), last_modified=meta.get("lastModified"),
        license=meta["license"]["type"], license_file_line=lic_line, reserved_font_names=rfn,
        attribution=meta["license"].get("attribution"), source=meta.get("source"),
        coverage=coverage, missing_symbols=missing_symbols,
        has_tnum="tnum" in feats, has_zero="zero" in feats, has_case="case" in feats,
        stylistic=[t for t in feats if re.match(r"(ss|cv)\d\d", t)],
        latin_wght_bytes=size(f"latin-{wkey}-normal"), latin_rep_bytes=os.path.getsize(rep),
        latin_italic_bytes=size(f"latin-{wkey}-italic"),
        all_subsets_wght_bytes=all_up, all_subsets_italic_bytes=all_it, all_subsets_rep_bytes=all_rep,
        stem_over_cap=stems, stem_grid=stem_grid, bowl_o=bowl, dot_i=dot,
        adv_400=adv400, weight_equal_colour=round(wn) if wn else None, adv_equal_colour=adv_norm,
        **m,
    )
    out[pid] = entry
    registry[pid] = {k: entry[k] for k in (
        "id", "package", "family", "group", "extra", "css_family", "css", "css_italic", "italic_kind", "italic_var",
        "capheight", "xheight", "has_tnum", "has_zero", "stylistic", "fs_axes", "license", "variable", "stem_grid")}

os.makedirs(os.path.join(ROOT, "out"), exist_ok=True)
json.dump(out, open(os.path.join(ROOT, "out", "facts.json"), "w"), indent=1, ensure_ascii=False)
with open(os.path.join(ROOT, "fonts.js"), "w") as fh:
    fh.write("// Generated by tools/facts.py from node_modules metadata. Do not edit by hand.\n")
    fh.write("window.FONTS = " + json.dumps(registry, indent=1, ensure_ascii=False) + ";\n")

hdr = f"{'id':28} {'lic':8} {'axes':22} {'wght':9} {'italic':20} {'xh':5} {'cap':5} {'adv':6} {'tn':2} {'z':1} {'LatX':4} {'Cyr':4} {'Grk':4} {'Viet':4} {'latK':5} {'allK':6} {'stem400/500/600/700 (÷cap)':30} bowl(o)"
print(hdr)
for k, v in out.items():
    st = "/".join(f"{v['stem_over_cap'].get(w, 0):.3f}" if isinstance(v['stem_over_cap'].get(w, 0), float) else "  -  " for w in (400, 500, 600, 700))
    c = v["coverage"]
    print(f"{k:28} {v['license']:8} {','.join(v['fs_axes']) or 'static':22} {v['weights'][0]}-{v['weights'][1]:<5} {v['italic_kind']:20} {v['xheight']:<5} {v['capheight']:<5} {v['avg_adv_em']:<6} {'Y' if v['has_tnum'] else ('d' if v['default_digits_tabular'] else '-'):2} {'Y' if v['has_zero'] else '-':1} {c['latin_ext']:<4} {c['cyrillic']:<4} {c['greek']:<4} {c['vietnamese']:<4} {v['latin_wght_bytes']/1024:5.1f} {v['all_subsets_wght_bytes']/1024:6.1f} {st:30} {v['bowl_o']} dot:{v['dot_i']} adv400:{v['adv_400']} eq:{v['weight_equal_colour']}/{v['adv_equal_colour']} {v['stylistic']} miss:{v['missing_symbols']} rfn:{v['reserved_font_names']}")
