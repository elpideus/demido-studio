# The brief, row by row

Every requirement in [`docs/brief.md`](brief.md), what has been decided about
it, what has been built, and whether a real model has ever driven it.

Checked by `scripts/check-rules.mjs`. See
[`docs/rules/brief.md`](rules/brief.md) for what CI enforces and what it cannot.

Decided on wayfinder ticket
[#12](https://github.com/elpideus/demido-studio/issues/12).

## How to read it

**Id.** Assigned once, from the next free number, and never reused or
renumbered, so a citation written today still resolves in a year. The table is
ordered by the **brief**, not by id: a requirement noticed later takes the next
free id and sits in its brief position, which is why `B49` appears between `B27`
and `B28`. A requirement that dies keeps its row and its id, struck through.

**Brief says.** A verbatim fragment of `brief.md`, long enough to find the line
and no longer. CI matches it against the brief character for character, so a row
cannot drift from the thing it claims to track.

**Decided.** The ticket that settled how this requirement is met, or `-`.

**Built.** Where the code lives, or `-`. Everything is `-` today: v3 has no code
yet.

**Live.** A link to the Definition-of-Done evidence
([`docs/rules/done.md`](rules/done.md): a live-model scenario and a screenshot of
the running window), or `-`. `n/a` where no model can exercise the requirement,
such as the licensing and attribution rows.

Three columns rather than one because they are three different failures, and v2
hit two of them. Milestones 10 and 11 were **decided but never checked against
the brief**. Twelve features were **built, unit-tested and never once driven**,
and the first live run found a bug in the newest of them within one question. A
single "covered" column hides exactly the distinction that set v2 aside.

## The ledger

| Id | Brief says | Decided | Built | Live |
|---|---|---|---|---|
| B01 | "check all the skills, plugins, and tools you have access to" | [#1](https://github.com/elpideus/demido-studio/issues/1) | - | n/a |
| B56 | "I need to create apps on developer dashboards, provide API keys, etc." | [#17](https://github.com/elpideus/demido-studio/issues/17) | - | - |
| B02 | "a harness that integrates all the tools I need seamlessly" | [#17](https://github.com/elpideus/demido-studio/issues/17) | - | - |
| B03 | "windows-only at first, during the pre-release versions" | - | - | - |
| B04 | "get the most out of even smaller models" | [#13](https://github.com/elpideus/demido-studio/issues/13), [#22](https://github.com/elpideus/demido-studio/issues/22) | - | - |
| B58 | "or the user tells it where it is wrong" | [#13](https://github.com/elpideus/demido-studio/issues/13), [#23](https://github.com/elpideus/demido-studio/issues/23) | - | - |
| B05 | "NOT this specific issue, but this specific KIND of issue" | [#13](https://github.com/elpideus/demido-studio/issues/13), [#22](https://github.com/elpideus/demido-studio/issues/22) | - | - |
| B06 | "All prompts should be editable. Requests monitorable." | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B07 | "recorded in an append-only session log" | [#8](https://github.com/elpideus/demido-studio/issues/8), [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B08 | "reverting back the session and files to a previous status" | - | - | - |
| B09 | "both at a model/global level as well as at a per-chat level" | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B10 | "The codebase should be modular and simple by design." | [#10](https://github.com/elpideus/demido-studio/issues/10) | - | - |
| B11 | "Guided set-up on first launch" | [#21](https://github.com/elpideus/demido-studio/issues/21), [#27](https://github.com/elpideus/demido-studio/issues/27), [#28](https://github.com/elpideus/demido-studio/issues/28), [#29](https://github.com/elpideus/demido-studio/issues/29) | - | - |
| B12 | "Multi-account system" | [#15](https://github.com/elpideus/demido-studio/issues/15) | - | - |
| B13 | "Two main chat modes: Chat and Agent." | [#20](https://github.com/elpideus/demido-studio/issues/20) | - | - |
| B57 | "There should be 3 agent modes: Cautious, Balanced, Autonomous." | [#20](https://github.com/elpideus/demido-studio/issues/20) | - | - |
| B14 | "Deep integration of Caveman-like system" | - | - | - |
| B15 | "Tool calling and custom tools" | [#14](https://github.com/elpideus/demido-studio/issues/14), [#20](https://github.com/elpideus/demido-studio/issues/20) | - | - |
| B16 | "Markdown, LaTeX and Mermaid supported in the chat bubbles." | - | - | - |
| B17 | "Projects system that allows to group chats" | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B18 | "Native web search and fetch" | - | - | - |
| B19 | "Configurable Agents & sub-agents system." | [#8](https://github.com/elpideus/demido-studio/issues/8), [#11](https://github.com/elpideus/demido-studio/issues/11), [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B20 | "Skills system. This is important because my requirements for it are complex." | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B50 | "I also need skills to be able to provide custom tools that LLMs can use" | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B51 | "The file that gets actually loaded (if skill is enabled) is the skill.json file" | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B59 | "There are also the slash commands, that allow users to use features from the skill on demand" | [#25](https://github.com/elpideus/demido-studio/issues/25) | - | - |
| B60 | "The "when" condition applies to these too." | [#14](https://github.com/elpideus/demido-studio/issues/14), [#25](https://github.com/elpideus/demido-studio/issues/25) | - | - |
| B52 | "skills also provide the required mcps and tools instead of the user having to grab them manually" | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B53 | "The ones without a specific type are assumed to be the ones provided by the skill in an "engine" or "src" folder" | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B54 | "The prompt type means that the tool is nothing more than an md file that gets read and tells LLM what to do on the spot." | [#14](https://github.com/elpideus/demido-studio/issues/14) | - | - |
| B21 | "Artifact system." | [#7](https://github.com/elpideus/demido-studio/issues/7), [#10](https://github.com/elpideus/demido-studio/issues/10) | - | - |
| B22 | "Models Browser & Downloader" | [#7](https://github.com/elpideus/demido-studio/issues/7) | - | - |
| B55 | "Multiple folders should be set-able for model detection" | [#15](https://github.com/elpideus/demido-studio/issues/15) | - | - |
| B23 | "LLM capabilities support. Vision, Thinking toggling" | - | - | - |
| B24 | "Task-model functionality." | [#24](https://github.com/elpideus/demido-studio/issues/24) | - | - |
| B25 | "Accounts management system" | [#5](https://github.com/elpideus/demido-studio/issues/5), [#17](https://github.com/elpideus/demido-studio/issues/17), [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B26 | "Google Account OAuth" | [#3](https://github.com/elpideus/demido-studio/issues/3), [#5](https://github.com/elpideus/demido-studio/issues/5), [#17](https://github.com/elpideus/demido-studio/issues/17), [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B27 | "Market Data accounts" | - | - | - |
| B49 | "Any other accounts added in the future." | [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B28 | "I want the system to be called Nexus." | [#4](https://github.com/elpideus/demido-studio/issues/4), [#18](https://github.com/elpideus/demido-studio/issues/18), [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B29 | "Native graphify or graphify-like implementation" | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B30 | "Email, Calendar and Contacts implementation" | [#3](https://github.com/elpideus/demido-studio/issues/3), [#17](https://github.com/elpideus/demido-studio/issues/17), [#26](https://github.com/elpideus/demido-studio/issues/26) | - | - |
| B31 | "Charts visualizer" | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B32 | "Chat export system." | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B33 | "Full Keyboard navigation" | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B34 | "Mac Spotlight-like command panel." | [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B35 | "Integrated basic Web Browser" | [#10](https://github.com/elpideus/demido-studio/issues/10), [#28](https://github.com/elpideus/demido-studio/issues/28) | - | - |
| B36 | "it should be simple, clean, dark and minimal" | [#7](https://github.com/elpideus/demido-studio/issues/7), [#9](https://github.com/elpideus/demido-studio/issues/9) | - | - |
| B37 | "on par with APPLE Design Team" | [#9](https://github.com/elpideus/demido-studio/issues/9) | - | - |
| B38 | "Use a pastel-colors palette for a more polished feel." | [#6](https://github.com/elpideus/demido-studio/issues/6) | - | - |
| B39 | "keep in mind to not mix-match systems for no reason" | [#10](https://github.com/elpideus/demido-studio/issues/10) | - | - |
| B40 | "Keep arbitrary values like colors localized" | [#9](https://github.com/elpideus/demido-studio/issues/9) | - | - |
| B41 | "There should be a splash small window" | [#6](https://github.com/elpideus/demido-studio/issues/6) | - | - |
| B42 | "a VSCode-like Icons-only sidebar" | [#7](https://github.com/elpideus/demido-studio/issues/7), [#8](https://github.com/elpideus/demido-studio/issues/8) | - | - |
| B43 | "Some panels should be movable by using a modifier key" | [#7](https://github.com/elpideus/demido-studio/issues/7), [#10](https://github.com/elpideus/demido-studio/issues/10) | - | - |
| B44 | "you should test the project yourself" | [#11](https://github.com/elpideus/demido-studio/issues/11) | - | n/a |
| B45 | "credited both in the source code and inside the program itself" | [#16](https://github.com/elpideus/demido-studio/issues/16), [#27](https://github.com/elpideus/demido-studio/issues/27) | - | n/a |
| B46 | "A licenses folder will also be needed" | [#16](https://github.com/elpideus/demido-studio/issues/16), [#27](https://github.com/elpideus/demido-studio/issues/27), [#28](https://github.com/elpideus/demido-studio/issues/28) | [`licenses/`](../licenses/README.md) | n/a |
| B47 | "should not be listed as a Contributor/Co-Author in git" | - | - | n/a |
| B48 | "Repo name should be" | [#1](https://github.com/elpideus/demido-studio/issues/1) | - | n/a |

## Amendments

The brief wins until Stefan says otherwise in writing. Eight lines have been
overruled so far, each on a ticket, each with the reasoning on that ticket.
`brief.md` itself is never edited: its verbatim guarantee is the whole value of
the file. This section is how a session reading the brief learns which lines no
longer bind, and CI checks that each original below still matches `brief.md`
character for character.

| Id | The brief said | Overruled by | What binds instead |
|---|---|---|---|
| B38 | "Use a pastel-colors palette for a more polished feel." | [#6](https://github.com/elpideus/demido-studio/issues/6) | **Karl**, Fluent 2 dark surfaces carrying v2's accents. Pastel lost on chroma, not contrast: 0.066 of separation from neutral ink against the saturated green's 0.182, and sRGB refuses more than about 0.08 at pastel lightness. |
| B42 | "Sub-Agents monitoring window" | [#8](https://github.com/elpideus/demido-studio/issues/8) | Sub-agents are a scope on the session log, not a window, so the rail loses that entry. |
| B43 | "The panels system should be inspired by Hyprland and i3 tiling systems on linux." | [#7](https://github.com/elpideus/demido-studio/issues/7) | Windows 11 and KDE behaviour: floating covers, pinned makes the desk give up exactly its width, no pin button and no minimise. |
| B35 | "Integrated basic Web Browser that both users and LLMs have access to" | [#10](https://github.com/elpideus/demido-studio/issues/10) | One panel over two engines: the user on WebView2, the model on Chromium over CDP, because WebView2 has no isolated worlds. |
| B12 | "allowing different users to use the software on the same machine" | [#15](https://github.com/elpideus/demido-studio/issues/15) | A Demido profile is a Windows profile. The separation is the operating system's, not Demido's: a second person makes a second Windows user. Two people at one Windows login share one profile entirely, and Demido neither detects nor warns about that. |
| B02 | "a harness that integrates all the tools I need seamlessly" | [#17](https://github.com/elpideus/demido-studio/issues/17) | Two audiences, not one promise: no setup on the common path, and every credential visible and replaceable underneath. Google mail is one click through an app password, but Google calendar and contacts cost a guided ten minutes in the Cloud console, because Demido ships no OAuth client of its own and verification needs a domain it does not have. |
| B13 | "Two main chat modes: Chat and Agent." | [#20](https://github.com/elpideus/demido-studio/issues/20) | One loop, always the agent loop, the shape Claude Code has. A conversation that calls no tool is the same loop with nothing to do but answer. Everything the Chat mode would have done is now a set with nothing in it on the offered axis, reached by a control that can also say "files but no shell", which no mode could express. |
| B60 | "The "when" condition applies to these too." | [#14](https://github.com/elpideus/demido-studio/issues/14) | Nothing evaluates `when` on a slash command, because the user chose it. It is help text on the command and is documented as help text. The clause still binds the skill itself: `when` at the top level says when to read the skill, and `agent.when` says when to delegate to it. |
