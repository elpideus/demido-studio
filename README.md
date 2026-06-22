<p align="center">
  <img src="public/logo.svg" alt="Demido Studio" width="120" />
</p>

# Demido Studio

> Open-source desktop AI chat for everyone — local models, cloud APIs, and tool use in one place.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8D8?logo=tauri)](https://tauri.app)
[![React 19](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev)

Demido Studio is a native desktop application built with **Tauri 2** (Rust backend) and **React 19** (TypeScript frontend). It gives you a single, privacy-respecting interface for every AI provider you use — from locally-running models on LM Studio or Ollama to cloud APIs from Anthropic, OpenAI, and Google Gemini — with full support for MCP (Model Context Protocol) tool servers, extended thinking / reasoning, file attachments, and an agent mode that can run multi-step tasks autonomously.

---

## Features

| Category | Details |
|----------|---------|
| **Multi-provider** | Anthropic, OpenAI, Google Gemini, Groq, LM Studio, Ollama, and any OpenAI-compatible endpoint |
| **Local model support** | Zero-config with LM Studio and Ollama (pre-seeded, no API key required) |
| **MCP tool use** | Spawn stdio MCP servers; tools are auto-discovered and available in chat |
| **Agent mode** | Three trust tiers — Cautious, Balanced, Autonomous — with path-sensitive permission gating |
| **Extended thinking** | Displays reasoning/thinking blocks from models that support them (Anthropic extended thinking, o-series) |
| **File attachments** | Attach images, PDFs, text files, and code files to any message |
| **Skills** | Drop custom skill files into the skills folder; they are injected as system context |
| **Floating windows** | Detachable, snappable panels for Settings and Tools with edge-docking |
| **Full-text search** | Instant conversation search powered by SQLite FTS5 |
| **Conversation export** | Export any conversation to Markdown |
| **Privacy first** | No telemetry. All data (conversations, settings, API keys) stays on your machine |

---

## Installation

> Current version: **v0.4.1** (pre-release). Download links below go directly to the installer for your platform.

### Windows

| Your PC | Recommended installer | Alternative |
|---|---|---|
| Most laptops & desktops (Intel / AMD) | [Download installer (.exe)](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_x64-setup.exe) | [MSI](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_x64_en-US.msi) · [Portable .exe](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_v0.4.1_windows_x64_portable.exe) |
| ARM-based (Surface Pro X, Snapdragon) | [Download installer (.exe)](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_arm64-setup.exe) | [MSI](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_arm64_en-US.msi) · [Portable .exe](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_v0.4.1_windows_arm64_portable.exe) |

Run the installer and follow the prompts. Windows may show a SmartScreen warning on first launch — click **More info → Run anyway**.

### macOS

| Your Mac | Download |
|---|---|
| Apple Silicon (M1 / M2 / M3 / M4) | [Download .dmg](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_aarch64.dmg) |
| Intel or unsure | [Download Universal .dmg](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_universal.dmg) |

Open the `.dmg`, drag **Demido Studio** to your Applications folder. On first launch macOS may block it — go to **System Settings → Privacy & Security** and click **Open Anyway**.

> macOS auto-update is not yet supported. Check **Settings → Info** for new versions or watch [Releases](https://github.com/elpideus/demido-studio/releases).

### Linux

| Your system | Recommended | Alternatives |
|---|---|---|
| Ubuntu / Debian / Mint (x86_64) | [.deb](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_amd64.deb) | [AppImage](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_amd64.AppImage) · [.rpm](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio-0.4.1-1.x86_64.rpm) |
| Fedora / openSUSE / RHEL (x86_64) | [.rpm](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio-0.4.1-1.x86_64.rpm) | [AppImage](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_amd64.AppImage) |
| Any distro (x86_64, no install) | [AppImage](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_amd64.AppImage) | — |
| Raspberry Pi / ARM64 | [.deb](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_arm64.deb) | [.rpm](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio-0.4.1-1.aarch64.rpm) |
| ARMv7 (older Raspberry Pi) | [.deb](https://github.com/elpideus/demido-studio/releases/download/v0.4.1/Demido.Studio_0.4.1_armhf.deb) | — |

**AppImage** — make executable and run:
```bash
chmod +x Demido.Studio_0.4.1_amd64.AppImage
./Demido.Studio_0.4.1_amd64.AppImage
```

**Deb package:**
```bash
sudo dpkg -i Demido.Studio_0.4.1_amd64.deb
```

**RPM package:**
```bash
sudo rpm -i Demido.Studio-0.4.1-1.x86_64.rpm
```

> The AppImage on x86_64 supports silent auto-update. Deb/RPM/ARM packages require manual download of new versions.

---

## Screenshots

> Screenshots coming soon.

---

## Requirements

| Tool | Minimum version |
|------|----------------|
| [Node.js](https://nodejs.org) | 20 LTS |
| [Rust](https://rustup.rs) | 1.77 (stable) |
| [Tauri CLI prerequisites](https://tauri.app/start/prerequisites/) | — |

On **Windows**: Visual Studio C++ Build Tools are required by Tauri. Follow the [Tauri Windows prerequisites guide](https://tauri.app/start/prerequisites/#windows).

On **Linux**: `libwebkit2gtk-4.1`, `libssl-dev`, and a few other packages are needed. See the [Tauri Linux prerequisites guide](https://tauri.app/start/prerequisites/#linux).

---

## Getting started

### 1. Clone

```bash
git clone https://github.com/demido-studio/demido-studio.git
cd demido-studio
```

### 2. Install JS dependencies

```bash
npm install
```

### 3. Run in development mode

```bash
npm run tauri dev
```

This starts the Vite dev server and the Tauri application simultaneously. Hot-reload works for the frontend; Rust changes require a full recompile.

### 4. Build a release installer

```bash
npm run tauri build
```

Produces NSIS (`.exe`) and MSI installers under `src-tauri/target/release/bundle/`.

---

## Configuration

All configuration is stored locally in the OS app-data directory:

| Platform | Path |
|----------|------|
| Windows | `%APPDATA%\studio.demido.app\` |
| macOS | `~/Library/Application Support/studio.demido.app/` |
| Linux | `~/.local/share/studio.demido.app/` |

**API keys** are written to `secrets.json` in the same directory. They are never sent anywhere except the provider you configure.

### Adding providers

Open **Settings → Providers**, then click **Add Provider**. Fill in:

- **Name** — display name
- **Type** — `anthropic`, `gemini`, or `openai_compat`
- **Base URL** — the API endpoint (e.g. `https://api.openai.com/v1`)
- **API Key** — stored locally and never logged

### Adding MCP servers

Open **Tools → MCP**. Click **Add Server** and provide the command to launch the server process (stdio transport). The server is started on save and its tools are immediately available in chat.

### Skills

Drop any `.md` or `.txt` file into the `skills/` folder inside the app-data directory. The file is listed in the **Tools → Skills** panel and can be toggled per-conversation. When active, its content is prepended to the system prompt.

---

## Architecture overview

```
demido-studio/
├── src/                     # React 19 + TypeScript frontend
│   ├── components/          # UI components (chat, settings, sidebar, tools, windows)
│   ├── stores/              # Zustand state stores
│   ├── lib/                 # Tauri command wrappers, utilities
│   └── types.ts             # Shared TypeScript types
└── src-tauri/               # Rust / Tauri backend
    └── src/
        ├── commands.rs      # All #[tauri::command] handlers (IPC surface)
        ├── db/              # SQLite layer — migrations + typed repository modules
        ├── providers/       # Streaming chat — Anthropic, Gemini, OpenAI-compat
        ├── mcp/             # MCP client (stdio transport)
        ├── agent/           # Agent loop, permission gating
        ├── skills.rs        # Skills discovery and loading
        └── secrets.rs       # API key storage (app-data JSON)
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for a deep-dive into architectural decisions, data-flow diagrams, and design patterns.

---

## Updating

Demido Studio includes a built-in auto-updater. When a new version is released, you will be notified inside the app.

**To check manually:** open **Settings → Info** and click **Check for Updates**. If an update is available, click **Download & Install** — the installer runs automatically and the app relaunches.

**Supported platforms for silent auto-update:**

| Platform | Auto-update |
|----------|-------------|
| Windows x64 | ✓ |
| Windows ARM64 | ✓ |
| Linux x86\_64 (AppImage) | ✓ |
| macOS | — (download manually from [Releases](../../releases)) |
| Linux aarch64 / armv7 | — (download `.deb` / `.rpm` manually from [Releases](../../releases)) |

---

## Project status

Demido Studio is **pre-release** (`v0.4.1`). The core functionality is stable and used daily, but the public API (IPC commands, store shapes) may change before `v1.0`. Feedback and contributions are very welcome.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines, branch conventions, and the PR process.

Short version:

1. [Open an issue](../../issues/new/choose) to discuss the change before starting work.
2. Fork → branch (`feat/my-feature` or `fix/short-description`) → PR.
3. All PRs require passing CI checks and at least one approving review.

---

## License

Demido Studio is released under the [GNU General Public License v3.0](LICENSE).
