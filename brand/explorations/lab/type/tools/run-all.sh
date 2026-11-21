#!/usr/bin/env bash
# Rebuild every capture and contact sheet of the type lab. About 1 minute on 8 cores.
#   pnpm install && tools/run-all.sh
# Needs: python3 + fontTools, /usr/bin/chromium, ImageMagick 7 (magick).
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p out out/final
py() { python3 "$@"; }

# ---------------------------------------------------------------- 0. facts and registry
py tools/facts.py > out/facts-table.txt
py tools/tables.py > out/facts.md      # Markdown tables pasted into REPORT.md

FIXED_MONO=jetbrains-mono
SANS_VIEWS=all,lockups,lockup-tile,chat,ui,settings,legibility,display,glyphs
MONO_VIEWS=all,mono,chat,settings,legibility,ui
# Geologica runs dark at its nominal weights. "Tuned" means: text weights 250/350/450 in place of 400/500/600,
# and lockup weights 200/300/350/400/500 in place of 300/400/500/600/700.
GEO_ARGS=(--tw 250,350,450 --lw 200,300,350,400,500)
GEO="geologica~tw250-350-450lw200-300-350-400-500"

if [ -z "${SHEETS_ONLY:-}" ]; then   # SHEETS_ONLY=1 tools/run-all.sh rebuilds the sheets from existing captures

# ---------------------------------------------------------------- 1. every sans with one fixed mono, 1x and 2x
py tools/capture.py --sans all --mono $FIXED_MONO --views $SANS_VIEWS --scales 1,2 -j 8
py tools/capture.py --sans geologica "${GEO_ARGS[@]}" --mono $FIXED_MONO --views $SANS_VIEWS --scales 1,2 -j 8
py tools/capture.py --sans google-sans-flex --sansvar ROND:100 --mono $FIXED_MONO --views lockups,lockup-tile,ui,chat,display --scales 1,2 -j 8

# ---------------------------------------------------------------- 2. every mono with the top sans, 1x and 2x
py tools/capture.py --sans geologica "${GEO_ARGS[@]}" --mono all --views $MONO_VIEWS --scales 1,2 -j 8
py tools/capture.py --sans geologica "${GEO_ARGS[@]}" --mono red-hat-mono --mw 450,550 --views mono,settings,chat --scales 1,2 -j 8

# ---------------------------------------------------------------- 3. wordmark studies and light theme
py tools/capture.py --sans geologica --tw 250,350,450 --views wordmark --ww 300,350,400,450 --wc 400 --ws 300 --wk 2 --scales 1,2 --theme dark,light
py tools/capture.py --sans inter,figtree,onest,geist --views wordmark --ww 400,450,500,600 --wc 500 --ws 400 --wk 2 --scales 1
py tools/capture.py --sans manrope --views wordmark --ww 500,550,600,700 --wc 600 --ws 450 --wk 2 --scales 1
py tools/capture.py --sans geologica "${GEO_ARGS[@]}" --mono $FIXED_MONO --views all,lockups --scales 1,2 --theme light
py tools/capture.py --sans inter --mono $FIXED_MONO --views all,lockups --scales 1,2 --theme light

fi

S=out/shots
cp "$S/all/${GEO}__${FIXED_MONO}__dark@1x.png"  out/final/recommended-geologica+jetbrains-mono-dark@1x.png
cp "$S/all/${GEO}__${FIXED_MONO}__dark@2x.png"  out/final/recommended-geologica+jetbrains-mono-dark@2x.png
cp "$S/all/${GEO}__${FIXED_MONO}__light@1x.png" out/final/recommended-geologica+jetbrains-mono-light@1x.png
cp "$S/all/inter__${FIXED_MONO}__dark@1x.png"   out/final/runner-up-inter+jetbrains-mono-dark@1x.png
cp "$S/all/inter__${FIXED_MONO}__dark@2x.png"   out/final/runner-up-inter+jetbrains-mono-dark@2x.png
cp "$S/all/inter__${FIXED_MONO}__light@1x.png"  out/final/runner-up-inter+jetbrains-mono-light@1x.png
cp "$S/wordmark/geologica~tw250-350-450wc400__${FIXED_MONO}__dark@1x.png"  out/sheet-wordmark-recommended.png
cp "$S/wordmark/geologica~tw250-350-450wc400__${FIXED_MONO}__dark@2x.png"  out/sheet-wordmark-recommended@2x.png
cp "$S/wordmark/geologica~tw250-350-450wc400__${FIXED_MONO}__light@1x.png" out/sheet-wordmark-recommended-light.png
cp "$S/wordmark/inter~wc500__${FIXED_MONO}__dark@1x.png" out/sheet-wordmark-runner-up.png

