#!/usr/bin/env bash
# What core/dev/voice-trial.py measures with (research note 28): a Python of its own and the speech models,
# kept in ~/.cache/demido-voice-trial so that nothing is downloaded twice. Everything comes from its makers:
# the packages from PyPI (no newer than DEMIDO_TRIAL_NOT_AFTER, so that a release of today is not tried),
# sherpa-onnx's model packages from its GitHub releases, Smart Turn from pipecat-ai's Hugging Face page.
# Nothing here is part of Demido; the trial decides what Demido is built from.
#   Licences (read 22 November 2026): sherpa-onnx Apache-2.0; onnxruntime MIT; transformers Apache-2.0
#   (only its Whisper feature extractor is used, the one Smart Turn's own code uses); Silero VAD MIT;
#   NVIDIA Parakeet TDT 0.6B v3 CC-BY-4.0; Whisper MIT; Kokoro-82M Apache-2.0; Smart Turn v3 BSD-2-Clause;
#   Pocket TTS CC-BY-4.0 with Kyutai's conditions of use (no voice cloning without consent); Smart Turn's test set
#   CC-BY-4.0 (one shard of nine, 481 MB: Smart Turn is checked on it, against its makers' own report).
set -euo pipefail
CACHE="${DEMIDO_VOICE_CACHE:-$HOME/.cache/demido-voice-trial}"
NOT_AFTER="${DEMIDO_TRIAL_NOT_AFTER:-2026-11-08}"
MODELS="$CACHE/models"
mkdir -p "$MODELS"
if [ ! -x "$CACHE/venv/bin/python" ]; then
  uv venv --python 3.14 "$CACHE/venv"
fi
VIRTUAL_ENV="$CACHE/venv" uv pip install --quiet --exclude-newer "$NOT_AFTER" "sherpa-onnx==2.0.1" onnxruntime numpy transformers pyarrow soundfile

file() { # url, name: one file, kept when it is there already
  [ -e "$MODELS/$2" ] && return 0
  echo "Downloading $2"
  curl -fL --retry 2 -o "$MODELS/$2.part" "$1"
  mv "$MODELS/$2.part" "$MODELS/$2"
}
package() { # url of a .tar.bz2, its folder's name: unpacked once, the archive not kept
  [ -d "$MODELS/$2" ] && return 0
  echo "Downloading $2"
  curl -fL --retry 2 -o "$MODELS/$2.tar.bz2.part" "$1"
  tar -xjf "$MODELS/$2.tar.bz2.part" -C "$MODELS"
  rm -f -- "$MODELS/$2.tar.bz2.part"
}
REL=https://github.com/k2-fsa/sherpa-onnx/releases/download
file "$REL/asr-models/silero_vad.onnx" silero_vad.onnx
package "$REL/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2" sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8
package "$REL/asr-models/sherpa-onnx-whisper-turbo.tar.bz2" sherpa-onnx-whisper-turbo
package "$REL/tts-models/kokoro-int8-multi-lang-v1_0.tar.bz2" kokoro-int8-multi-lang-v1_0
package "$REL/tts-models/sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2" sherpa-onnx-pocket-tts-int8-2026-01-26
file "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/main/smart-turn-v3.2-cpu.onnx" smart-turn-v3.2-cpu.onnx
mkdir -p "$CACHE/smart-turn-test"
if [ ! -e "$CACHE/smart-turn-test/train-00000-of-00009.parquet" ]; then
  echo "Downloading one shard of Smart Turn's test set"
  curl -fL --retry 2 -o "$CACHE/smart-turn-test/part.parquet" "https://huggingface.co/datasets/pipecat-ai/smart-turn-data-v3.1-test/resolve/main/data/train-00000-of-00009.parquet"
  mv "$CACHE/smart-turn-test/part.parquet" "$CACHE/smart-turn-test/train-00000-of-00009.parquet"
fi
du -sh "$MODELS" "$CACHE/venv"
