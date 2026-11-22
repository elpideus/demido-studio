#!/usr/bin/env bash
# A real model asked with its maker's settings (engine/sampling.rs): that the runtime takes them, thinking
# and not. A scratch data folder sharing the real runtime and models (real-home.sh), on port 7399. Uses the
# graphics card for a minute.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/real-home.sh" "$@"
for m in "${MODELS[@]}"; do
  echo "== $m"
  "$D" models settings "$m"
  echo "-- not thinking"
  "$D" ask --model "$m" "Name three colours of a rainbow, in one line." 2>&1 | quiet | tail -4
  echo "-- thinking"
  "$D" ask --model "$m" --think "Is 91 a prime number? Answer in one line." 2>&1 | quiet | tail -4
done
echo "== What the runtime said against a request (nothing is good):"
grep -i -E "invalid|unknown (field|param)|error" "$SCRATCH/engine/router.log" | grep -v -i "no error" | head -10
