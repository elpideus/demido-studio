# Colour lab report

> **Superseded on the accent.** The owner set the brand colour to green after this run. The neutrals and
> the caution, danger and info signals from this report stand; the accent is now Demido green
> (`oklch(0.86 0.15 160)`), chosen and measured in `docs/DECISIONS.md` A4. The magenta recommendation below is history.

Assembled from the lab run of 2026-11-20. Measured tables: `out/tables.md`. Recommended kit values: `out/recommended.md`.
Reproduce: `node lib/selftest.mjs && node build.mjs && ./capture.sh && ./sheets.sh`.

## Recommendation

True neutral ramp + "Magenta line" accent (softened) + monochrome primary action + the searched signal set. Dark: background #141414, accent #ff85df oklch(0.780 0.181 338). Light: background #f7f7f7, accent #c500a5 oklch(0.560 0.246 338). Same hue (338) in both themes. The accent is never a button fill: it marks only "a model is live here" (lamp dot, running arc, streaming caret, the word Loaded) and focus; the primary button wears the logo's own white (dark) or black (light). Reasons: (1) magenta is the only free hue: 48 degrees from danger, 92 from info, and no LLM neighbour uses it; (2) it is the only chromatic candidate that keeps the base signal set untouched on dark and needs one signal moved on light; (3) it keeps its hue in the light theme where lime, yellow and cyan turn olive, mustard and teal; (4) it has the right cockpit meaning (the active route on a flight display); (5) by eye it sits comfortably beside the soft round white mark when small. Honest cost: as a large fill it is bubblegum pink and at full chroma it is loud and telecom-like, which is why primary must stay mono and the app icon must stay near-black with the white mark. One measured weakness: on dark, live pink vs info blue is 7.7 (deuteranopia) / 7.1 (protanopia), under the target of 8, above the floor of 6: WARN, covered by distinct glyphs.

## Runner-up

Aqua / cyan: dark #47e9eb oklch(0.854 0.130 196), light #00838e oklch(0.557 0.095 205), still with mono primary and true neutral. It is the only other chromatic accent that clears the signal set in both themes (worst-pair score 1.01 dark, 1.06 light), but only by moving all four signals on dark and two on light. It is cold and sci-fi beside the round mark, and on light it becomes teal (dark-to-light drift 30 vs 23 for magenta), about 10-11 from AnythingLLM sky and ChatGPT legacy green as the lab recalls them. Take it only if pink is rejected on taste.

## Ranking

- 1. Magenta line #ff85df / #c500a5: recommended. Signals: WARN on dark (info 7.7/7.1), pass on light. Unowned hue, same hue both themes, right story; bubblegum if used as a fill. Licence: n/a (colour values).
- 2. Aqua / cyan #47e9eb / #00838e: runner-up. Passes only with all four signals moved on dark; teal on light; cold, HUD commonplace. Licence: n/a.
- 3. Orchid (added) #e3a0f8 / #a535bd: softest partner for the logo by eye, but FAILS vs info on dark (normal 14, protan 6.3) and is plain purple on light, in the LM Studio / Gemini / AI-purple zone. Licence: n/a.
- 4. Monochrome #fafafa / #0e0e0e: passes everything, but the live state then has no colour signal and Ollama, Open WebUI, Cursor and ChatGPT already own black-and-white. Keep it as the primary action, not as the accent. Licence: n/a.
- 5. Lime #c3f62a / #5d8200: strongest lamp on dark (12.5:1), passes dark with caution moved; trendy acid-lime look, sharp against a soft mark; olive on light and FAILS there vs success and caution. Licence: n/a.
- 6. Mint (added) #6eecc1 / #008668: friendliest green, good with the logo; FAILS vs success on dark (12 apart) and its light value is 1 degree of hue from ChatGPT's legacy green. Licence: n/a.
- 7. Ion blue #7cbdff / #0070e1: calm but generic (the default focus ring); it IS the info colour: FAILS on dark (10/8/10), passes light only with info pushed to navy; 4-5 from Jan and AnythingLLM blues as recalled. Licence: n/a.
- 8. Signal green #3ee275 / #00893d: cannot be told from success (10 apart at best); FAILS both themes; app tile looks like a messaging app. Licence: n/a.
- 9. Tungsten yellow #ffd160 / #9a6f00: warmest and kindest beside the mark, but amber means caution: Loaded and Needs-you are the same colour (2 apart before shift); mustard on light; FAILS both themes. Licence: n/a.
- 10. Flight orange #ff9165 / #cd4c00: true flight orange (#ff691f) cannot carry text; the fixed value is peach, 7 from OpenCode's and 9 from Claude's as recalled; sits between caution and danger; FAILS both themes. Licence: n/a.

