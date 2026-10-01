# Third-party notices

Demido Studio is licensed under the GNU General Public License v3.0 or later (see `LICENSE`).
It is built on the open source work listed here. Full license texts ship with each package in
its source distribution.

## Included in the app

| Component | License | Use |
|---|---|---|
| [Tauri](https://tauri.app) | MIT or Apache-2.0 | Desktop shell |
| [React](https://react.dev) | MIT | User interface |
| [Lucide](https://lucide.dev) | ISC | Icons |
| [Lightweight Charts](https://github.com/tradingview/lightweight-charts) by TradingView | Apache-2.0 | Price charts |
| [TradingView-API](https://github.com/Mathieu2301/TradingView-API) by Mathieu Colmon | ISC | Real-time market data |
| [dukascopy-node](https://github.com/Leo4815162342/dukascopy-node) by Leonid Pyrlia | MIT | Historical market data |
| [react-markdown](https://github.com/remarkjs/react-markdown), remark-gfm, remark-math, rehype-katex, rehype-highlight | MIT | Rendering answers |
| [KaTeX](https://katex.org) | MIT | Math |
| [highlight.js](https://highlightjs.org) | BSD-3-Clause | Code highlighting |
| [zustand](https://github.com/pmndrs/zustand) | MIT | State |
| [axios](https://axios-http.com), [ws](https://github.com/websockets/ws), [jszip](https://stuk.github.io/jszip/) | MIT (jszip: MIT or GPL-3.0) | Used by TradingView-API |
| [SQLite](https://sqlite.org) via rusqlite | Public domain, MIT | Chat storage |
| [portable-pty](https://github.com/wezterm/wezterm) from WezTerm, [avt](https://github.com/asciinema/avt) from asciinema | MIT; Apache-2.0 | The terminal the assistant's commands run in, and reading its screen |
| [pdf-extract](https://github.com/jrmuizel/pdf-extract) and [lopdf](https://github.com/J-F-Liu/lopdf), [calamine](https://github.com/tafia/calamine), [quick-xml](https://github.com/tafia/quick-xml), [image](https://github.com/image-rs/image), [encoding_rs](https://github.com/hsivonen/encoding_rs) | MIT; MIT; MIT; MIT; MIT or Apache-2.0; (Apache-2.0 or MIT) and BSD-3-Clause | Reading attached files: PDF, spreadsheets, Word, PowerPoint and OpenDocument, images, old text encodings |
| Rust crates (tokio, serde, reqwest, notify, keyring, sysinfo, zip, sha2 and others) | MIT or Apache-2.0 | Backend |

Lightweight Charts shows TradingView's attribution logo on every chart, as its license asks.

## Read while the app runs

| Component | License | Use |
|---|---|---|
| [models.dev](https://models.dev) | MIT | What Gemini models can do (tools, images, audio) |

## Downloaded by the installer

These are not part of the installer. Setup downloads them from their official sources onto the
person's computer and verifies each download against a pinned SHA-256 checksum.

| Component | License | Source |
|---|---|---|
| [llama.cpp](https://github.com/ggml-org/llama.cpp) | MIT | GitHub releases |
| [Node.js](https://nodejs.org) | MIT and bundled third-party licenses | nodejs.org |
| [uv](https://github.com/astral-sh/uv) | MIT or Apache-2.0 | GitHub releases |
| [CPython](https://www.python.org) (python-build-standalone via uv) | PSF License | GitHub releases |
| numpy, pandas, matplotlib, requests | BSD-3-Clause, PSF-based, Apache-2.0 | PyPI |
| Qwen and Gemma models, quantized by [unsloth](https://huggingface.co/unsloth) | Apache-2.0 (see each model card) | Hugging Face |
