# Demido Studio: Brief for Claude Code

Save this file as `docs/BRIEF.md` in the new repo. Kickoff message: "Read docs/BRIEF.md completely, make your plan, and start. Stop for my review after the brand kit, and again after the interactive preview." My original request is preserved word for word in Appendix A. If this brief and Appendix A conflict, ask me.

## 0. How to work

1. Read everything first: this whole brief, including Appendix A, before starting.
2. You make all technical decisions:
   * the stack, languages, frameworks and libraries
   * the architecture and project structure
   * runtimes and storage
   * the build plan and the order of work

   This field moves fast, so research the current state before choosing. Write down your decisions and the reasoning behind them in the repo, so I can review them.
3. Fixed order. This part is mine:
   1. Build the brand kit. Then stop for my review.
   2. Build an interactive preview with fake data (section 4.33). Then stop for my approval.
   3. Only then start the real feature work.
4. After that, work in increments you choose, and stop to report at sensible checkpoints.
5. Ask me before anything irreversible, and before anything that costs money (for example paid API calls).
6. Use your skills and subagents to your advantage, especially for design, the brand kit and research.
7. No silent stubs. Anything not implemented yet must be visibly marked as such in the app.

Ideas and pushback: never hold back.

* Speak up: whenever you find a better idea, a missing feature, a risk, a simplification, or something in this brief you think is wrong, tell me. Do this even if it contradicts what I asked for. I want your honest opinion, not polite compliance.
* Write it down: keep a written log of your ideas and suggestions.
* When to raise ideas:
   * at each checkpoint
   * right away, if an idea affects the work in progress
* When to implement:
   * small, obvious improvements: just do them, and mention them in your report
   * big changes or changes of direction: discuss with me first

## 1. Hard rules

* Never look at previous Demido Studio versions. Don't clone, read, search or reference them. That includes the GitHub repo that will be renamed `demido-studio-third` (and its CI workflows) and any old Demido folders on my machine. They are bad and must not influence you. If you're unsure whether something is an old version, ask.
* License: `GPL-3.0-or-later`. Every dependency must be compatible. Facts to keep in mind:
   * MIT, BSD, Apache-2.0, MPL-2.0, LGPL and GPLv3 are compatible.
   * AGPL-3.0 code can be combined under section 13 of GPLv3, but those parts keep their AGPL terms. Some of the search engines below are AGPL, so tell me how you handle them.
   * GPLv2-only, proprietary and "non-commercial" licenses are not compatible.
* No redistribution. The installer stays lightweight. Most components are downloaded and installed on the user's system from their official sources, which avoids redistribution violations.
* Watch for look-alike repos. Several projects named in this brief have clone repos, some of them likely malicious. Verify the canonical source before using anything (section 5).
* Where it runs:
   * a multi-platform desktop app (Linux, Windows, macOS)
   * server mode, including on a VPS
   * a web UI in the browser
   * a CLI
   * an API

   Details are in 4.32. I develop on Linux.
* Mobile comes later. Don't build it now, but don't make choices that would block it.

## 2. GitHub

1. Confirm the account. Ask me which GitHub account owns the current `demido-studio` repo.
2. Rename the old repo. Rename `demido-studio` to `demido-studio-third`, without cloning or reading it.
3. Create the new repo. Create a new public `demido-studio` repo under `GPL-3.0-or-later`, and publish the project there.
4. Automatic builds on tag/release. When I push a version tag or publish a GitHub release, CI must automatically build Demido Studio for every supported platform and attach the build artifacts to the release. My current repo already has this, but don't look at it; this description is the requirement.

## 3. Vision and principles

What it is: Demido Studio is an advanced, multi-platform, open-source AI harness. Its aim is to make the local LLM experience better and easier for everyone.

* Main goal: make smaller, "stupider" models behave, using lots of techniques. My examples:
   * self-sanitizing tool requests
   * compressed prompts
   * vector reasoning instead of language reasoning, where applicable

   Research and propose the full set.
* Local first. It will mostly be used to run local models. Models load only when needed and unload automatically when they're not needed. This is a recurring theme across the whole project.
* Fully transparent. Never hide anything. Things can be grouped properly, or placed behind extra steps so the UI doesn't feel overwhelming, but nothing is ever hidden. The user can always access and tweak any setting.
* Simple and approachable, while giving power users all the tools and parameters they need.
* Modular and easy to work on.
   * It's easy to swap parts and to add or remove features without editing things in too many places.
   * Everything is easily edited.
