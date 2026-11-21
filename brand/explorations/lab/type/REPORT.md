# Demido Studio type lab: report

Date: 2026-11-20. Scope: one sans (UI, text, display, wordmark) and one mono, both OFL, bundled with the app.
Everything here is rebuilt by `pnpm install && tools/run-all.sh` (about 1 minute).

## 1. Decision

| | Sans | Mono |
|---|---|---|
| **Recommended** | **Geologica** (variable, OFL-1.1), run at text weights 250 / 350 / 450 | **JetBrains Mono** (variable, OFL-1.1) |
| Runner-up | Inter (variable, OFL-1.1) | JetBrains Mono |

**Why Geologica.** It is the only candidate that does all three things the brief asks for at once:

1. **It is built from the same shapes as the mark.** Its `o` is a near circle: width ÷ height 1.04, and a fill of 0.791, the closest of all 19 sans to a true circle's 0.785. Its dots are round and its joins are smooth. Beside the ring and the two solid dots of the symbol it reads as the same hand. See `out/sheet-lockups.png`.
2. **It covers Latin Extended, Cyrillic, Greek and Vietnamese natively.** None of the listed round candidates does that with italics: Figtree, DM Sans, Albert Sans, Outfit and Plus Jakarta Sans have no Cyrillic and no Greek, Onest has no Greek and no italics, Manrope has no italics. A chat app prints whatever a model writes, including Russian and Greek letters in maths, so this is a floor, not a bonus.
3. **It holds at 12 to 14 px at 1x on dark** once its weights are tuned (see the caveat below), with tabular figures and a designed italic. See `out/sheet-sans-ui-dark-1x.png` and `out/sheet-sans-ui-dark-1x-zoom.png`.

It is also not a habit default. To my knowledge no product in this category uses it.

**The one real cost.** Geologica is drawn dark. Its Regular (400) has the stroke of most families' Semibold: stem ÷ cap height is 0.157, against 0.114 for Inter 400. At nominal weights the UI looks loud (`out/sheet-sans-ui-dark-1x-all-b.png`, tile "Geologica"). The fix is to use the weight axis lower: **250 / 350 / 450** for regular / medium / semibold gives the same stroke as Inter 400 / 500 / 600 (0.119 / 0.144 / 0.170). All "tuned" tiles use these values. The design tokens must carry them; nothing may assume 400 and 600.

**Why JetBrains Mono.** It is the only mono on the brief's list that is variable on Fontsource, has italics and covers the same scripts as Geologica (Latin Extended, Cyrillic, Greek, Vietnamese). Lilex, which I added, is the only other one. It is also the sturdiest at 10.5 to 13 px at 1x. Its ovals are squarer than the mark (o width ÷ height 0.80). I accept that on purpose: the sans carries the mark's softness, the mono is the instrument voice for exact values. This is a popular face chosen on function, not by habit. If the director wants a rounder instrument voice, the alternative is Geist Mono (section 5).

**Why Inter is the runner-up and not the pick.** Inter is the benchmark for small text and has every script, italics and an optical-size axis. It is also the anonymous default of this product category, and its forms (oval `o` at 0.94, grotesque construction) only tolerate the mark; they do not echo it. Choose it if the weight remapping of Geologica is judged too risky. Nothing else changes: same mono, same sizes, standard weights 400 / 500 / 600.

## 2. Recommended wordmark treatment

Tested in `out/sheet-wordmark-recommended.png` (dark) and `out/sheet-wordmark-recommended-light.png`.

| Property | Value |
|---|---|
| Text | `Demido Studio`, title case |
| Typeface | Geologica, SHRP 0, upright |
| Weights | `Demido` 400, `Studio` 300 |
| Tracking | −0.01 em at 32 px and above; 0 at 20 px and below |
| Symbol height | 2.0 × cap height (1.40 em), centred on the cap height |
| Gap symbol to wordmark | 0.30 × symbol height (0.42 em) |
| Small sizes (wordmark 20 px or less, the title bar) | `Demido` 450, `Studio` 350 |
| Short form | symbol + `Demido` 400 (450 at 20 px or less) |
| Master files | convert to outlines; the app title bar may use live text |

