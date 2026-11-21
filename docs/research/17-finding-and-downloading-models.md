# 17. Finding and downloading models: Hugging Face's interface, and what was tried

Written 2026-11-21, while building brief 4.7 (models browser and downloader).

How sure each line is: **Tried** (run on the owner's computer today), **Read** (read today in the source
linked).

## Hugging Face's public interface

| Fact | How sure |
|---|---|
| A search: `GET https://huggingface.co/api/models?filter=gguf&search=…&sort=trendingScore\|downloads\|likes\|lastModified&direction=-1&limit=…`. With `expand[]=gguf` each model comes with what its GGUF files say: the number of parameters (`gguf.total`), the architecture, the context it can hold, and its chat template; with `expand[]=cardData` its licence; `gated`; `pipeline_tag`; `tags`. The next page is in the `Link` header, as a `cursor` | Tried |
| A model's page: `GET /api/models/{owner}/{name}?blobs=true&expand[]=siblings…` lists every file with its size and, for large files, the SHA-256 Hugging Face keeps (`lfs.sha256`), and the commit (`sha`) | Tried: `Qwen/Qwen3-4B-GGUF` lists `Qwen3-4B-Q4_K_M.gguf`, 2,497,280,256 bytes, the same size as the copy on this computer |
| A file: `GET /{owner}/{name}/resolve/{commit}/{path}` answers 302 to a content network (`us.aws.cdn.hf.co/xet-bridge-us/…`), with `x-linked-size`, `x-linked-etag` (the SHA-256) and `x-repo-commit`. A `Range` request through the redirect answers 206: a download can go on from where it stopped | Tried |
| Limits, for an address without an account: 500 interface requests, 3,000 file requests and 100 page requests in each 5 minutes ("subject to change"). The answer says how many are left: `ratelimit: "api";r=499;t=188`. Above the limit: 429, and `t=` is the seconds to wait | Read: <https://huggingface.co/docs/hub/rate-limits>; the header seen on every answer |
| A gated model needs an account and the maker's terms accepted on its page; its files then need a key with every request | Read: `gated` is `"auto"` or `"manual"` for those |

## What real repositories hold, and what that changed

| Seen | What Demido does |
|---|---|
| The quantization is in the file's name: `Qwen3-4B-Q4_K_M.gguf`, `Qwen3.8-27B-UD-Q4_K_XL.gguf`, `model.IQ4_XS.gguf` | It is read from the name, and the files are ranked by it |
| A large model comes in shards: `Q4_K_M/big-Q4_K_M-00001-of-00002.gguf` | Shards are one quantization, downloaded into one folder |
| A model that sees images has a vision part, `mmproj-*.gguf`, often in several precisions | The one in half precision is downloaded with whichever quantization is chosen, into one folder. **Tried:** `ggml-org/SmolVLM-256M-Instruct-GGUF` came down as a folder of two files, and the runtime, loading it, said it takes images and video |
| `unsloth/Inkling-2-Mini-GGUF` (26B) also holds, in a folder of its own, a 1 GB draft model in Q8_0 and BF16, and calibration data (`imatrix_unsloth.gguf`). The first build offered the draft model as the 26B model's "Q8_0", and recommended it | A file much smaller than the model would be in its quantization (under 60% of parameters × bits) is not offered; nor is calibration data. They are listed as "not offered" |
| Searches for GGUF also find models that make pictures (`text-to-image`) | They are left out: the runtime does not run them |
| Trending GGUF repositories are often re-uploads with words like "uncensored" or "abliterated" in their names | Not judged: Demido shows what Hugging Face lists, with its maker and licence |

## Tried, end to end

| What | What happened |
|---|---|
| `demido models search smollm2 135m instruct` | Eight repositories, each with its licence, size and "fits" |
| `demido models download bartowski/SmolLM2-135M-Instruct-GGUF` (the recommended Q8_0) | 138 MB in about 7 seconds. Its SHA-256 matched the one Hugging Face lists. It went to the models folder as one file, and where it came from was written beside it. Loaded and asked "What is the capital of France?", it answered "The capital of France is Paris." at 388 tokens a second |
| The same through the window, with `bartowski/SmolLM2-360M-Instruct-GGUF` | Search, the model's page, Download (369 MB), progress on the button and a bar, then "On this computer" on its page and in the list |

## The estimate of what a model needs

What a model needs on the graphics card is its file, the cache of what it holds in mind, and a little for the
runtime. The cache was measured for Qwen3 4B at about 1.2 GB at 8,192 tokens, which is about 300 MB a
billion parameters; large models share it across more heads, so it is held to 4 GB. This is an estimate
only. The runtime measures the real figure when it loads a model (DECISIONS K22), and the models page
shows that from then on. Not checked against more than one model.