* UI quality: modern, clean and polished, with sleek, subtle animations and Apple Design level care and attention to detail.
* Use it anywhere: desktop app, web browser, CLI or API, whether it runs locally or on a remote server.

## 4. Features

### 4.1 Installer and runtimes

* Easy installer that detects the user's components and recommends what to download and install based on them (for example CUDA rather than ROCm).
* Lightweight: most things are downloaded and installed on the user's system by the installer.
* More later: more runtimes and components can be downloaded from inside the app after installation.
* Server installs: installing on a headless server or VPS must also be easy (4.32).

### 4.2 Model loading and model-specific optimizations

* Fast loading: extremely optimized and performant model loading that doesn't affect the model's capabilities.
* Per-model tuning: model-specific optimizations and performance improvements.

### 4.3 Making small models behave

The techniques from section 3, plus whatever else you find works. Where something is experimental (for example vector reasoning), research what's actually feasible and discuss it with me.

### 4.4 Anti-prompt-injection system

Protects against prompt injection from any external content that reaches the model: web pages, emails, files, tool outputs, messages from chat platforms and from other sessions.

### 4.5 Chat interface

* Clean and polished.
* Rendering: markdown, Mermaid charts, and even custom elements that the model makes on the spot.

### 4.6 Task list with sub-tasks

* Task-list behavior, where each task can contain sub-tasks. This is really important for smaller models and context-constrained systems.
* Visual reference (my screenshot): a vertical checklist in a dark theme.
   * Done: solid circle with a check mark.
   * In progress: animated spinner arc, brightest text.
   * Pending: dashed circle outline, dimmer text.

### 4.7 Models browser and downloader

* Browsing: a search, sort and filter system.
* Capability info: vision, hearing, tool usage, thinking, and so on.
* Recommendations: models recommended based on the user's system.
* Quantization selector, with quantization recommendations based on the user's system.
* Automatic handling of models with different capabilities (mmproj, vision, embedding, and so on).

### 4.8 Adding capabilities through embeddings

Embedding support that adds features to models that wouldn't normally have them (for example, adding vision to a model that doesn't support it). Research what's feasible and propose an approach.

### 4.9 Thinking control

A thinking toggle or a levels selector, for every model that supports thinking.

### 4.10 Providers

* Initial providers: Gemini (Google AI), Anthropic (both subscription and API), OpenRouter and OpenCode. More will come later.
* Anthropic:
   * API: using an Anthropic API key.
   * Subscription: this means the user's local Claude Code CLI, just like OpenCode. Demido works through the user's own installed Claude Code. The current rules are in section 5; follow them.
* OpenCode: I mean the local OpenCode CLI tool, which offers free models. See section 5 for what I found about how it exposes them.

### 4.11 Tools system

* One concept: Tools.
   * In Demido there's no distinction between skills and MCPs; both are Tools.
   * A Tool can contain both functionality and guidance in a single place.
* Traditional MCPs are supported but shown as Deprecated.
   * The reason: Demido has a more integrated and stable tools system that is properly indexed and doesn't load the context for no reason.
   * Models can "convert" or "install" an MCP as a Demido tool so it gets those advantages. Converted MCPs show as External instead of Deprecated.
* Classic skills are also shown as Deprecated. Skills in Demido are part of Tools.
* Language: Tools are built in Rust. If you think there's a better way, discuss it with me.
* Indexing: Tools are indexed as efficiently as possible, using as little context as possible, without hurting the models' ability to know they exist and when to use them.
* Editing: a tool's instructions (the skill part) can be edited without leaving the app.

### 4.12 Tools browser and updater

* In-app tools browser and updater.
* Automatic updates: supported, but disabled by default. If the user turns it on, they take responsibility for not checking that each update is safe.
* Automatic audit option: the user selects a model, and that model audits every tool before it's installed or updated.

### 4.13 Caveman support

* Simple language everywhere internal: the entire system uses simple, straight-to-the-point language. Models are deeply guided to think and write that way. No detailed prose, especially in skills, prompts and thinking.
* The exception is output shown to the user, which uses normal language.
* My reading, which you should confirm with me if you read it differently: internal text (prompts, tool guidance and descriptions, thinking, messages between agents and sessions) is caveman, and only output shown to the user is normal prose.
* Reference: JuliusBrussee/caveman.

