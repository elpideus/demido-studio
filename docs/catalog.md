# The catalog

`catalog/runtimes.json` and `catalog/models.json` say exactly what the installer downloads. They
are compiled into the installer (`crates/catalog`), so a release always installs what it was
tested with. Every file is pinned by size and SHA-256 and verified before it is used.

## runtimes.json

| Key | What |
|---|---|
| `llamaCpp.release` | The llama.cpp build tag, e.g. `b11146` |
| `llamaCpp.baseUrl` | Where the release's assets are downloaded from |
| `llamaCpp.variants[]` | One per runtime and platform: `id`, `backend` (`cuda`, `rocm`, `metal`, `vulkan`, `cpu`), `os`, `arch`, `label`, `requires` (minimum NVIDIA driver, minimum compute capability) and `assets` (`name`, `size`, `sha256`) |
| `node`, `uv` | `version` and one asset per platform |
| `python` | The version uv installs and the packages installed into it |

The CUDA variants differ in the toolkit they are built with: CUDA 13 needs a newer driver and a
Turing (compute capability 7.5) or newer GPU; CUDA 12 covers older drivers and GPUs from Maxwell
on. The installer checks both against what `nvidia-smi` reports and recommends the newest one
the machine can run.

To move to a new llama.cpp build:

```bash
gh release view b12000 -R ggml-org/llama.cpp --json assets --jq '.assets[] | [.name, .size, .digest] | @tsv'
```

Update `release`, `baseUrl` and each variant's asset `name`, `size` and `sha256` (the `digest`
without its `sha256:` prefix), then run `cargo test -p demido-catalog`, which checks that every
variant has assets and every asset a size and a checksum.

The app asks llama.cpp what each local model can do (`models/probe.rs`), and asks again for every
model once the build changes. Check that the new build still reports what it reads: `modalities`
and `chat_template_caps.supports_tool_calls` in `GET /props`, and the trace line
`chat template, thinking = 1` at start with `--log-verbosity 4`. When one is gone, the model shows
that capability as unknown rather than wrong.

## models.json

| Key | What |
|---|---|
| `families[]` | Qwen and Gemma, with the description the installer shows |
| `tiers[]` | GPU tiers, largest first. The first tier whose `minVramGb` fits the GPU's memory wins |
| `cpuTiers[]` | The same for CPU-only machines, by `minRamGb` |
| `smokeTest` | A tiny model for testing the install path: `demido-setup-cli --dev --model smoke` |
| `search.models[]` | Search models, which find passages of attached files by meaning, largest first. The first whose `minVramGb` fits the GPU the runtime uses wins; a CPU-only machine gets the last |
| `speech.models[]` | Speech models, which write down what the person says for a chat model that cannot hear, largest first, picked like the search models |

Each tier has a `contextLength` and one model per family: `name`, `repo`, `file`, `quant`,
`size` and `sha256`. The sizes leave room for the context (KV cache) and for the desktop's own
use of the GPU; a 12 GB card lands in `mainstream` (10 GB) rather than a tier that would fill
it completely.

Each pick also has a `projector` (`file`, `size`, `sha256`): the repo's `mmproj` file, which lets
the model read pictures and, for Gemma 4 E2B, E4B and 12B, hear sound. The installer saves it next
to the model, where the app finds it. Take the BF16 one, the precision the models were trained in;
the catalog test rejects any other. The tier test counts only the model's size, since llama.cpp
fits the projector beside the model and sizes the context to what is left.

Each search model also has the prompts its model card asks for, `queryPrefix` before a question
and `documentPrefix` before a passage, `relevance`, the similarity from which a passage is
taken as about the question, and `microBatch`, the tokens llama.cpp decodes at once: 2048 (a
whole input) for a model that reads text both ways, which llama.cpp requires, and less for one
that reads left to right, which saves GPU memory. Measure it again for a new model: embed a long document, then
questions it answers and questions it does not, and put it between the two groups' best scores.
`gpuMemoryMb` is the GPU memory its server takes with those settings, which a chat model sized to
the GPU's free memory leaves for it: measure it as the difference `nvidia-smi` shows once the
server has embedded something.

Models are unsloth's quantized GGUFs ("UD" dynamic quants, and the quantization-aware "QAT"
builds for Gemma). The catalog test rejects any chat model outside `unsloth/`; a search model
may come from its maker when unsloth publishes none (Qwen3 Embedding is Qwen's own GGUF). A file's size and
SHA-256 are in the Hugging Face API:

```bash
curl -s "https://huggingface.co/api/models/unsloth/Qwen3.5-9B-GGUF/tree/main" | jq '.[] | select(.path | endswith(".gguf")) | {path, size, sha256: .lfs.oid}'
```

### Speech models

Each speech model has `name`, `repo`, `file`, `quant`, `size`, `sha256` and a `projector`, its
audio encoder, without which it hears nothing; `contextLength`, the tokens of its server's one
slot, which must hold the longest recording (a five-minute clip is 3,633 tokens for Qwen3-ASR)
and its transcript (about 800 more); and `gpuMemoryMb`, what its server takes on the GPU.

They are Qwen3-ASR 1.7B, from 16 GB of GPU memory, and 0.6B below that and on the CPU, both
Q8_0. They come from `ggml-org`, the llama.cpp project, rather than unsloth: unsloth publishes no
Qwen3-ASR, and `unslothai` on Hugging Face is a different account, never a source. The catalog
test holds the speech models to `ggml-org/Qwen3-ASR-`.

To measure a new one, run its `llama-server` with `-m`, `--mmproj` and the context, and:

- transcribe clips through `/v1/audio/transcriptions` with each of the repo's projectors, on
  clean, noisy and hard speech, and take the smallest projector that writes the same words (Q8_0
  for Qwen3-ASR: the same transcripts as bf16, 156 and 272 MiB less GPU memory);
- for `gpuMemoryMb`, add up the model, KV and compute buffers llama.cpp logs with `-lv 4`, and its
  "estimated worst-case memory usage of mmproj"; or the difference `nvidia-smi` shows, when
  nothing else uses the GPU;
- time a 5- and a 30-second clip on the GPU and on the CPU (`-ngl 0 --no-mmproj-offload --device
  none`), with a fresh clip each run: llama.cpp caches the prompt, so the same clip twice is
  answered from its cache. The catalog's `$comment` has the times on an RTX 3060 and an
  i7-12700K.
