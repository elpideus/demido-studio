#!/usr/bin/env bash
# Demido Studio colour lab: contact sheets from the captured panels (ImageMagick montage, labelled tiles, 1:1 pixels).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p sheets
FONT="Noto-Sans"; magick -list font | grep -q "Font: Noto-Sans$" || FONT="DejaVu-Sans"
label() { case "$1" in
  lime) echo "Lime";; green) echo "Signal green";; mint) echo "Mint (added)";; cyan) echo "Aqua / cyan";; blue) echo "Ion blue";;
  orchid) echo "Orchid (added)";; magenta) echo "Magenta line";; orange) echo "Flight orange";; yellow) echo "Tungsten yellow";; mono) echo "Monochrome";; esac; }
# sheet <out> <tile-geometry> <columns> <title> <label|file>...
sheet() {
  local out="$1" geo="$2" cols="$3" title="$4"; shift 4
  local args=()
  while [ $# -gt 0 ]; do args+=(-label "$1" "$2"); shift 2; done
  montage -background '#808080' -fill '#101010' -font "$FONT" -pointsize 22 -title "$title" \
    "${args[@]}" -tile "${cols}x" -geometry "$geo" "sheets/$out"
  echo "  sheets/$out $(magick identify -format '%wx%h' "sheets/$out")"
}
A1="lime green mint cyan blue orchid"; A2="magenta orange yellow mono"
for t in dark light; do
  # accents side by side: app shell (panel 1), mono primary
  for part in A B; do
    list=$A1; [ $part = B ] && list=$A2
    args=(); for a in $list; do args+=("$(label $a)" "renders/p1/neutral-$a-$t-mono.png"); done
    cols=3; [ $part = B ] && cols=2
    sheet "accents-$t-shell-$part.png" "760x900+10+8" $cols "Accents, $t theme, app shell (true neutral, mono primary) $part" "${args[@]}"
  done
  # accents side by side: controls with the accent as primary fill (panel 3, top 560 px)
  for part in A B; do
    list=$A1; [ $part = B ] && list=$A2
    args=(); for a in $list; do magick "renders/p3/neutral-$a-$t-accent.png" -crop 760x580+0+0 +repage "renders/p3/crop-$a-$t.png"; args+=("$(label $a)" "renders/p3/crop-$a-$t.png"); done
    cols=3; [ $part = B ] && cols=2
    sheet "accents-$t-controls-$part.png" "760x580+10+8" $cols "Accents, $t theme, controls with accent primary $part" "${args[@]}"
  done
  # accents: model card, meters, chart (panel 2 top 700) is wide; banners, icon, swatches (panel 4)
  for part in A B; do
    list=$A1; [ $part = B ] && list=$A2
    args=(); for a in $list; do args+=("$(label $a)" "renders/p4/neutral-$a-$t-mono.png"); done
    cols=3; [ $part = B ] && cols=2
    sheet "accents-$t-signals-$part.png" "680x900+10+8" $cols "Accents, $t theme, banners, app icon, swatches $part" "${args[@]}"
  done
done
# neutral temperatures side by side, both themes
for a in magenta cyan mono; do
  args=()
  for t in dark light; do for n in neutral warm cool; do args+=("$n, $t" "renders/p1/$n-$a-$t-mono.png"); done; done
  sheet "neutrals-$a-shell.png" "760x900+10+8" 3 "Neutral temperature: true neutral, warm (h 80), cool (h 255). Accent: $(label $a)" "${args[@]}"
done
args=(); for t in dark light; do for n in neutral warm cool; do args+=("$n, $t" "renders/p3/$n-magenta-$t-mono.png"); done; done
sheet "neutrals-magenta-controls.png" "760x900+10+8" 3 "Neutral temperature: controls and text on the four surfaces" "${args[@]}"
# mono primary versus accent primary
for a in magenta cyan lime; do
  for t in dark light; do
    sheet "primary-$a-$t.png" "760x900+10+8" 3 "Primary action: mono versus accent fill. $(label $a), $t theme" \
      "mono primary: shell" "renders/p1/neutral-$a-$t-mono.png" "accent primary: shell" "renders/p1/neutral-$a-$t-accent.png" "accent primary: controls" "renders/p3/neutral-$a-$t-accent.png"
  done
done
# app icon strip for every accent (panel 4 crop)
args=(); for a in $A1 $A2; do magick "renders/p4/neutral-$a-dark-mono.png" -crop 540x200+0+250 +repage "renders/p4/icon-$a.png"; args+=("$(label $a)" "renders/p4/icon-$a.png"); done
sheet "app-icons.png" "540x200+8+6" 4 "App icon tile per accent (logo stays black or white)" "${args[@]}"

# evidence pages generated from out/tokens.json (CVD simulation, whole palette)
node pages.mjs | while read -r name w h; do
  /usr/bin/chromium --headless=new --hide-scrollbars --screenshot="$PWD/renders/$name.png" --window-size="$w,$((h + 400))" --force-device-scale-factor=1 \
    --user-data-dir="$(mktemp -d)" "file://$PWD/out/$name.html" >/dev/null 2>&1
  magick "renders/$name.png" -bordercolor '#808080' -border 1 -trim +repage "sheets/${name#sheet-}.png"
  echo "  sheets/${name#sheet-}.png $(magick identify -format '%wx%h' "sheets/${name#sheet-}.png")"
done
# the recommended combination, whole board, dark beside light (81% so both fit in 2340 px)
REC_N="${REC_N:-neutral}"; REC_A="${REC_A:-magenta}"
montage -background '#808080' -fill '#101010' -font "$FONT" -pointsize 22 -title "Recommended: true neutral, $(label $REC_A), mono primary (boards at 81%)" \
  -label "dark" "renders/full/$REC_N-$REC_A-dark-mono.png" -label "light" "renders/full/$REC_N-$REC_A-light-mono.png" -tile 2x -geometry "1160x1450+5+8" "sheets/recommended-boards.png"
echo "  sheets/recommended-boards.png $(magick identify -format '%wx%h' sheets/recommended-boards.png)"