### 4.14 Answer style

Models answer as if the user had ADHD or another attention impairment: concise answers, similar to aircraft manuals.

### 4.15 Compaction

* Manual and automatic: compaction and auto-compaction, with a custom size option for auto-compaction.
* Default size: calculated from the system and the loaded model(s).

### 4.16 Permission levels

* Cautious: always asks the user for permission. Very safe, very annoying.
* Balanced: the best of both worlds. Asks only for things that could cause damage.
* Autonomous: never asks. Similar to Claude Code's Bypass Permissions mode.
* Smart: the model decides when to ask and when not to.

### 4.17 Agents and resource-aware model juggling

* Resource-aware agents: the agents system takes into account the resources available for local models.
   * My example, with made-up numbers: a model needing 6 GB of VRAM can only run the main session, with no agents, on an 8 GB system.
   * Even 12 GB might not be enough once you leave breathing room for embedding, audio models, context and so on.
   * On a 32 GB system, the same model might run the main session plus 2 agents.
* Borrowing: if the main session has to wait for an agent's answer anyway, it can "borrow" its loaded model to that agent. It gets the model back once the agent finishes or needs something from the main session.
* Fresh contexts help: agents work on fresh contexts, which could be extremely useful for making models behave.
* Deep integration with tasks: for example, tasks that are mechanical and don't need context can be assigned to agents.
* Juggling across everything, so multiple things can happen "at the same time". For example: a session is using the model and a scheduled task is due. The session either lends its model to the schedule, or unloads it to make room for the schedule's model. Once the schedule finishes, the session reloads its model if needed and continues.

### 4.18 CLI output compression

An RTK-like system, or rtk itself if there's no better option. It compresses CLI outputs while always keeping a way to see the entire output if it's really needed.

### 4.19 Session messaging

Sessions can message each other, so they can coordinate and work in harmony. For example, this avoids sessions overwriting each other's work.

### 4.20 Usage indicators

Usage indicators and viewers for remote providers (such as Anthropic and OpenRouter).

### 4.21 Session grouping and search

* Grouping and search for sessions.
* Automatic organization: sessions are named automatically and get tags assigned, to make searching easier.
* Content search toggle: off by default. It lets the user search inside the contents of local sessions too. This is slower, so the user is told that and advised to keep it off unless really necessary.

### 4.22 Voice

* Dictation:
   * Clicking the microphone icon in the chat starts recording, and what the user says gets written into the chat.
   * The text is corrected and normalized: talking noises like "ehm", repeated words, and the user's self-corrections are cleaned up.
* Voices browser.
* Voice cloning section:
   * The user records themselves or loads audio files.
   * The audio is cleaned, with noise and music removed.
   * It becomes a new voice that can be named and given an image and a description.
   * The user chooses which voice Demido has.
* TTS: messages can be read aloud in the voice the user chose.
* Voice mode:
   * A button that lets the user converse with the model just by speaking. The user speaks, and the model speaks back.
   * If a task will take time, the model must always tell the user to wait before doing it, so the user gets feedback on what they just said.
* Safety: cloning involves real people's voices, so handle consent responsibly.

### 4.23 Characters

* Profile: a custom name, picture and description.
* Behavior: a custom system prompt.
* Model: a model can be assigned, with custom model settings.

### 4.24 Files, viewer, editor and artifacts

* Artifacts: files the model creates and sends in the session.
   * The model can update them.
   * The user can see their previous versions.
   * They can be downloaded.
   * Claude Desktop has this feature.
* Session folder: the files in the current session's folder can be viewed.
* Media viewer: PDFs, pictures, videos and music.
* Editor: automatic syntax highlighting, plus Markdown and HTML previews, including side by side.

### 4.25 Browsing, web search, web fetch, data aggregators

* In-app browser with tabs. The model can interact with it in every way a normal user could, including:
   * click elements, or points on the page as a fallback
   * take screenshots
   * run scripts
   * inspect the network
   * read the console
   * open tabs
* Web search using these, or any combination of them, to get the most results, ranked and merged:
   * a3-search (probably a3s-search, section 5)
   * SoSearch
   * DonSeTch
   * DuckDuckGo
   * metadata-search-engine-rs
   * RustXNG (direct, no API)
   * spider as a fallback
