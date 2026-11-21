# 03. The inference runtime

Written 2026-11-21, directly. Narrow on purpose: what runs the models, where its files come from, and how
Demido picks the right build for a computer. Model-specific tuning and the small-model techniques are separate
notes, still to be written.

**Read** means I read it from the source myself (the registry, the GitHub API, this machine). **Search** would
mean search results only; there are none in this note.

## What is true today

| Fact | Checked |
|---|---|
| llama.cpp lives at `ggml-org/llama.cpp`. Licence MIT. Not archived; last push 2026-11-20. | **Read**: GitHub API, repository record |
| It reached **v1.0.0 on 2026-11-13**. The release notes say `llama-server` was hardened (SSRF, CORS, DNS rebinding) and gained an audio speech endpoint. | **Read**: GitHub API, release `v1.0.0` |
| Builds are published per commit under tags like `b12600` (2026-11-20), marked as pre-releases. The stable release carries one small file, `nightly-tag.txt`, naming the build it stands for (`b12359` for v1.0.0). | **Read**: release list and that file |
| Each build ships 38 files. Among them, for Linux x64: CPU, **CUDA 12.8**, **CUDA 13.5**, **ROCm 10.0**, Vulkan, SYCL, OpenVINO. For Windows x64: CPU, CUDA 12.4, CUDA 13.5, ROCm 10.0, Vulkan, SYCL, OpenVINO. For macOS: arm64 and x64. Also Linux and Windows arm64, Android. The CUDA runtime libraries come as separate `cudart-…` files. | **Read**: asset list of `b12600` |
| Releases link to GitHub build attestations. | **Read**: release body |

So the brief's wish holds as stated: a small Demido that downloads the right runtime from the people who make
it. Linux CUDA builds are official now, so nothing has to be compiled on the user's machine.

## Decision

**llama.cpp's `llama-server`, run by Demido as a separate process, one per loaded model.** Demido talks to it
over the loopback network.

Why a process and not a library linked into Demido:

1. **Unloading is certain.** "Load when needed, unload when not" is the theme of the whole project. Ending a
   process frees its graphics memory every time. A linked library can leak or hold on.
2. **The build is chosen at install time, not at compile time.** One Demido binary works with the CUDA, ROCm,
   Vulkan or CPU build. Linking would mean one Demido per backend.
3. **Nothing is redistributed.** The files are the publisher's own, downloaded from the publisher's releases.
4. **A crash in the runtime does not take Demido down.**
5. **It already speaks what Demido needs**: an OpenAI-style API, and grammar- and schema-constrained output,
   which is the base of reliable tool calls on small models.

Cost: one more process, and a little latency on the loopback hop. Both are small next to a model's own time.

## Picking the build

Demido reads the computer, then recommends. First cut of the rule, for Linux and Windows x64:

1. NVIDIA card: ask the driver which CUDA it supports (through NVML). 13.5 or newer: the CUDA 13.5 build.
   12.8 or newer (12.4 on Windows): the CUDA 12 build. Otherwise Vulkan.
2. AMD card: Vulkan. ROCm is offered, not recommended, until its requirements are checked.
3. Intel card: Vulkan. SYCL and OpenVINO are offered.
4. No usable card: the CPU build.
5. macOS: the arm64 or x64 build (Metal is built in).

"Stable" means: the newest non-pre-release, resolved through its `nightly-tag.txt`. The user can pick a newer
build by hand.

## Measured on the owner's machine

NVIDIA GeForce RTX 3060, 12,288 MiB, driver 620.70.06. Intel Core i7-12700K, 20 threads, AVX2. 31 GB RAM.
Linux 7.3.4 (CachyOS), x86_64. No `llama-server` and no Ollama installed.

## Not checked yet

- What the ROCm, SYCL and OpenVINO builds need on the user's side.
- Whether the "ubuntu" builds run unchanged on every common Linux (they depend on the system's C library).
- How to verify a download: the attestations exist; the exact check is not written.
- Other runtimes (for example for speech, or for Apple's MLX). llama.cpp is the first, not the only one.

## Sources

- <https://api.github.com/repos/ggml-org/llama.cpp>
- <https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/v1.0.0>
- <https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=6>
- <https://github.com/ggml-org/llama.cpp/releases/download/v1.0.0/nightly-tag.txt>