## Key facts

- Scope: 3 neutral ramps x 2 themes x 12 roles, 10 accents (8 asked + mint and orchid), 52 specimen boards captured at 1440x1800, 26 contact sheets at 1:1 pixels. Colour maths self-test: 20 of 20 reference checks pass (APCA, WCAG 2, OKLCH).
- Contrast after fixing: 0 failing rows across all ramps, accents and themes. The build moved 26 values and logs each (T10). True neutral dark: primary text 15.0:1 / Lc 92, secondary 9.4:1 / Lc 64, tertiary 6.6:1 / Lc 47 on #141414; strong border 3.2-3.8:1 on every surface.
- Neutral temperature: all three pass identically, so it is a character call. Warm reads as Claude (cream paper on light, brown-black on dark); cool reads as generic developer-tool slate; true neutral is the owner's reference and leaves the accent as the only hue on screen.
- Mid-tone trap: vivid magenta at L 0.70 gives dark text 6.4:1 but Lc 48 and white text Lc 61 but 3.0:1. An accent that is mark, text and fill on dark must sit near L 0.78. This softened magenta, and turned true flight orange into peach.
- Light theme kills bright hues: the 3:1 mark rule caps lightness near 0.60. Lime becomes olive #5d8200, tungsten yellow mustard #9a6f00, cyan teal #00838e (moved 10-19 units from intent). Blue, orchid and magenta keep hue and gain chroma.
- Signal collisions before any shift (normal/deutan/protan): yellow vs caution 2/2/2, blue vs info 2/2/2, green vs success 5/5/5, mint vs success 7/6/6, orange vs danger 7/5/7. Magenta's only flagged pair is info 22/8/7 (WARN).
- Signal set was found by grid search for the most separable set under normal vision, deuteranopia and protanopia (Machado 2009, severity 1.0). Worst pair among the four signals: 8.0 dark, 8.9 light. A conventional light set (all near L 0.53) collapses: caution vs danger 1.3 under deuteranopia. Hence the dark crimson danger text #990012 on light.
- Never colour alone: five silhouettes (live = dot in ring, success = check in disc, caution = triangle, danger = octagon, info = i in ring). Signal colours are for glyph plus short bold label only; dark danger reaches Lc 50 on background.
- Mono vs accent primary: with mono primary the accent appears only as small live marks plus the focus ring; with accent primary Send fuses with the focus ring into one pink object, toggles turn pink, and the pink fill sits 15.6 from danger red (closest pair in the kit).
- Logo fit by eye: orchid, mint and tungsten yellow are the gentlest partners; magenta one notch louder; lime and cyan argue with the mark. Of the gentle ones only magenta survives the measurements.
- Specimen uses brand/logo/demido-symbol.svg and the brand/icons/state glyphs unchanged (inlined as a sprite so currentColor works). Specimen URL: specimen.html?neutral=neutral|warm|cool&accent=lime|green|mint|cyan|blue|orchid|magenta|orange|yellow|mono&theme=dark|light&primary=mono|accent.
- Reproduce from `brand/explorations/lab/color`: node lib/selftest.mjs && node build.mjs && ./capture.sh && ./sheets.sh. Spec is palette.mjs; machine-readable results are out/tokens.json and out/tokens.css; kit tables are out/recommended.md.

## Values