# ---------------------------------------------------------------- 4. contact sheets (never wider than 2400 px)
SHORT="$GEO,inter,figtree,onest,geist,manrope,hanken-grotesk,instrument-sans,dm-sans"
SHORT6="$GEO,inter,figtree,onest,geist,manrope"
ALL_A="neutral"                                              # the ten neutral / technical candidates
ALL_B="round,geologica,$GEO,google-sans-flex"                # the seven round candidates and the two added sans
sheet() { py tools/sheets.py "$@"; }

# lockups: first the shortlist at 72 px (left: medium, right: semibold; for tuned Geologica 350 and 400), then every candidate
sheet sheet-lockups.png                 --view lockups     --fonts "$GEO,figtree,onest,inter,geist,manrope,dm-sans,hanken-grotesk" --cols 1 --crop 1440x136+0+8
sheet sheet-lockups-all-neutral.png     --view lockup-tile --fonts "$ALL_A" --cols 3
sheet sheet-lockups-all-round.png       --view lockup-tile --fonts "$ALL_B,google-sans-flex~ROND100" --cols 3
sheet sheet-lockups-light.png           --view lockups     --fonts "$GEO,inter" --cols 1 --theme light --crop 1440x136+0+8

# small UI text at 1x on dark: shortlist, every candidate, and a 200 % nearest-neighbour zoom of the task list
sheet sheet-sans-ui-dark-1x.png         --view ui --fonts "$SHORT" --cols 3
sheet sheet-sans-ui-dark-1x-all-a.png   --view ui --fonts "$ALL_A" --cols 3
sheet sheet-sans-ui-dark-1x-all-b.png   --view ui --fonts "$ALL_B,google-sans-flex~ROND100" --cols 3
sheet sheet-sans-ui-dark-1x-zoom.png    --view ui --fonts "$SHORT6" --cols 3 --crop 366x290+22+22 --zoom 200
sheet sheet-sans-ui-dark-2x.png         --view ui --fonts "$GEO,inter,figtree,onest" --cols 1 --scale 2

sheet sheet-sans-chat-dark-1x.png       --view chat --fonts "$SHORT6" --cols 3
sheet sheet-sans-chat-dark-1x-all-a.png --view chat --fonts "$ALL_A" --cols 3
sheet sheet-sans-chat-dark-1x-all-b.png --view chat --fonts "$ALL_B" --cols 3
sheet sheet-sans-chat-dark-2x.png       --view chat --fonts "$GEO,inter" --cols 1 --scale 2

sheet sheet-sans-settings-dark-1x.png       --view settings --fonts "$SHORT6" --cols 3
sheet sheet-sans-settings-dark-1x-all-a.png --view settings --fonts "$ALL_A" --cols 3
sheet sheet-sans-settings-dark-1x-all-b.png --view settings --fonts "$ALL_B" --cols 3

sheet sheet-sans-display.png            --view display --fonts "$ALL_A,$ALL_B,google-sans-flex~ROND100" --cols 3
sheet sheet-sans-legibility.png         --view legibility --fonts "$SHORT" --cols 3
sheet sheet-sans-legibility-all-a.png   --view legibility --fonts "$ALL_A" --cols 3
sheet sheet-sans-legibility-all-b.png   --view legibility --fonts "$ALL_B" --cols 3
sheet sheet-sans-legibility-zoom.png    --view legibility --fonts "$SHORT6" --cols 1 --crop 700x100+20+26 --zoom 200
sheet sheet-sans-glyphs.png             --view glyphs --fonts "$SHORT" --cols 3
sheet sheet-sans-glyphs-all-a.png       --view glyphs --fonts "$ALL_A" --cols 3
sheet sheet-sans-glyphs-all-b.png       --view glyphs --fonts "$ALL_B" --cols 3

# monos, each with the top sans
sheet sheet-mono.png              --view mono     --kind mono --fixed "$GEO" --fonts "mono,red-hat-mono~mw450-550" --cols 3
sheet sheet-mono-settings-1x.png  --view settings --kind mono --fixed "$GEO" --fonts "mono,red-hat-mono~mw450-550" --cols 3
sheet sheet-mono-chat-1x.png      --view chat     --kind mono --fixed "$GEO" --fonts "jetbrains-mono,geist-mono,commit-mono,lilex,red-hat-mono~mw450-550,ibm-plex-mono" --cols 3
sheet sheet-mono-2x.png           --view mono     --kind mono --fixed "$GEO" --fonts "jetbrains-mono,geist-mono,commit-mono,red-hat-mono~mw450-550" --cols 1 --scale 2
sheet sheet-mono-zoom.png         --view settings --kind mono --fixed "$GEO" --fonts "jetbrains-mono,geist-mono,commit-mono,lilex,red-hat-mono~mw450-550,ibm-plex-mono" --cols 3 --crop 360x250+376+16 --zoom 200

ls -1 out/*.png | wc -l