Reasons, all visible in the sheet:

1. **Stroke match.** The symbol's inner stroke is 6.8 % of its height. At 2.0 × cap height that is 0.136 × cap height. `Studio` at 300 has a stem of 0.131 (0.97 × the mark's stroke): the same line. `Demido` at 400 is 0.157 (1.16 ×): the name is a little heavier than the mark, as the two solid dots are.
2. **Two weights, one family.** `Demido` + lighter `Studio` separates brand from product without a second typeface, and leaves room for `Demido Server` or `Demido CLI`. One weight for both words (350) is the fallback where two weights cannot be told apart, for example at 14 px.
3. **Symbol at 2.0 × cap height, not 1.6.** At 1.6 the wordmark stems are 1.45 × the mark's stroke and the mark looks thin and cramped. At 2.0 the two balance.
4. **Title case.** Lowercase `demido studio` is friendly and makes the two `d` forms of the mark explicit, but lowercase wordmarks are a common convention among developer tools, and it reads less like a manual. Keep lowercase for the CLI and package name, set in the mono.
5. **Do not use weight 500 or more for the wordmark in Geologica.** It overpowers the symbol (rows "450" and the nominal Geologica tile in `out/sheet-lockups-all-round.png`).

If Inter is chosen instead: `Demido` 500 (1.04 × the mark's stroke at 2.0 × cap height), `Studio` 400, same geometry. See `out/sheet-wordmark-runner-up.png`.

## 3. Recommended values

**Sans: Geologica** (`@fontsource-variable/geologica`, file set `full`: axes wght 100 to 900, slnt −12 to 0, CRSV 0 to 1, SHRP 0 to 100).

| Token | Value |
|---|---|
| Regular / medium / semibold | 250 / 350 / 450 (captured in every "tuned" tile) |
| Bold, and text of 12 px or less | 550 and 300. Suggested by the stem table; not captured. |
| Display (32 and 56 px) | 450, tracking −0.02 em |
| Italic | `font-style: italic` (drives slnt to −12) plus `font-variation-settings: "CRSV" 1` (cursive forms). Verified pixel-identical to setting the axes by hand. |
| SHRP | 0. At 100 the joins get notches and stop matching the mark. |
| Numbers that carry data | `font-variant-numeric: tabular-nums` |
| Sizes | 14 px default UI and chat text, 13 px dense lists, 12 px helper text, 11 px uppercase labels (+0.06 em). Geologica's x-height is 0.48 em against 0.546 for Inter, so do not go one size smaller than you would with Inter. |
| `<b>`, `<strong>`, Markdown bold | override to 450. The browser default of 700 is far too heavy here. |

**Mono: JetBrains Mono** (`@fontsource-variable/jetbrains-mono`, upright and italic files, wght 100 to 800).

| Token | Value |
|---|---|
| Regular / medium | 400 / 500 |
| Ligatures | off: `font-variant-ligatures: none; font-feature-settings: "calt" 0, "liga" 0`. `!=` and `->` stay as typed. |
| Inline code size | 0.90 em of the surrounding sans (x-height 0.55 × 0.90 = 0.495 against 0.48). The specimen uses 0.93 em. |
| Zero | marked by default, so `0` and `O` differ; no feature needed |

**Payload** (woff2 from Fontsource, all subsets): Geologica 175 KB (one file set, roman and italic) + JetBrains Mono 84 KB upright + 90 KB italic = about 350 KB. Latin only: 47 + 39 + 42 = 128 KB. The runner-up pair is about 870 KB with Inter's optical-size files.

## 4. Sans: ranked shortlist

Criteria, in order of weight: harmony with the logo, legibility at 12 to 14 px at 1x on dark, digits and punctuation, x-height and width, character, weight range, italics, language coverage, file size, license. All 19 pass the license test.

| # | Family | Verdict |
|---|---|---|
| 1 | **Geologica** (added) | Roundest forms, round dots, full coverage, italic on axes, 92 to 175 KB. Needs tuned weights. No slashed zero; `I` and `l` are the same bar (as in most of the list). |
| 2 | **Inter** | Best small text, every script, optical size, italics. Anonymous. Oval bowls only tolerate the mark. The Fontsource build has no `zero`, `case`, `ss` or `cv` features. |
| 3 | **Figtree** | The best fit to the mark among the listed candidates: round, calm, clean italics, 30 KB. Latin only. Would be my pick for a Latin-only product. |
| 4 | Geist | Clean and legible, Cyrillic, italics, `l` with a tail. No Greek, square dots, and it is so widely used as a starter default that it will not look like ours. |
| 5 | Onest | Round, technical, Cyrillic, arrows included. No italics (the browser slants the roman) and no Greek. |
| 6 | Manrope | Cyrillic and Greek. No italics, oval `o` (0.93), square dots, tight at 13 px. |
| 7 | Hanken Grotesk | Calm and clear, `l` with a tail. Latin and Vietnamese only; figures are always tabular. |
| 8 | Instrument Sans | Precise and distinctive, circular `o`. Latin only, weights 400 to 700 only, square dots. |
| 9 | IBM Plex Sans | Serifed `I` and tailed `l`, so `Il1|` never collide (only Atkinson does the same); Cyrillic and Greek. Engineered, squarish (0.89) and tied to IBM; it argues with a soft mark. Stops at 700. |
| 10 | DM Sans | Round and friendly with optical size. No tabular figures, and `0` is close to `O` (0.87 of its width; Inter 0.79). Latin only. |
| 11 | Reddit Sans | Friendly, round dots. Latin only, carries another brand's name. |
| 12 | Mona Sans | Wide and clean. Latin only, GitHub's face. |
| 13 | Plus Jakarta Sans | Perfectly circular `o` (1.00), but its word space is 0.17 em against 0.25 to 0.28 for Inter, Figtree and Geologica, so 14 px text runs together (see the chat sheet). No Cyrillic, no Greek. |
| 14 | Albert Sans | No tabular figures. Latin only. Generic. |
| 15 | Wix Madefor Text | Friendly, Cyrillic. No tabular figures, weights start at 400. |
| 16 | Public Sans | Neutral, narrow `o` (0.91). Latin only. |
| 17 | Atkinson Hyperlegible Next | Honest legibility design with serifed `I` and tailed `l`, but its quirks become the brand. Latin only. |
| 18 | Outfit | Closest geometry to the ring. x-height 0.46, no italics, Latin only: a logo face, not an interface face. |
| 19 | Google Sans Flex (added) | Technically the best file on the table: roundness axis, optical size, grade, slashed zero. Rejected: it is Google's identity (Gemini is a neighbour), this build has no Cyrillic or Greek, and Latin with all axes is 1.37 MB. |

What the evidence showed that I did not expect:

- **Weight is not comparable by number.** Geologica 400 has the stem of Inter 550. Manrope 600 has the stem of Inter 500. Lockups must be matched by stem, not by weight name.
- **Three round candidates fail on figures.** DM Sans, Albert Sans and Wix Madefor Text have no tabular figures in these builds. A product that shows VRAM, tokens per second and context sizes cannot use them for data.
- **Round dots are rarer than they look.** Geist, Manrope, Plus Jakarta Sans, Hanken Grotesk, Instrument Sans and Public Sans have square dots. The mark has two solid round dots.

## 5. Mono: ranked shortlist

All shown with Geologica in `out/sheet-mono.png`, `out/sheet-mono-settings-1x.png`, `out/sheet-mono-zoom.png`.

| # | Family | Verdict |
|---|---|---|
| 1 | **JetBrains Mono** | Variable, italics, Latin Extended + Cyrillic + Greek + Vietnamese, 84 KB. Sturdiest at small sizes. Squarer than the mark. |
| 2 | **Geist Mono** | Rounder (0.90) and quieter, variable 100 to 900, Cyrillic. No Greek, square dots. Take it if the mono should feel softer; add JetBrains Mono's Greek subset behind it (both advance 0.6 em, so columns still align). |
| 3 | Commit Mono | The quietest and most neutral, round dots, arrows included. Static files only on Fontsource; no Cyrillic in this build. |
| 4 | Lilex (added) | Variable descendant of IBM Plex Mono with Cyrillic, Greek and italics. Typewriter details suit "aircraft manual", but it is light at 10.5 to 11.5 px. |
| 5 | Red Hat Mono | The best shape match to the mark and to Geologica (o 0.98, round dots). Latin only and light: needs 450 as regular. |
| 6 | IBM Plex Mono | Static only on Fontsource, no Greek. Use Lilex instead. |
| 7 | Monaspace Neon | Clear and large. Static only on Fontsource, Latin only, wider cell (0.62 em). |
| 8 | DM Mono | Soft and pleasant. Three static weights, Latin only. |
| 9 | Spline Sans Mono | Latin only, no reason to prefer it. |
| 10 | Atkinson Hyperlegible Mono | Round, but a wide cell (0.632 em), light, Latin only. |
| 11 | Martian Mono | 0.7 em cell and 0.6 x-height: too wide and too loud for a quiet interface. No italics. |

## 6. Facts

Measured with fontTools from the installed Fontsource files (`tools/facts.py`, full data in `out/facts.json`). Coverage is the union of all subsets a package ships.

- **Width\*** = average advance in em of a lowercase English sample at the weight whose stem is 0.118 × cap height, so that light and dark designs compare fairly (for static fonts: weight 400).
- **o w/h** = width ÷ height of the outer contour of `o` at weight 600. A circle is 1.00.
- **i-dot** = shape of the tittle (area ÷ bounding box: round below 0.805, square above 0.95).
- **stem ÷ cap** = thickness of the stem of `l` as a share of cap height. The symbol's inner stroke is 0.122 × cap height when the symbol is 1.8 × cap height tall, 0.136 at 2.0 ×.
- **Italic** = the family ships an italic or oblique design (as files or on an axis). "no" means the browser slants the roman.
- **Latin KB / All subsets KB** = variable woff2 with the weight axis only, upright. Italic files are extra (about the same again).

| Family | Group | License | Axes | Weights | Italic | Lat-Ext | Cyrillic | Greek | Viet | x-height | Cap | Width* | tnum | zero | o w/h | i-dot | stem 400 / 500 / 600 ÷ cap | Latin KB | All subsets KB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Inter | neutral | OFL-1.1 | opsz, wght | 100–900 | yes | yes | yes | yes | yes | 0.546 | 0.728 | 0.487 | yes | no | 0.94 | round | 0.114 / 0.141 / 0.170 | 47 | 213 |
| IBM Plex Sans | neutral | OFL-1.1 | wdth, wght | 100–700 | yes | yes | yes | yes | yes | 0.516 | 0.698 | 0.457 | always tabular | no | 0.89 | soft square | 0.115 / 0.156 / 0.183 | 45 | 159 |
| Mona Sans | neutral | OFL-1.1 | wdth, wght | 200–900 | yes | yes | no | no | yes | 0.517 | 0.729 | 0.475 | yes | no | 0.96 | soft square | 0.111 / 0.141 / 0.172 | 39 | 61 |
| Instrument Sans | neutral | OFL-1.1 | wdth, wght | 400–700 | yes | yes | no | no | no | 0.510 | 0.720 | 0.464 | yes | no | 1.03 | square | 0.111 / 0.143 / 0.176 | 29 | 40 |
| Hanken Grotesk | neutral | OFL-1.1 | wght | 100–900 | yes | yes | no | no | yes | 0.493 | 0.697 | 0.459 | always tabular | no | 0.96 | square | 0.120 / 0.139 / 0.158 | 34 | 64 |
| Atkinson Hyperlegible Next | neutral | OFL-1.1 | wght | 200–800 | yes | yes | no | no | no | 0.496 | 0.668 | 0.447 | yes | no | 0.97 | round | 0.126 / 0.159 / 0.183 | 33 | 52 |
| Geist | neutral | OFL-1.1 | wght | 100–900 | yes | yes | yes | no | yes | 0.530 | 0.710 | 0.476 | yes | no | 0.94 | square | 0.118 / 0.149 / 0.180 | 29 | 75 |
| Wix Madefor Text | neutral | OFL-1.1 | wght | 400–800 | yes | yes | yes | no | yes | 0.492 | 0.715 | 0.475 | **no** | no | 0.97 | round | 0.108 / 0.140 / 0.162 | 20 | 55 |
| Public Sans | neutral | OFL-1.1 | wght | 100–900 | yes | yes | no | no | yes | 0.517 | 0.723 | 0.474 | yes | no | 0.91 | square | 0.126 / 0.146 / 0.165 | 26 | 52 |
| Reddit Sans | neutral | OFL-1.1 | wght | 200–900 | yes | yes | no | no | yes | 0.518 | 0.720 | 0.459 | yes | no | 0.98 | round | 0.117 / 0.138 / 0.160 | 41 | 85 |
| Figtree | round | OFL-1.1 | wght | 300–900 | yes | yes | no | no | no | 0.500 | 0.700 | 0.457 | yes | no | 0.98 | round | 0.114 / 0.134 / 0.160 | 20 | 30 |
| Onest | round | OFL-1.1 | wght | 100–900 | no | yes | yes | no | yes | 0.527 | 0.707 | 0.478 | yes | no | 0.96 | round | 0.120 / 0.146 / 0.173 | 33 | 141 |
| DM Sans | round | OFL-1.1 | opsz, wght | 100–900 | yes | yes | no | no | no | 0.526 | 0.700 | 0.474 | **no** | no | 0.97 | round | 0.120 / 0.143 / 0.171 | 36 | 54 |
| Manrope | round | OFL-1.1 | wght | 200–800 | no | yes | yes | yes | yes | 0.540 | 0.720 | 0.470 | yes | no | 0.93 | square | 0.102 / 0.124 / 0.145 | 24 | 73 |
| Albert Sans | round | OFL-1.1 | wght | 100–900 | yes | yes | no | no | no | 0.500 | 0.700 | 0.472 | **no** | no | 0.99 | round | 0.107 / 0.136 / 0.161 | 31 | 54 |
| Outfit | round | OFL-1.1 | wght | 100–900 | no | yes | no | no | no | 0.460 | 0.676 | 0.449 | yes | no | 1.03 | round | 0.133 / 0.163 / 0.195 | 32 | 46 |
| Plus Jakarta Sans | round | OFL-1.1 | wght | 200–800 | yes | yes | no | no | yes | 0.536 | 0.745 | 0.477 | yes | no | 1.00 | square | 0.101 / 0.126 / 0.152 | 27 | 58 |
| Geologica | round (added) | OFL-1.1 | CRSV, SHRP, slnt, wght | 100–900 | yes, on slnt + CRSV axes | yes | yes | yes | yes | 0.480 | 0.700 | 0.477 | yes | no | 1.04 | round | 0.157 / 0.183 / 0.209 | 24 | 92 |
| Google Sans Flex | round (added) | OFL-1.1 | GRAD, ROND, opsz, slnt, wdth, wght | 100–900 | oblique, slnt axis | yes | no | no | yes | 0.510 | 0.716 | 0.485 | yes | yes | 1.01 | round | 0.117 / 0.151 / 0.185 | 50 | 185 |

Geologica with its italic axes (file set `full`) is 47 KB for Latin and 175 KB for all subsets. Inter with optical size is 71 KB and 332 KB upright, and as much again for the italic. Google Sans Flex with all axes is 1 369 KB for Latin alone.

| Family | License | Variable on Fontsource | Weights | Italic | Lat-Ext | Cyrillic | Greek | Viet | x-height | Advance | o w/h | i-dot | → in the package | Latin KB | All subsets KB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| JetBrains Mono | OFL-1.1 | yes | 100–800 | yes | yes | yes | yes | yes | 0.550 | 0.600 | 0.80 | soft square | missing | 39 | 84 |
| Geist Mono | OFL-1.1 | yes | 100–900 | yes | yes | yes | no | yes | 0.530 | 0.600 | 0.90 | square | missing | 23 | 69 |
| Commit Mono | OFL-1.1 | **no, static files** | 200–700 | yes | yes | no | yes | no | 0.540 | 0.600 | 0.86 | round | yes | 47 per weight | 47 per weight |
| IBM Plex Mono | OFL-1.1 | **no, static files** | 100–700 | yes | yes | yes | no | yes | 0.516 | 0.600 | 0.93 | soft square | missing | 14 per weight | 48 per weight |
| Martian Mono | OFL-1.1 | yes | 100–800 | no | yes | yes | no | no | 0.600 | 0.700 | 0.89 | soft square | missing | 23 | 52 |
| Atkinson Hyperlegible Mono | OFL-1.1 | yes | 200–800 | yes | yes | no | no | no | 0.496 | 0.632 | 0.99 | round | missing | 17 | 28 |
| Monaspace Neon | OFL-1.1 | **no, static files** | 200–800 | yes | yes | no | no | no | 0.513 | 0.620 | 0.94 | soft square | yes | 43 per weight | 43 per weight |
| Spline Sans Mono | OFL-1.1 | yes | 300–700 | yes | yes | no | no | no | 0.545 | 0.600 | 0.88 | round | missing | 36 | 56 |
| Red Hat Mono | OFL-1.1 | yes | 300–700 | yes | yes | no | no | no | 0.488 | 0.600 | 0.98 | round | missing | 22 | 33 |
| DM Mono | OFL-1.1 | **no, static files** | 300–500 | yes | yes | no | no | no | 0.496 | 0.600 | 0.95 | round | missing | 14 per weight | 24 per weight |
| Lilex (added) | OFL-1.1 | yes | 100–700 | yes | yes | yes | yes | yes | 0.516 | 0.600 | 0.93 | soft square | missing | 39 | 110 |

Notes on the candidate list:

- **License.** Every package states OFL-1.1 in `metadata.json` and in its `LICENSE` file. None was dropped. Mona Sans and Monaspace declare a Reserved Font Name; a modified or re-subset build of those must be renamed. Geologica, Inter, JetBrains Mono and Geist Mono declare none.
- **Variable on Fontsource.** All 19 sans are. Four monos are not and ship static files only: Commit Mono, IBM Plex Mono, Monaspace Neon, DM Mono.
- **Added candidates (three).** Geologica: round forms with Cyrillic, Greek and an italic. Among the listed round candidates only Manrope has both scripts, and it has no italic. Google Sans Flex: to test whether a roundness axis (rounded terminals) answers the mark better than round bowls. It does not justify its costs. Lilex: IBM Plex Mono is static on Fontsource and has no Greek there; Lilex is the variable descendant with Cyrillic and Greek.
- **Looked at and removed.** Commissioner and Sofia Sans (added by the interrupted run, before the logo was known), Nunito Sans and Google Sans Code were installed, looked at in the lockup and UI tiles, and removed to stay within three additions. None would have ranked in the top five.

### OpenType features and axes enabled in the specimen

| Family | Optical size | Tabular figures on data | Slashed zero on data | Italic | Other |
|---|---|---|---|---|---|
| Inter | auto (`font-optical-sizing`) | `tnum` on | no feature | italic file |  |
| IBM Plex Sans | no axis | always tabular | no feature | italic file |  |
| Mona Sans | no axis | `tnum` on | no feature | italic file |  |
| Instrument Sans | no axis | `tnum` on | no feature | italic file |  |
| Hanken Grotesk | no axis | always tabular | no feature | italic file |  |
| Atkinson Hyperlegible Next | no axis | `tnum` on | no feature | italic file |  |
| Geist | no axis | `tnum` on | no feature | italic file |  |
| Wix Madefor Text | no axis | **not available** | no feature | italic file |  |
| Public Sans | no axis | `tnum` on | no feature | italic file |  |
| Reddit Sans | no axis | `tnum` on | no feature | italic file |  |
| Figtree | no axis | `tnum` on | no feature | italic file |  |
| Onest | no axis | `tnum` on | no feature | none: browser slants the roman |  |
| DM Sans | auto (`font-optical-sizing`) | **not available** | no feature | italic file |  |
| Manrope | no axis | `tnum` on | no feature | none: browser slants the roman |  |
| Albert Sans | no axis | **not available** | no feature | italic file |  |
| Outfit | no axis | `tnum` on | no feature | none: browser slants the roman |  |
| Plus Jakarta Sans | no axis | `tnum` on | no feature | italic file |  |
| Geologica | no axis | `tnum` on | no feature | slnt axis + `"CRSV" 1` | SHRP 0; text weights 250/350/450 in the tuned run |
| Google Sans Flex | auto (`font-optical-sizing`) | `tnum` on | `zero` on | slnt axis | ROND 0 and 100 both captured |
| JetBrains Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |
| Geist Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |
| Commit Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file (400 only loaded) | ligatures off (`calt`, `liga`) |
| IBM Plex Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file (400 only loaded) | ligatures off (`calt`, `liga`) |
| Martian Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | none: browser slants the roman | ligatures off (`calt`, `liga`) |
| Atkinson Hyperlegible Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |
| Monaspace Neon | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file (400 only loaded) | ligatures off (`calt`, `liga`) |
| Spline Sans Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |
| Red Hat Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |
| DM Mono | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file (400 only loaded) | ligatures off (`calt`, `liga`) |
| Lilex | no axis | monospaced | no feature; default 0 is slashed or dotted | italic file | ligatures off (`calt`, `liga`) |

`font-kerning: normal` is on everywhere. Missing glyphs fall back to a serif on purpose, so gaps are visible.

## 7. Where to look

Open these first (dark, 1x unless named otherwise):

1. `out/sheet-lockups.png`: shortlist beside the symbol at 72 px, medium left and semibold right.
2. `out/sheet-wordmark-recommended.png`: weights, symbol scale, gap, tracking and small sizes for Geologica.
3. `out/final/recommended-geologica+jetbrains-mono-dark@1x.png`: the whole specimen in the recommended pair (also `@2x` and `-light`).
4. `out/sheet-sans-ui-dark-1x.png`: task list and sidebar at 13 px, nine finalists. `out/sheet-sans-ui-dark-1x-zoom.png` is the same at 200 %, nearest neighbour.
5. `out/sheet-sans-chat-dark-1x.png`: the manual-style answer with table.
6. `out/sheet-sans-display.png`: "Make small models behave." in every sans.
7. `out/sheet-mono.png`: every mono with Geologica.
8. `out/sheet-lockups-all-round.png` and `out/sheet-lockups-all-neutral.png`: every sans beside the symbol.

Also built: `-all-a` (ten neutral) and `-all-b` (round and added) versions of the UI, chat, settings, legibility and glyph sheets; `sheet-sans-glyphs.png` (alphabets, Cyrillic, Greek, Vietnamese, italic); `sheet-sans-legibility.png` and `-zoom`; 2x sheets for UI, chat and mono; `sheet-lockups-light.png`; `sheet-wordmark-runner-up.png`.

The page itself: open `specimen.html?sans=geologica&tw=250,350,450&lw=200,300,350,400,500&mono=jetbrains-mono` in a browser. A picker at the top switches fonts, theme and view.

## 8. Method

- **Specimen.** `specimen.html` + `specimen.css`, plain HTML and CSS. Parameters: `sans`, `mono`, `theme`, `view`, and optional `tw` (text weights), `mw` (mono weights), `lw` (lockup weights), `sansvar`, `itv`. The symbol and the task-state glyphs are the owner's SVG files, inlined unchanged.
- **Captures.** `tools/capture.py` runs `/usr/bin/chromium --headless=new --hide-scrollbars --screenshot=… --window-size=1440,2600 --force-device-scale-factor=1|2`, plus a throw-away profile directory and `--virtual-time-budget=5000` so web fonts have loaded. Every sans was captured with JetBrains Mono, every mono with tuned Geologica, at 1x and 2x: about 500 captures in `out/shots/` (git-ignored, about 120 MB).
- **Sheets.** `tools/sheets.py` uses `magick montage` with a label per tile. Tiles are never resampled, except the sheets named `-zoom` (200 %, nearest neighbour).
- **Judging.** I looked at the tiles at 1:1 and at the zooms, in this order: lockups, 13 px UI, chat, legibility strip, display, glyph coverage. Roundness, dot shape, stems, widths, coverage and sizes are measured; "calm", "anonymous" and "loud" are my judgement.

## 9. Caveats

1. **Geologica's weights are not standard.** Tokens must say 250 / 350 / 450. Third-party components that hard-code 400 or 600 will look bold. A cleaner fix is to rebuild the font with a remapped weight axis so that 400 means today's 250; the OFL allows it and Geologica reserves no font name. Not done here.
2. **Only Linux was captured** (headless Chromium, FreeType, greyscale anti-aliasing). Windows usually renders thin strokes lighter and macOS heavier. Check Geologica 250 at 12 and 13 px on both before the weights are frozen.
3. **Fontsource subsets drop symbols.** `→ ← ✓ ≥ ≤ ≠ ≈` are missing from 18 of 19 sans packages and from 9 of 11 monos (the legibility strip shows the serif fallback). `voice.md` uses `→`. Production must build its own woff2 from the upstream releases with those ranges kept, or draw them as icons. I could not check the upstream glyph sets: downloads were limited to npm.
4. **CJK, Arabic, Hebrew, Thai and Indic scripts are not covered** by any candidate. They need a Noto fallback, which is tens of megabytes if bundled. That decision is outside this lab.
5. **Geologica has no slashed zero and no distinct `l`.** Identifiers, model names such as `Q4_K_M`, versions and paths belong in the mono. The specimen already does this.
6. **Geologica is less proven as an interface face than Inter.** The specimen shows English, short Russian, Greek and Vietnamese lines. Long-form text in those scripts was not tested.
7. **Neighbours were not surveyed.** "Distinct from neighbours" rests on general knowledge that Inter, Geist and system fonts are the defaults of this category, not on a check of each product.
8. **The italic needs one extra declaration.** Without `"CRSV" 1` Geologica's italic is a sloped roman.
9. **Mark detail at title-bar size.** At a 28 px symbol the two dots are about 4 px. That is a logo question; no typeface changes it.
10. **One side effect outside this folder.** pnpm added the font packages to its existing store at the root of this drive (it cannot hard-link from the home directory to this drive). Nothing else was written outside `brand/explorations/lab/type/`.
