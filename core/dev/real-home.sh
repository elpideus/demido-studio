# Sourced by the trials that go through `demido ask`: a scratch data folder that shares the real runtime and
# the real models, and a server on a port of its own. Nothing here touches the sessions or settings of the
# server a person is using.
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
D="$REPO/core/target/debug/demido"
REAL="${DEMIDO_REAL_HOME:-$HOME/.local/share/demido}"
[ -x "$D" ] || { echo "Build first: cargo build --manifest-path core/Cargo.toml" >&2; exit 1; }
[ -d "$REAL/models" ] || { echo "No models in $REAL/models" >&2; exit 1; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/demido-trial-XXXXXX")"
ln -s "$REAL/runtimes" "$SCRATCH/runtimes"
ln -s "$REAL/models" "$SCRATCH/models"
export DEMIDO_HOME="$SCRATCH"
"$D" serve --listen 127.0.0.1:7399 > "$SCRATCH/server.log" 2>&1 &
SERVER=$!
sleep 2
MODELS=("$@")
[ ${#MODELS[@]} -eq 0 ] && MODELS=(Qwen3-0.6B-Q8_0 Qwen3-1.7B-Q8_0 granite-3.3-2b-instruct-Q4_K_M Qwen3-4B-Q4_K_M)
# What `demido ask` prints that is not the answer or a step.
quiet() { grep -v "tokens at\|Go on with\|Loading\|^$" | cut -c1-400; }
finish() { kill "$SERVER" ${SITE:-} 2>/dev/null; wait "$SERVER" 2>/dev/null; rm -rf "$SCRATCH"; }
trap finish EXIT
