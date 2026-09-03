# Brief

Stefan's original brief for Demido Studio, copied verbatim on 2026-09-03.

**Never summarise this file.** Never resolve a ticket from a summary of it. Every wayfinder ticket
cites this file by heading or quoted line. If a decision contradicts the brief, the brief wins until
Stefan says otherwise in writing, and that change is recorded as a resolution comment on a ticket.

---

First of all check all the skills, plugins, and tools you have access to, understand what they do, and require their usage at useful moments throught the plan.

I need a custom-made LLM harness tailored to my needs.\
The issue I run into most often with other harnesses is that tools are poorly integrated or missing alltogether. Too often I have to go around looking for MCPs, Skills, Plugins, then manually install and configure them one by one. I need to create apps on developer dashboards, provide API keys, etc.\
What i currently need is a harness that integrates all the tools I need seamlessly and in a way that at least feels native.\
It will be windows-only at first, during the pre-release versions, but I will need cross-platform close to 1.0 release.

Another of the goals is allowing people to get the most out of even smaller models, which is something I haven't really seen done by other harnesses. By properly "guiding" the llms, in a smart way that also does not consume too much context, it is possible to make even smaller models (like Qwen3.5 9B, Gemma 4 26B A4B and even Gemma 4 E4B it, all models which I will personally use during testing phase) behave "properly". I can already think of many ways of achieving this myself, like intelligently guardrailing LLMs against obscure errors and mistakes that might happen during development, implementing a system that teaches the LLMs how to handle an error after they have encounter one, and have dealt with it. For example: model tries to use the Powershell 5.1 command `Test-Connection -ComputerName Server01 -Count 4 -AsJob -Authentication Default -TimeToLive 128`, but user has upgraded to Powershell 7.x so the correct command would be `Test-Connection -TargetName Server01 -Count 4 -TraceRoute`; either the model finds out the issue itself, checks the help commands, manuals and what not and gets to the Powershell 7.x command, or the user tells it where it is wrong; in either case the model should now remember this kind of issue and how to solve it (IMPORTANT: NOT this specific issue, but this specific KIND of issue. In this case it shouldn't remember how to solve this single Test-Connection command issue, but if a similar problem happens, say, when trying to run an updated or outdated version of the mpv video player, or while configuring hyprland which always has breaking updates, it now knows how to solve those too).

I also want to provide transparency to the users. 