```
NEUTRAL (true neutral, chroma 0; hex then OKLCH L)
Dark: sunken #0f0f0f 0.168 | bg #141414 0.191 | raised #1b1b1b 0.222 | overlay #222222 0.252 | fill #2b2b2b 0.289 | hairline #343434 0.325 | disabled #575757 0.457 | border #6f6f6f 0.542 | text3 #9a9a9a 0.686 | text2 #b9b9b9 0.786 | text1 #e8e8e8 0.931 | solid #fafafa 0.985
Light: overlay #ffffff 1.000 | raised #fcfcfc 0.991 | bg #f7f7f7 0.976 | sunken #f0f0f0 0.955 | fill #e7e7e7 0.928 | hairline #dbdbdb 0.891 | disabled #a4a4a4 0.719 | border #868686 0.620 | text3 #696969 0.521 | text2 #4a4a4a 0.409 | text1 #1b1b1b 0.222 | solid #0e0e0e 0.164

ACCENT (Magenta line, hue 338)
Dark: base #ff85df oklch(0.780 0.181 338) | hover #ff9fe4 oklch(0.820 0.142 338) | pressed #eb70cc oklch(0.719 0.185 338) | tint #351d2f oklch(0.270 0.047 337) | tint border #6c385e oklch(0.420 0.090 338) | as text #ff91e1 oklch(0.798 0.163 338) | text on fill #0e0e0e (8.89:1, Lc 61)
Light: base #c500a5 oklch(0.560 0.246 338) | hover #b00093 oklch(0.514 0.226 338) | pressed #9c0082 oklch(0.471 0.206 338) | tint #fee7f6 oklch(0.951 0.032 339) | tint border #ecb8dc oklch(0.840 0.076 338) | as text #c500a5 | text on fill #ffffff (5.36:1, Lc 80)

PRIMARY ACTION (mono) and FOCUS
Dark: fill #fafafa, hover #e6e6e6, pressed #d1d1d1, text #0e0e0e (18.5:1). Light: fill #0e0e0e, hover #2a2a2a, pressed #3d3d3d, text #ffffff (19.3:1). Focus ring: the accent, 2 px with a 2 px gap.

SIGNALS (text and glyph / tint / solid fill with white text)
Dark: success #35de9a oklch(0.801 0.166 161) / #0c2e1f / #00875a | caution #faca4b oklch(0.859 0.150 88) / #302506 / #947200 | danger #ff7269 oklch(0.720 0.174 26) / #3b1d1a / #d73e3c | info #69b8fc oklch(0.760 0.126 246) / #10283c / #287bba
Light: success #007c5e oklch(0.521 0.105 168) / #dbf6eb / #008768 | caution #935f00 oklch(0.529 0.113 72) / #fcecd8 / #9f6700 | danger #990012 oklch(0.430 0.175 26) / #ffe9e6 / #d42229 | info #4c65ca oklch(0.541 0.159 270) / #e8eeff / #516bd1 (moved for magenta; base-set info is #0065b0 oklch(0.500 0.142 250))

RUNNER-UP ACCENT (Aqua / cyan): dark #47e9eb oklch(0.854 0.130 196), light #00838e oklch(0.557 0.095 205)
APP ICON: near-black tile #0f0f0f with the white mark; the logo stays black or white on any tile.
```

## Comparison sheets

- `sheets/recommended-boards.png`
- `sheets/accents-dark-shell-A.png`
- `sheets/accents-dark-shell-B.png`
- `sheets/accents-light-shell-B.png`
- `sheets/cvd.png`
- `sheets/neutrals-magenta-shell.png`
- `sheets/primary-magenta-dark.png`
- `sheets/accents-dark-controls-B.png`

## Caveats

- Which combinations fail: signal green and mint vs success; ion blue and orchid vs info on dark; flight orange vs danger and caution; tungsten yellow vs caution (both themes); lime on light (olive, vs success and caution). Warm and cool neutrals have no measured failure and are rejected for character only.
- The recommendation carries one WARN, not a clean pass: on dark, magenta vs info is 7.7 / 7.1 under deuteranopia / protanopia (target 8, floor 6). No magenta hue between 330 and 346 clears 8 without moving three signals and drifting to pink.
- Pink is a taste risk. Softened magenta reads as a lamp when small and as bubblegum when large; on light #c500a5 sits about 10 from T-Mobile magenta (23 degrees of hue apart). Mono primary and a near-black app icon are conditions of the recommendation, not options.
- Neighbour brand colours are the lab's recollection, not fetched (downloads were forbidden). Distances to them are indicative; Jan's and Msty's values are the least certain.
- sRGB only; no P3 exploration. Colour blindness is one simulation model at full severity. APCA thresholds beyond body Lc 75 (secondary Lc 60, tertiary Lc 45, accent and on-fill Lc 60, signal text Lc 45) are the lab's own and are what pushed accents lighter on dark.
- The signal set comes from a grid search inside hue and lightness windows the lab chose; other windows give other sets. The light danger text is a dark crimson by design; the destructive button keeps a brighter red.
- Chart series 2-4 use the dataviz reference slots as placeholders; on light the yellow series holds only 2.1:1 on the card and relies on end labels and legend. A brand data palette is still to be designed. The live series sits above the categorical lightness band on dark on purpose.
- Three signal glyphs (caution triangle, danger octagon, info ring) and the lamp dot were drawn by the lab as placeholders; they are not brand assets. Text is system-ui (Noto Sans on the capture machine). Captures are static: hover, pressed and focus are drawn states and the running arc does not spin.
- A previous run had left lib/color.mjs, lib/selftest.mjs and a palette spec; the maths library was verified and reused, the spec was rewritten, and a leftover probe script was removed. No old Demido code or repo was opened, nothing was downloaded, and all work stayed inside brand/explorations/lab/color/.
