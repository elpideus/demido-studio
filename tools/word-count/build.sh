#!/usr/bin/env bash
# Builds word_count's component from its source and packs it, signed (DECISIONS C12):
#   tools/word-count/build.sh [<key file>]
# The key defaults to ~/.local/share/demido-dev-keys/tools.key, a development key of this computer's, made when it
# is not there; never in the repository. The package and its signature go to tools/word-count/dist/.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
KEY="${1:-$HOME/.local/share/demido-dev-keys/tools.key}"
( cd "$HERE" && cargo build --release --target wasm32-wasip2 )
cp "$HERE/target/wasm32-wasip2/release/word_count.wasm" "$HERE/package/tool.wasm"
cargo run --quiet --manifest-path "$REPO/core/Cargo.toml" -p demido-core --example pack_tool -- "$HERE/package" "$KEY" "$HERE/dist"