First way is: everything about what happens, from the input getting sent to the model using the tools available up until the reply (and after, but we'll get to this later), should be visible. All prompts should be editable. Requests monitorable. 
I really like the logging/monitoring system that Deepseek did in their harness. From their own website for the harness:
> **Every run is traceable**\
Everything the model sees is recorded in an append-only session log: system prompts, reasoning, tool calls and results, subagent scheduling, and every context injection. In the Trajectory view, you can inspect these records by source. Resume, fork, search, and replay all operate on the same event stream.
I am not a great fan of the UI Deepseek did for their session log, but I do really love the idea of allowing users to properly see what is going on so that they can debug their own workflows, skills and what not. Of coures it would also be really useful for us during development. Another thing it could be used for is reverting back the session and files to a previous status, since also the write actions should be logged.

Another way of providing transparency is allowing users to change the model's settings (like context length, temperature, etc.) both at a model/global level as well as at a per-chat level (and a per-character level, since I will need a character system implemented later down the road so users can give LLMs personalities and ways of doing things).

The codebase should be modular and simple by design. No over-engineering when it is not needed. Parts of code should be swappable, deletable or additionable quickly, without issue and especially without having to touch lots of files. I want a codebase that feels like a server rack, a NAS or a drop-ceiling. In a NAS you can remove a drive and everything works fine, you can add one, and everything still works just as fine, you can even switch the disks and everything still works normally. In a rack you can add, remove and replace entire machines and everything still works. Same as drop-ceilings, you remove a broken tile, replace it with a new one...the ceiling didn't change, only the tile did.
I basically need a codebase similar to UNIX, package managers or kernel modules....or even React in a way, due to its components-based nature.

Here are the features I need:
- Guided set-up on first launch (GPU & GPU Ecosystem (CUDA, ROCm, etc.) selector, Runtime & Dependencies installation, etc.)
- Multi-account system, allowing different users to use the software on the same machine (think families, friends in college rooms, etc.)
- Two main chat modes: Chat and Agent. There should be 3 agent modes: Cautious, Balanced, Autonomous.
- Deep integration of Caveman-like system (https://github.com/juliusbrussee/caveman). UI selector with Off, Lite, Full, Ultra, Wenyan Lite, Wenyan Full and Wenyan Ultra, as well as a toggle + selector (on by default in all sessions unless user toggles it off in any session) that enables or disables caveman during model's thinking process and also allows to select a specific level of caveman for the thinking (Ultra by default). 
- Tool calling and custom tools like run_command, read_file, write_file, delete_file, list_dir, etc.
- Markdown, LaTeX and Mermaid supported in the chat bubbles. More (like charts, pictures, etc.) down the road.
- Projects system that allows to group chats related to the same project. Projects should be each an expandable element in the chat list, and chats should be draggable into them. UI should have a tag-like button system at the top to easily filter between All, Chats, Projects. Users should also be able to connect a project to files or entire folders, so that models can use them for reference and other purposes. Of course projects should have a name, short description, and also user should be given an icon picker so that they can assign an icon (even custom png, svg...) to the project.
- Native web search and fetch that also supports javascript-requiring/javascript-based websites.  
- Configurable Agents & sub-agents system. Models should be able to delegate an agent to do a specific task in a separate clean context, for two main reasons: not filling up context with useless tool call outputs and such things, and so that it can be parallelized (by people whose systems can afford the parallelization). Each agent should contain a list of suggested models to be used by. For example a code reviewer agent might have models like GLM 5.2 or Kimi K3 suggested, while a document writer agent might have models like Qwen 3.5 26B suggested, due to not needing to be extremely smart. User should be able to assign their models to the different agents. Also user should be able to manually set the amount of parallel agents they want to run at the same time, and how deep agents can delegate one another (E.g: depth 3 would mean Main chat/context delegates an agent we will call Agent 1. Agent 1 needs another info so it delegates Agent 2. Agent 2 needs something else so it delegates Agent 3.)
Agents are not only for parallelization, but also for keeping context clean, even more than RTK could do. Complex tasks get delegated by the main session to a sub-agent, then it waits for sub-agent's answer. This is good too, when parallelization is not an option due to low amount of resources.
- Skills system. This is important because my requirements for it are complex. Skills should work just like skills in other harnesses, via MD files. However, in Demido Studio they should also have a .json file containing tools and if they can be used as agents/sub-agents and when to do so. For example a Market Data Analyst skill might have a skill.json file containing:
```json
{
  "id": "market",
  "name": "Market Data",
  "description": "Market data tools for the model: live prices, quotes, historical OHLCV candles and charts across FX, stocks, indices, commodities and crypto. Prices come from the user's own connected MetaTrader 5 accounts (everything except crypto) and from crypto exchanges through CCXT (keyless). Use for any price, quote, chart, trend, indicator, pattern, backtest or technical-analysis question, and for downloading deep historical data from archives. Owns the market_quote, market_history, market_search, market_sources, market_download_estimate and market_download tools. Read this skill's SKILL.md before downloading history and whenever the user has more than one broker account: the download-size policy (estimate first, auto if quick and small, ask as size grows) and the rules for picking which account answers both live there.",
  "commands": [
    {
        "name": "market_report",
        "description": "Create market report for asset, timeframe and duration requested by user",
        "file": "commands/market_report.md",
        "params": [
            {
                "name": "asset",
                "description": "Market asset. Can be FOREX, Stock, Crypto, Future, Commodity, etc. E.g: \"EUR/USD\"",
                "required": true
            },
            {
                "name": "timeframe",
                "description": "Time frime on which analysis should be made (\"15m\", \"4h\", \"1d\" (or \"d\"), \"1w\", \"1y\").",
                "required": false
            },
            {
                "name": "duration",
                "description": "Duration on which to make analysis. E.g: \"since last January\" or \"from 2nd February 2024 to 8th April 2025\"",
                "required": true
            },
        ]
    }
  ],
  "agent": {
    "tools": [
        { 
            "type": "mcp",
            "name": "market_quote", 
            "description": "Current price snapshots for FX, stocks, indices, commodities, crypto" 
        },
        {
            "type": "mcp",
            "name": "market_history", 
            "description": "Historical OHLCV candles (CSV) for charting and analysis"
        },
        {
            "type": "mcp",
            "name": "market_search", 
            "description": "Find the ticker symbol for an instrument, and which account or exchange carries it"
        },
        {
            "type": "mcp",
            "name": "market_sources",
            "description": "List the connected broker accounts and crypto exchanges a chart can be read from"
        },
        {
            "type": "mcp",
            "name": "market_download_estimate",
            "description": "Size a deep-history archive download before fetching it"
        },
        {
            "name": "market_download",
            "description": "Download deep historical candles from archives into the cache"
        },
        {
            "type": "prompt",
            "name": "market_documentation",
            "description": "Create stylish documentation of market data or situation user asked about",
            "file": "tools/prompt/market_documentation.md"
        }
    ]
    "models": "Qwen 3.5 9B, Gemma 4 26B A4B, GPT OSS 20B",
    "when": "the work needs several rounds of raw price data whose CSV you do not need to keep: more than one instrument, more than one timeframe, a scan across a watchlist, or a history download followed by analysis. A single quote or one short candle series is faster done inline."
  }
}
```
You might have noticed that there is a tools array. That is because I also need skills to be able to provide custom tools that LLMs can use. In fact skills might (and if the tools are defined it more than probably does) also provide an MCP entry via an mcp.json file like this:
```json
"mcp": [
    {
        "github": {
            "type": "http",
            "url": "https://api.githubcopilot.com/mcp/"
        }
    },
    {
        "vercel": {
            "type": "stdio",
            "command": "npx",
            "args": ["mcp-remote", "https://mcp.vercel.com"]
        }
    }
]
```
Having these 2 json files allows for a really smart behavior: The file that gets actually loaded (if skill is enabled) is the skill.json file, which contains short descriptions of what the skill does, and when it should be used as an agent. This allows to simply not load the skill if unneccessary, even if it is enabled in the UI by the user. This solves problems caused by undisciplined or not-knowledgeable-enough users.\
There are also the slash commands, that allow users to use features from the skill on demand. It basically loads prompts when requested by the user. The "when" condition applies to these too.\
Some of these skill features have been already implemented by other harnesses, but some are Demido-specific. This is a big part of the very core of what will make models running in Demido Studio feel "smarter" and more capable than those in other harnesses.\
Initially I will be the only one providing skills, but even when there will be multiple people providing them, I still think it will not be a problem since skills also provide the required mcps and tools instead of the user having to grab them manually. \
As you might have noticed there are different types of tools. The ones without a specific type are assumed to be the ones provided by the skill in an "engine" or "src" folder (where the actual source code that makes the tools work resides). I don't know what language and proper system is best for this kind of thing, but I trust you to make a good choice.\
The prompt type means that the tool is nothing more than an md file that gets read and tells LLM what to do on the spot. The mcp type means that is simply a tool provided by the MCP. If you are asking why are we not loading it directly from the mcp itself (it already has name and description), it is because the skill might only require some tools, not all, and the skill creator might need to change the tool description to better fit in their skill.
(Regarding skills, I am also fine with some custom extension-like system, if it is better.)
- Artifact system. LLMs should be able to create Claude Code/Z-AI-like "temporary" files directly inside the message bubble itself. Clicking the artifact in the bubble should open a sidebar showcasing its contents. ![Artifact System Reference Image](C:\Users\elpid\Desktop\demido-studio-planning\references\ClaudeArtifactSystem.png)
- Models Browser & Downloader ![Models Browser Reference Image](C:\Users\elpid\Desktop\demido-studio-planning\references\LMStudioModelSelector.png)
UI should also have a download status indicator which also allows to pause, resume or cancel model downloads (especially useful when adding multiple models to the download queue). Also a download folder should be set-able by the user. Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks.
- LLM capabilities support. Vision, Thinking toggling, etc., and everything mmproj-file-related.
- Task-model functionality. User should be able to set a model (the conversation one by default) that does simple tasks around Demido Studio, like renaming conversation every N messages based on context, and other such things.
- Accounts management system for things like:
    - Google Account OAuth for things like Email, Calendar and Contacts integration.
    - Market Data accounts (Bybit, Binance, Kukoin, TradingView, etc.)
    - Any other accounts added in the future.
- Free models system via OmniRoute/9Route-like system. I want the system to be called Nexus. It should be Demido Studio's own free models router system so that anyone can use Demido Studio for free without having to download a model.
You can find the codebases of other AI Router projects in S:\Development\routers. Use it for reference and to understand how to implement our own.
- Native graphify or graphify-like implementation, that also provides tools to LLMs when enabled on a project so that they can properly navigat the codebases via graph and also update the graph when needed. graphify-out folder should be automatically be added to .gitignore by demido studio (if it is not already) when it triggers graphify to create it. On projects where it has not been initialized yet a simple button or "Build graph" CTA would be enough.
- Email, Calendar and Contacts implementation + tools provided to LLMs to use them (with multi-account support). I need a way to read and send emails, view and create events in the calendar, as well as browse my contacts, edit them and delete them, directly inside Demido Studio, in one place. I have multiple Google accounts I use, and in the future I will also need other services implementation (like simple SMTP, Zoom, etc.). LLMs should use the default account (the first one added or configured from the settings for each of the services), unless user doesn't explicitly specify another. 
- Charts visualizer (via TradingView or some chart library). I want it to look good, but especially be reliable and show data correctly. No need for any tools like drawing tools (lines, arrows, text, etc) or any indicators (VWAP, MACD, Volumes, etc.) needed initially. I will implement those in the later future. For now I just need a simple candle chart to see what the market is doing at different timeframes. No market operations initially, just market data for analysis, tips and suggestions by the LLM. Maybe https://github.com/Mathieu2301/Tradingview-API would be a good idea here? Connection should happen from the Accounts Management system. A window should pop up with the TradingView website on the log in page. Demido Studio should check if user has logged in and capture the cookies Tradingview-API needs.
- Chat export system. Each session should be exportable as a JSON, CSV, or YML. If you think other formats would be nice to have then please go on. It should contain everything, chat, tool calls and results, context, skills, EVERYTHING. It should probably be based on the Session Monitor (the one inspired by the Deepseek's one I talked about earlier). Now that I think of it, the Session monitor should also have an export button. This system will be especially useful during development so you know what to look into exactly when something does not work.
- Full Keyboard navigation with cool key-shaped shortcuts indicators shown across the UI. Do not overdo it, only show them where it makes sense and it looks cool. It is okay to show them on hover so user learns it after hovering, there is no issue with that. Also I want all shortcuts to be customizable. Vivaldi browser does this very well and I love it.
- Mac Spotlight-like command panel. It should open via F1 by default and user should be able to do basically everything from there. Of course it should be curated and take into consideration typos and such things. It should allow user to navigate to chats, find specific settings by searching them and pressing enter -> the proper window opens, and smoothly scrolls to the setting if needed, then the settings gets highlighted temporarily (android style), and even find the files they have added as reference to projects.
- Integrated basic Web Browser that both users and LLMs have access to. Mostly for LLMs, so that they can test the websites they make as such, but also for users so that they can visit websites without leaving Demido Studio.

Regarding to the UI, it should be simple, clean, dark and minimal. The main structure should be based on Bento Box UI & Islands UI concepts (the same ones used by VSCode and Jetbrains' Fleet. Shaping/Structure Style References: JetbrainsFleetRandomScreenshot1.webp, VisualStudioCodeRandomScreenshot1.png, BentoShowcase1.webp and BentoShowcase2.png in C:\Users\elpid\Desktop\demido-studio-planning\references).\
UI should be extremely curated. A level of curation and attention to details on par with APPLE Design Team.\
Use a pastel-colors palette for a more polished feel.

Use the most modern libraries and technologies for the frontend and for frontend components, so that you obtain an updated-looking modern piece of software. Shadcn, Radix, Tailwin, all valid solutions, but keep in mind to not mix-match systems for no reason.
Keep arbitrary values like colors localized, do not go sprinkling hex values across the codebase.

There should be a splash small window (similar to that of Discord), showing the logo, the Demido Studio name and the loading status (maybe even what it is actually loading?).

The only thing I would really like about the UI would be a VSCode-like Icons-only sidebar that can be moved on the left or on the right side of the program. You can put whatever you feel its best in it, but I think navigation would be the best idea (Chats, File Explorer, Code Graph Window, Market Charts Window, Session Monitor Window, Sub-Agents monitoring window, Settings (maybe in the other corner, like if the others are at the top, settings should be at the bottom...)). users should be able to move these icons around by right clicking an empty space or the settings button and selecting Edit Navbar from the context menu.

Some panels should be movable by using a modifier key (Alt by default, user-editable from the keybind-editing system). Panels should snap to positions and automatically resize, to fit what Windows does. The panels system should be inspired by Hyprland and i3 tiling systems on linux. 

This is not definitive. If there are wrong decisions I have taken here, if there are better ways of doing anything or for whatever other reason, just tell me.

The entire development process should be completely automatic, you should test the project yourself (either by using tauri mcp + plugin, or/and agent-browser cli tool on this machine), understand problems and issues, and keep working until you have a completely working system that is comparable, if not better than other harnesses. 

You can find my older failed trials at making it myself in `S:\Development\Demido Studio Project\demido-studio-first-version` and `S:\Development\Demido Studio Project\demido-studio-second-version`. You can also find the source code for other harnesses in `S:\Development\Demido Studio Project\harnesses`. Use their codebases to understand the best ways of doing things or even to port things when the license allows.

Demido Studio will be released under GPL v3, so build it in a way that does not go against that. Furthermore all technologies, frameworks and libraries used will need to be credited both in the source code and inside the program itself.
A licenses folder will also be needed:
```
licenses
    ggml-org
        llama.cpp
            LICENSE
```
so that all licenses are provided.

This is the planning session so please ask as much as you want about the project.

Commits should al be signed using my GPG key, or SSH key (should be already configured). You (Claude Code) should not be listed as a Contributor/Co-Author in git, as I (Stefan Cucoranu, elpideus@gmail.com) should be the sole contributor.
You can create the repo on github as it does not already exist. 

I might decide or change things in the future (this is why code should be modular)
Repo name should be `demido-studio`.
