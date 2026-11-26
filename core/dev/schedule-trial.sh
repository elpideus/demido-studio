#!/usr/bin/env bash
# Whether real models ask for a schedule when asked for one (DECISIONS J13). A scratch home with the real
# runtime and models (real-home.sh), at Autonomous so nothing waits for a yes (it has read nothing from the
# internet, so nothing is asked); then the schedules it made are listed. Uses the graphics card a minute or so.
# Usage: core/dev/schedule-trial.sh [model ...]
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/real-home.sh"
ASKS=(
  "Can you schedule a dad joke of the day every day at 8 in the morning, so I start the day laughing?"
  "Every weekday at 9:30, remind me to drink a glass of water."
)
for m in "${MODELS[@]}"; do
  for a in "${ASKS[@]}"; do
    echo "=== $m: $a"
    "$D" ask --model "$m" --level autonomous "$a" 2>&1 | quiet | tail -8
  done
done
echo "=== schedules made"
"$D" schedules 2>&1 | cut -c1-200
