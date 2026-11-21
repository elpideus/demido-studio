#!/usr/bin/env bash
# Demido Studio colour lab: capture the specimen matrix with headless Chromium and crop the four panels.
# Usage: ./capture.sh            (full matrix)   |   ./capture.sh neutral magenta dark mono   (one combination)
set -euo pipefail
cd "$(dirname "$0")"
HERE="$PWD"
mkdir -p renders/full renders/p1 renders/p2 renders/p3 renders/p4
shot() { # neutral accent theme primary
  local name="$1-$2-$3-$4" f="$HERE/renders/full/$1-$2-$3-$4.png"
  /usr/bin/chromium --headless=new --hide-scrollbars --screenshot="$f" --window-size=1440,1800 --force-device-scale-factor=1 \
    --user-data-dir="$(mktemp -d)" "file://$HERE/specimen.html?neutral=$1&accent=$2&theme=$3&primary=$4" >/dev/null 2>&1
  magick "$f" -crop 760x900+0+0 +repage "renders/p1/$name.png"
  magick "$f" -crop 680x900+760+0 +repage "renders/p2/$name.png"
  magick "$f" -crop 760x900+0+900 +repage "renders/p3/$name.png"
  magick "$f" -crop 680x900+760+900 +repage "renders/p4/$name.png"
  echo "  $name"
}
export -f shot; export HERE
if [ $# -eq 4 ]; then shot "$@"; exit 0; fi
ACCENTS="lime green mint cyan blue orchid magenta orange yellow mono"
{
  for t in dark light; do for a in $ACCENTS; do for p in mono accent; do echo "neutral $a $t $p"; done; done; done
  for n in warm cool; do for t in dark light; do for a in magenta cyan mono; do echo "$n $a $t mono"; done; done; done
} | xargs -P 6 -L 1 bash -c 'shot "$@"' _
echo "captured $(ls renders/full | wc -l) boards"