* Search fallback chain:
   1. Fallback 1: free, keyless services. Where a service allows higher limits with a key, the user can enter their own key.
   2. Fallback 2: scraping.
   3. Fallback 3: the integrated browser. It also covers search engines that require JavaScript. (My original said "typescript"; I meant JavaScript.)
* Web fetch: normal fetching, falling back to the integrated browser when a website requires JavaScript.
* Data aggregators: there are useful data aggregators and fetchers for many things, and it's better to use them than to fetch from scratch.

### 4.26 Local servers

Models can start a local server and work with the main modern web technologies in an integrated way.

### 4.27 Scheduling and monitoring

* Examples that must work:
   * "Give me my email report every morning at 7."
   * "Monitor the price of this sweater and tell me when it goes on sale."
* No always-loaded model: the model doesn't have to stay loaded all the time. It unloads automatically when it isn't needed.
* Sharing: schedules share models with sessions as described in 4.17.

### 4.28 Email

* Google preset.
* Protocols: IMAP, SMTP, and POP3 (POP3 isn't a hard requirement).
* Optimized browsing and reading.
* Rendering: support for loading images and styles.
* Multiple accounts.
* LLM access: models operate on emails according to the permissions.

### 4.29 Messaging platforms

* WhatsApp.
* Telegram, via a bot or by logging in as a userbot.
* Discord.

See the terms-of-service notes in section 5.

### 4.30 Markets

* Features: charts, real-time data, history downloading, backtesting, and more.
* Data source: something like https://github.com/Mathieu2301/Tradingview-API, plus an in-app TradingView login that captures the session cookies.
* See section 5 for the risks.

### 4.31 App updates

In-app updates, with support for automatic updates.

### 4.32 CLI, server mode, web UI and API

* CLI: Demido Studio works from a CLI.
* Server mode: so I can use it from anywhere, even when it runs on a VPS.
* Web UI: because of server mode, the UI must also work from a web browser.
* API: for integrating Demido into other things, or for using it just as a model manager.
* Security: the server will be reachable remotely, and Demido can run commands, read email, control a browser and hold API keys. Securing it is essential.

### 4.33 Brand kit and interactive preview (these come first)

* Brand kit: build it before building the UI, so the UI stays consistent. Then stop for my review.
* Interactive preview with fake data: build it after the brand kit and before the actual work begins.
   * I need to be able to click through the app and judge its look, feel and flows.
   * Then stop until I approve.

## 5. Research notes (facts I found, to save you time; verify the current state)

### Providers

* Anthropic subscription through Claude Code:
   * Not allowed: extracting or reusing the subscription's OAuth tokens in another app, or spoofing the Claude Code client. Anthropic's docs have said since February 2026 that subscription tokens are only for Claude Code and Claude.ai, and it blocks spoofing server-side.
   * Supported: headless mode (`claude -p`) and the Agent SDK are official ways to use Claude Code programmatically.
   * Billing: since June 15, 2026, programmatic use (Agent SDK, `claude -p` and third-party tools) no longer counts against the subscription's normal limits.
      * It draws from a separate monthly credit equal to the subscription fee.
      * Unused credit doesn't roll over.
      * Beyond the credit, usage is billed at API rates through "extra usage", if the user has enabled it.
      * So Demido driving the user's Claude Code most likely spends that credit, and the usage indicators should reflect that.
   * Unclear: I couldn't find explicit documentation on whether third-party apps may wrap the Claude Code CLI binary. Check the current Claude Code legal and compliance docs and Anthropic's branding guidelines (for example, how the integration may be labeled in the UI), and tell me if anything blocks this.
* Gemini: using Gemini CLI's OAuth in third-party tools has been reported as prohibited. API keys are the standard route.
* OpenCode CLI:
   * `opencode serve` runs a headless HTTP server that exposes an OpenAPI endpoint. There's also a JS SDK. The server runs OpenCode's own sessions (its agent loop).
   * OpenCode includes free models that work without an account. The list rotates, and there are IP-level usage limits.
   * Some free models may use prompts for training during their free period.
   * Some free-tier models reportedly only accept requests from the OpenCode client, so go through the real local CLI and never spoof it.
   * Docs: https://opencode.ai/docs/server and https://opencode.ai/docs/zen

### Search engines and tools (canonical sources; watch for clones)

* RustXNG: https://github.com/sahenjp/RustXNG. AGPL-3.0-or-later, based on SearXNG, JSON API and MCP. Its README warns about search-provider terms.
* DonSeTch: reportedly github.com/dondai44423/donsetch. AGPL-3.0. Many look-alike forks exist, so confirm the original.
* SoSearch: https://github.com/NetLops/SoSearch. Rust; DuckDuckGo, Brave and Yahoo; HTTP API and MCP. Repos called "sosearch-rust-search-hub" are lookalikes; ignore them.
* a3s-search: a crate from A3S-Lab (docs: https://a3s-lab.github.io/a3s/docs/search). I believe this is the "a3-search" I meant. Check the license.
* metadata-search-engine-rs: https://github.com/MikeLuu99/metasearch-rust. Check the license.
* spider: probably https://github.com/spider-rs/spider
* rtk: https://github.com/rtk-ai/rtk. MIT. An unrelated crate is also called "rtk" (Rust Type Kit), so don't mix them up.
* caveman: https://github.com/JuliusBrussee/caveman
* Tradingview-API: https://github.com/Mathieu2301/Tradingview-API. Unofficial, written in Node.js.

### Terms-of-service and safety notes (discuss with me where a choice is needed)

* WhatsApp: there's no official API for personal accounts. Unofficial libraries risk getting the number banned. The official Business Platform requires a business account.
* Telegram: bots use the Bot API. Userbots need the user's own API credentials from my.telegram.org.
* Discord: self-bots (automating a user account) violate Discord's terms. Bot accounts are the allowed route.
* TradingView: the API above is unofficial, and capturing session cookies may break TradingView's terms.
* Gmail: full mail access uses restricted OAuth scopes, which require Google verification for publicly distributed apps. App passwords are an alternative.
* Voice cloning models: licenses vary, and many are non-commercial.
* Model weights: licenses vary, and some models are gated.

## Appendix A: Original request (verbatim)

I want you to build Demido Studio, an advanced multi-platform Open Source AI harness that aims at making the local LLM experience better and easier for everyone. The goal is making smaller "stupider" models behave via tons of techniques. Self-sanitizing tool requests, compressed prompts, vector reasoning instead of language reasoning where applicable, etc. The UI should be modern, clean and polished, with sleek, subtle animations and Apple Design level of care and attention to detail. You have so many skills at your disposal so use them to your advantage. The code needs to be modular and easy to work on. I want to be able to easily swap parts, add and remove features and more without the need to edit things in too many places. Everything should be easily edited. Demido Studio needs to be fully transparent and never hide anything. At most things should be grouped properly or under extra steps so that the UI does not feel overwhelming, but nothing should be ever hidden. User should always be able to access and tweak any setting they want. UI should be simple and easily approachable, while giving power users all the tools and parameters they need. These are the features I need:

* Easy installer that detects user's components and reccommends the things to download/install based on those (CUDA rather than ROCm, etc.). The installer should be lightweight and most things should be downloaded and installed on the user's system by the installer. This way we also avoid any redistributing violation issues. It should also possible to download more runtimes and such after installation from in-app.
* Extremely optimized and performant model loading without affecting the model's capabilities.
* Anti-prompt-injection system.
* Model-specific optimizations and performance improvements
* Clean and polished chat interface with support for markdown, mermaid charts, and even custom elements that the model makes on the spot.
* Task-list behavior (like `/home/elpideus/Pictures/Screenshots/Screenshot_20261120_120047.png`) in which each task can contain sub-tasks. Really important for smaller models and context-constrained systems.
* Models browser and downloader with:
   * Search, sort and filter system
   * Models capabilities info (vision, hearing, tool usage, thinking, etc.)
   * Models reccomendations based on user's system
   * Quantization selector with quantization reccomendations based on user's system
   * Automatic detection and handling of models with different capabilities (mmproj, vision, embedding, etc.)
* Embedding support to add features to models that would normally not support them (E.G: adding vision to a model that would normally not support it)
* Thinking toggler or levels selector, for all models supporting it.
* Support for Gemini (google AI), Anthropic (both subscription (Claude Code) and API), OpenRouter and OpenCode initially, with more coming later down the road.
* Traditional MCPs should be supported, but shown as deprecated. This is because I want Demido Studio to have a more integrated and stable tools system that is properly indexed and does not load the context for no reason. Because of this please add a way for models to "convert" or "install" MCPs as Demido Studio tools, so that they get the advantages. They will get shown as "External" tools instead of "Deprecated". As for skills, the classic skills should too be shown as deprecated. Skills in Demido are part of the Tools. In Demido Studio there should be no distinction between Skills and MCPs, as they should be seen as Tools. As such tools in Demido studio can contain both the functionality and guidance in a single place. Tools should be built in Rust (although if you think there are better ways we can discuss this). Tools should be indexed as efficiently as possible, to use as little context as possible, without interfering with the ability of LLMs to know about their existence and when to use them. Tools' instructions (the skill part) should be editable without leaving the app.
* In-App tools browser and updater, with autopmatic update support (disabled by default, as user should assume responsibility if they don't want to check that the update is safe to install). There should be an automatic audit option that allows the user to select a model that audits each and every tool before it gets installed or updated.
* Caveman support. The entire system should use simple language, except for the output shown to the user, skills, tools, descriptions, etc. The models should be deeply guided towards thinking and using simpler, straight to the point language. No need to write detailed prose, especially when writing skills, prompts or thinking.
* Compaction and autocompaction support, with possibility to set a custom size for autocompaction. The default size should be calculated based on the system and loaded model(s).
* Permission levels:
   * Cautious: Always asks the user for permission. Very safe, very annoying.
   * Balanced: Best of both worlds. Asks only for things that could be damaging.
   * Autonomous: Never asks. Similar to Claude Code's Bypass Permissions mode.
   * Smart: The model decides when to ask and when not to
* Models should be guided to answer as if user had ADHD or any other attention impairment. Concise answers, similar to aircraft manuals.
* Agents system, that take into consideration the amount of resources they have (for local models). For example a model that requires 6GB of VRAM can only load the main session with no agents on an 8GB VRAM system, or even 12GB if we take into consideration that we have to leave some breathing space for embedding, audio model, context etc.. Using the same model, only the main session and 2 agents can run on a 32GB VRAM system. I am making numbers up but you get the idea.
   * Main session can "borrow" its loaded model to an agent if it has to wait for its answer anyway, and get it back once the agent has finished or requires somethign from the main session.
   * Agents could potentially be extremely useful in making models behave since they work on fresh contexts. The agents system should be deeply integrated with the tasks system. One example is tasks that are mechanical and don't require context could be assigned to agents.
* RTK-like system (or rtk itself if no better options), that compresses cli outputs while still offering a way to see the entire input if really really needed.
* Session messagging system. One session can message/contact another if needed. This way sessions can coordinate and work in harmony, avoiding for example the overwriting of each other's work.
* Usage indicators and viewers for remote providers (like Anthropic or OpenRouter).
* Sessions grouping and searching. Sessions should be automatically named and tags should be assigned to them, to allow for better searching. Lastly the search should offer a toggle that is off by default, that allows searching the contents of sessions too (local ones), which is slower (user gets told that it is slower and it is suggested to keep it off unless really necessary).
* Complex voice system
   * Normal voice dictation. Clicking on microphone icon in the chat starts recording and user talking gets written in the chat, corrected and normalized (meaning it eliminates the talking things like ehm... or repeating words, or the user correcting themselves).
   * Voices browser, as well as a voice cloning window/page/section that allows users to record themselves or load audio files that get cleaned (noise and music removed) and then used to create a new voice that then can be named and assigned an image and description. User can chose the voice they want Demido to have.
   * TTS: mesages can be read by the voice users chose.
   * Voice mode. There should be a button that allow users to conversate with the model in a human manner, just by speaking. User speaks, model speaks back. If the task would take time, model should always tell the user to wait, before actually doing things, otherwise user gets no feedback to what they just said.
* Character system:
   * Custom Name, Picture and Description
   * Custom system prompt.
   * Possibility to assign model and set custom model settings
* File explorer, viewer and editor and Artifact system.
   * Model can create artifacts, files that get sent in the session by the model, that can be updated, user can see its previous versions, and can also be downloaded. Claude Desktop has this feature.
   * Possibility to view the files in the folder of the current session
   * Viewer for media like PDFs, pictures, videos and music.
   * Editor with automatic syntax highlighting, and previewing for Markdown and HTML files, with side by side support too
* Browsing features:
   * In-app browser with tabs
      * Model can interact with the browser:
         * click elements, or points on the page as a fallback
         * take screenshots
         * run scripts
         * inspect network
         * read console
         * open tabs
         * basically everything a normal user would be able to do
   * web search
      * a3-search, SoSearch, DonSeTch, duckduckgo, metadata-search-engine-rs, RustXNG (direct, no api), with spider fallback, or any combination of these to get the most results, ranked and merged.
      * fallback 1:: using free and key-less services: for those who allow higher limits or the likes via a key, allow the users to input their key if they own one
      * fallback 2: scraping
      * fallback 3: using integrated browser. It is also useful for search engines that require typescript
   * web fetch: normal fetching, but uses integrated browser when the website requires javascript
   * data aggregators, there are some useful data aggregators and fetchers for many things, that would be better to use instead of fetching from scratch.
* Local server support with support for the main web technologies: models should be able to start a local server and work with modern web technologies in an integrated manner.
* Scheduling and tasks support: I should be able to ask Demido to give me my email report every morning at 7 o'clock, or ask it to monitor the price of a certain sweater and tell me when it goes on sale, without the model having to be loaded all the time. Unloading should happen automatically when model is not needed (this is a recurring thing in this project, which will mostly be used to load local models).
* Email system:
   * Preset for Google
   * Support for IMAP, POP3 (not a hard requirement) and STMP.
   * Optimized browsing and reading.
   * Support for loading images and styles.
   * Multiple email accoutns support
   * LLMs operate on emails based on permissions
* Whatsapp support
* Telegram support via bot or userbot logging
* Discord support
* Markets support with charts, real-time data, history downloading, backtesting, etc., using something like https://github.com/Mathieu2301/Tradingview-API and in-app tradingview logging that captures cookies.
* In-app updates with Automatic updates support.

To allow multiple things to happen "at the same time", Demido should intelligently handle model unloading. For example if the model is being used by a session to do something, and a scheduled task is going to happen, the session should temporarily borrow the model to the schedule, or unload it to leave space for the schedule's model, and go back to whatever it was doing after eventually load back its model once the scheduled task is finished. Build a brand kit before building the UI so it stays consistent. I have made other demido studio versions in the past but they are all bad and I don't want you to get corrupted by them, so you should not look into them for any reason. You should publish the project on github on a new demido-studio repo, after renaming the current one to demido-studio-third.

Follow-up clarifications:

* License: GPL-3.0-or-later.
* By OpenCode I mean the local CLI tool, which offers free models.
* By Anthropic subscription I also mean Claude Code, just like OpenCode.
* Mobile later down the road.
* I want an interactive preview with fake data before the actual work begins.
* I also want Demido Studio to work via cli and have a server mode, so that I can use it from anywhere, even if it's on a vps. Its UI therefore should also work from a web browser, and there should also be an API for when I want to integrate it in other things or just use it as a model manager.
* If Claude Code finds ideas, suggestions or better ideas, it should discuss them with me, never holding back.
* There should be automatic building on GitHub tag/release, just like the current one (do not look at the current one).
* Claude Code decides the technical choices (stack, architecture, structure, plan) itself.

---

## Added by the owner after the brief

Said in conversation, kept here in the owner's words so that the brief stays the whole of what was asked.

**2026-11-21, caching.** "Also a thing I forgot to tell you in the brief is to use caching whenever possible
in order to do as few requests as possible and to make Demido Studio faster."
How it is applied: `docs/DECISIONS.md`, section P.

**2026-11-21, a model for each agent.** "Another thing I forgot to mention is that I want models to know
about the other available models on the system and be able to assign a model per agent, for orchestration
and doing better work, and quicker where less good models are ok or work is mechanical"
It extends section 4.17 (agents and resource-aware model juggling). How it is to be built:
`docs/DECISIONS.md`, section O.

**2026-11-21, vector reasoning is withdrawn.** Sections 3 and 4.3 name "vector reasoning instead of language
reasoning" as a technique to research. I reported that it could mean three things, and that the third,
"Making a downloaded model reason in vectors and not in words", is not possible because it needs models
trained for it. The owner answered, quoting that line: "Forget about the vector thing then...I meant this"
So that example is no longer part of what is asked. Recorded in `docs/DECISIONS.md` L10.

**2026-11-21, Balanced and programs.** Asked whether the Balanced level should run a program without asking
when the program is shut in a sandbox and the folder was saved first, as it then did: "Always ask first."
It settles how section 4.16's "asks only for things that could cause damage" applies to running programs.
Recorded in `docs/DECISIONS.md` T3.
