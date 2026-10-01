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

Each tier has a `contextLength` and one model per family: `name`, `repo`, `file`, `quant`,
`size` and `sha256`. The sizes leave room for the context (KV cache) and for the desktop's own
use of the GPU; a 12 GB card lands in `mainstream` (10 GB) rather than a tier that would fill
it completely.

Each search model also has the prompts its model card asks for, `queryPrefix` before a question
and `documentPrefix` before a passage, `relevance`, the similarity from which a passage is
taken as about the question, and `microBatch`, the tokens llama.cpp decodes at once: 2048 (a
whole input) for a model that reads text both ways, which llama.cpp requires, and less for one
that reads left to right, which saves GPU memory. Measure it again for a new model: embed a long document, then
questions it answers and questions it does not, and put it between the two groups' best scores.

Models are unsloth's quantized GGUFs ("UD" dynamic quants, and the quantization-aware "QAT"
builds for Gemma). The catalog test rejects any chat model outside `unsloth/`; a search model
may come from its maker when unsloth publishes none (Qwen3 Embedding is Qwen's own GGUF). A file's size and
SHA-256 are in the Hugging Face API:

```bash
curl -s "https://huggingface.co/api/models/unsloth/Qwen3.5-9B-GGUF/tree/main" | jq '.[] | select(.path | endswith(".gguf")) | {path, size, sha256: .lfs.oid}'
```
