#!/usr/bin/env bash
# The two tools that read from the internet, through `demido ask`, on real models (research note 15).
#
#     core/dev/web-trial.sh [model ...]
#
# Eight requests, each in a new session. It asks real sites and search services (Exa first, as the ladder
# goes; example.com; Wikipedia): about forty requests in all for four models, under Demido's own name. Nobody is there to say yes, so an address a
# model writes itself is answered with no. Takes a few minutes and uses the graphics card.
set -u
source "$(dirname "$0")/real-home.sh"
REQUESTS=(
  "Say hello in French."
  "What is 17 times 23?"
  "Search the web: what is the tokio library for Rust?"
  "Look up on the internet how wide a grey heron's wings are."
  "Read https://example.com and tell me what that page is for."
  "Read https://en.wikipedia.org/wiki/Grey_heron and tell me how long grey herons live."
  "What is the weather in Rome right now?"
  "Which version of llama.cpp is the newest?"
)
for model in "${MODELS[@]}"; do
  echo "################ $model"
  for request in "${REQUESTS[@]}"; do
    echo "---- $request"
    # shellcheck disable=SC2086
    timeout 180 "$D" ask --model "$model" $request < /dev/null 2>&1 | quiet | head -24
  done
done
