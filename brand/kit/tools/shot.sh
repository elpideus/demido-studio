#!/usr/bin/env bash
# Build one kit section on its own and screenshot it with headless Chromium.
# Usage: brand/kit/tools/shot.sh <section-name> [dark|light] [width] [height]
#   e.g. brand/kit/tools/shot.sh 60-state dark 1440 2200
# Output: brand/kit/.shots/<section-name>-<theme>-<width>.png
set -euo pipefail
name="${1:?section name, e.g. 60-state}"; theme="${2:-dark}"; width="${3:-1440}"; height="${4:-2400}"
kit="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
node "$kit/build.mjs" --only "$name" >/dev/null
mkdir -p "$kit/.shots"
out="$kit/.shots/$name-$theme-$width.png"
url="file://$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$kit/preview-$name.html")?theme=$theme"
chromium --headless=new --hide-scrollbars --disable-gpu --virtual-time-budget=4000 \
  --window-size="$width,$height" --force-device-scale-factor=1 --screenshot="$out" "$url" >/dev/null 2>&1
echo "$out"
