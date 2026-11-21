#!/usr/bin/env python3
"""Print the Markdown facts tables used in REPORT.md from out/facts.json."""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
F = json.load(open(os.path.join(ROOT, "out", "facts.json")))
kb = lambda b: f"{b / 1024:.0f}" if b else "–"
yn = lambda v: "yes" if v >= 0.95 else ("part" if v > 0.1 else "no")
IT = {"italic file": "yes", "italic files (static)": "yes", "slnt axis (oblique)": "oblique, slnt axis",
      "slnt + CRSV axes (designed italic)": "yes, on slnt + CRSV axes", "none": "no"}


def dotname(d):
    """Area of the tittle over its bounding box: a circle is 0.785, a square 1.0."""
    if not isinstance(d, float):
        return "?"
    return "round" if d < 0.805 else ("square" if d > 0.95 else "soft square")


def axes(v):
    return ", ".join(a for a in v["fs_axes"] if a != "ital") if v["variable"] else "static only"


print("### Sans\n")
print("| Family | Group | License | Axes | Weights | Italic | Lat-Ext | Cyrillic | Greek | Viet | x-height | Cap | Width* | tnum | zero | o w/h | i-dot | stem 400 / 500 / 600 ÷ cap | Latin KB | All subsets KB |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for k, v in F.items():
    if v["group"] == "mono":
        continue
    c = v["coverage"]
    st = " / ".join(f"{v['stem_over_cap'].get(str(w), 0):.3f}" if isinstance(v['stem_over_cap'].get(str(w)), float) else "–" for w in (400, 500, 600))
    tn = "yes" if v["has_tnum"] else ("always tabular" if v["default_digits_tabular"] else "**no**")
    dot = dotname(v["dot_i"])
    grp = v["group"] + (" (added)" if v["extra"] else "")
    print(f"| {v['family']} | {grp} | {v['license']} | {axes(v)} | {v['weights'][0]}–{v['weights'][1]} | {IT[v['italic_kind']]} | {yn(c['latin_ext'])} | {yn(c['cyrillic'])} | {yn(c['greek'])} | {yn(c['vietnamese'])} | {v['xheight']:.3f} | {v['capheight']:.3f} | {v['adv_equal_colour'] or v['adv_400']:.3f} | {tn} | {'yes' if v['has_zero'] else 'no'} | {v['bowl_o'][0]:.2f} | {dot} | {st} | {kb(v['latin_wght_bytes'])} | {kb(v['all_subsets_wght_bytes'])} |")

print("\n### Mono\n")
print("| Family | License | Variable on Fontsource | Weights | Italic | Lat-Ext | Cyrillic | Greek | Viet | x-height | Advance | o w/h | i-dot | → in the package | Latin KB | All subsets KB |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for k, v in F.items():
    if v["group"] != "mono":
        continue
    c = v["coverage"]
    dot = dotname(v["dot_i"])
    arrows = "missing" if "→" in v["missing_symbols"] else "yes"
    per = "" if v["variable"] else " per weight"
    name = v["family"] + (" (added)" if v["extra"] else "")
    print(f"| {name} | {v['license']} | {'yes' if v['variable'] else '**no, static files**'} | {v['weights'][0]}–{v['weights'][1]} | {IT[v['italic_kind']]} | {yn(c['latin_ext'])} | {yn(c['cyrillic'])} | {yn(c['greek'])} | {yn(c['vietnamese'])} | {v['xheight']:.3f} | {v['adv_400']:.3f} | {v['bowl_o'][0]:.2f} | {dot} | {arrows} | {kb(v['latin_wght_bytes'])}{per} | {kb(v['all_subsets_wght_bytes'])}{per} |")

print("\n### Extra numbers\n")
for k in ("geologica", "inter", "jetbrains-mono", "geist-mono", "figtree", "onest", "google-sans-flex"):
    v = F[k]
    print(f"- {v['family']}: specimen file set `{v['spec_key']}`: latin {kb(v['latin_rep_bytes'])} KB, all subsets {kb(v['all_subsets_rep_bytes'])} KB; italic files all subsets {kb(v['all_subsets_italic_bytes'])} KB; equal-colour weight {v['weight_equal_colour']}; stems {v['stem_grid']}; missing symbols '{v['missing_symbols']}'; RFN {v['reserved_font_names']}; version {v['version']} ({v['last_modified']}); glyphs(latin) {v['glyphs']}")

print("\n### Features\n")
print("| Family | Optical size | Tabular figures on data | Slashed zero on data | Italic | Other |")
print("|---|---|---|---|---|---|")
for k, v in F.items():
    mono = v["group"] == "mono"
    opsz = "auto (`font-optical-sizing`)" if "opsz" in v["fs_axes"] else "no axis"
    if mono:
        tn = "monospaced"
    else:
        tn = "`tnum` on" if v["has_tnum"] else ("always tabular" if v["default_digits_tabular"] else "**not available**")
    zero = "`zero` on" if v["has_zero"] else ("no feature; default 0 is slashed or dotted" if mono else "no feature")
    it = {"italic file": "italic file", "italic files (static)": "italic file (400 only loaded)", "slnt axis (oblique)": "slnt axis",
          "slnt + CRSV axes (designed italic)": "slnt axis + `\"CRSV\" 1`", "none": "none: browser slants the roman"}[v["italic_kind"]]
    other = "ligatures off (`calt`, `liga`)" if mono else ("ROND 0 and 100 both captured" if k == "google-sans-flex" else ("SHRP 0; text weights 250/350/450 in the tuned run" if k == "geologica" else ""))
    print(f"| {v['family']} | {opsz} | {tn} | {zero} | {it} | {other} |")
