# Ideas and pushback

A running log. Small, obvious improvements get done and noted. Larger ones wait for the owner.
Status: **Done** (built), **Proposed** (built as a proposal or waiting for a yes), **Raised** (not built).

## 2026-11-20, brand kit

| # | Idea | Status |
|---|---|---|
| 1 | **Eyes as the sign for models.** You called the logo's dots "the eyes". Open eye = loaded (awake), outline = unloaded (asleep), turning = loading, breathing = generating. A model that finishes loading opens its eye with the same move as the logo. | Proposed, in the kit |
| 2 | **A tray icon that tells the state.** Eyes open when a model is in memory, closed when none is. You can see at a glance whether your GPU is in use without opening the app. | Proposed, in the kit |
| 3 | **Four more task states** than your reference: with an agent, needs you, failed, skipped. The task list needs them once agents and permissions exist. | Proposed, in the kit |
| 4 | **"Quiet until it matters."** The whole interface is neutral; colour appears only as a signal. Borrowed from the dark-cockpit rule in aircraft, to match your aircraft-manual answer style. It also lowers visual noise, which helps the attention goal of brief 4.14. | Done, core principle |
| 5 | **Three signal words** used exactly (NOTE, CAUTION, WARNING), in answers and in the interface, as manuals do. Gives small models a fixed, short vocabulary for risk. | Done |
| 6 | **A Tool has two texts.** A description for people (complete, normal language) and guidance for models (caveman). The brief's caveman rule covers "tool guidance and descriptions"; descriptions shown in the Tools browser are read by people, so they should not be caveman. Needs your confirmation. | Raised |
| 7 | **Changed settings are marked.** With hundreds of tweakable settings, "Changed · Reset" on each row keeps drift from defaults visible. Follows "nothing hidden". | Done in the kit |
| 8 | **Every setting row has a Details line** with default, range, cost and trade-off. This is how "simple for beginners, everything for power users" works without a separate advanced mode. | Done in the kit |
| 9 | **"Not built yet" marker** as a standard component, so the no-silent-stubs rule is enforced by design from the preview on. | Done |
| 10 | **Untrusted-content marker.** Text from the web, email, files or other sessions is visibly labelled "treated as data". Makes the injection defence visible, not just internal. | Done in the kit |
| 11 | **Compressed output always says what it left out** ("42 of 1,180 lines") with the full output one press away. The visible half of brief 4.18. | Done in the kit |
| 12 | **Permission requests show the fact before the claim.** The exact command first, the model's reason second. A model's stated reason can be wrong or injected; the command cannot. | Done in the kit |
| 13 | **Memory budget as one labelled bar** (model, agents, embedding, kept free, free). Makes brief 4.17's juggling visible: you can see why a second agent does or does not fit. | Done in the kit |
| 14 | **The brand CSS as a contract for model-made elements.** If small models are told "use class `d-card`, `d-badge`, `data-state`", their on-the-spot elements look native with very few tokens of instruction. Worth testing when 4.5 is built. | Raised |
| 15 | **User-selectable accent**, like macOS. "Everything tweakable" suggests it. | Done: you asked for it. Built as a setting (Automatic, Demido green, Custom) with any colour allowed; a collision with a signal colour is covered by glyph and word, and noted in the kit. |
| 16 | **Name and logo need their own licence line.** GPL on a logo lets anyone alter and reuse it. Decide before the repo is public. | Raised, blocks publishing |
| 17 | **Honest note on the mark:** below about 24 px it can read as a yin-yang symbol. The small cut helps. Worth knowing before it becomes a favicon everywhere. | Raised |
| 18 | **Geologica's weights are non-standard** (250 = regular). A remapped build of the font (so 400 means regular) would remove a class of mistakes in third-party components. | Raised |
| 19 | **Font subsetting build.** The bundled subsets lack arrows and the check mark. The app build should subset the upstream fonts itself and keep them. | Raised |

## 2026-11-20, second pass (green accent)

| # | Idea | Status |
|---|---|---|
| 20 | **Success is always Demido green**, even when the accent is something else. Keeps "good news is green" stable, and with the default accent avoids two almost-equal greens. | Done |
| 21 | **Lightness guard on any accent.** A wallpaper can produce a near-black or a very pale accent. Demido keeps the hue and strength and moves only the lightness into a readable band, so the "awake" mark never disappears. | Done |
| 22 | **"In use now" row** next to the Accent setting: shows the colour actually in use on each theme and why (system, wallpaper, yours, default). Nothing hidden. | Done in the kit |
| 23 | ~~Automatic follows only a wallpaper-derived accent.~~ I proposed this so Demido would not turn blue on most Windows and macOS machines. | Declined by you: Automatic follows any system accent. The separate "System accent" choice was then redundant and is removed. |
| 24 | **Your wallpaper accent today is a pink (`#e6a5d9`)**, close to the magenta I first proposed. On your machine, Automatic will show that pink, not green. Worth knowing before you judge the green in the app. | Raised |
| 25 | **The loading eye uses a travelling line, not a spin.** Same pace and look as the task circle. Its line ends are flat, not round, at large sizes; invisible at 16 px. A stroked SVG would give round ends if that matters later. | Done |

## 2026-11-20, interactive preview

| # | Idea | Status |
|---|---|---|
| 26 | **A "Now" panel**, one press from anywhere: memory in use, which model is awake, what is running. The eye in the corner shows the state at a glance. | Done in the preview |
| 27 | **A session says what it has read.** "Clean" until outside content enters; then "Outside content: web". Makes the injection defence something you can see. | Done in the preview |
| 28 | **Tools show their cost.** The Tools list shows what each tool adds to every conversation, and the total. A classic MCP server at 1,240 tokens next to a converted one at 12 makes the case for converting by itself. | Done in the preview |
| 29 | **Schedules are written as a sentence**, then shown back as "Understood as" before you confirm. | Done in the preview (scripted parser) |
| 30 | **Candles without red and green.** Hollow rises, solid falls. Keeps the "colour is a signal" rule and works for red-green colour blindness. Traders may expect colour; it could be a setting. | Changed by the owner (2026-11-20): green and red is the default; this is now the "Hollow and solid" option |
| 31 | **Google mail through an app password**, not "Sign in with Google", because full mail access through Google sign-in needs Google to verify the app. Stated in the add-account dialog. Needs checking before it is built. | Raised |
| 32 | **Voice licences shown per voice** ("may be used commercially" or "personal use only") before download. | Done in the preview |
| 33 | **In a narrow window**, pressing the rail icon of the screen you are on brings its list back over the content. | Done in the preview |
| 34 | **Fourteen areas is a lot for one rail.** It works at 900 px tall; below about 760 px it scrolls. If it feels heavy, the four "connect" areas (Browser, Email, Messages, Markets) could fold into one. | Raised |

## 2026-11-20, preview changes: voice mode and markets

| # | Idea | Status |
|---|---|---|
| 35 | **The Demido symbol speaks.** My first version used a plain oval (the eye). The owner asked for the logo. The symbol swells a little, its two eyes a lot, so the eyes read as the living part. | Done in the preview |
| 36 | **"Answer now".** A key (Enter) for the moment Demido waits and you are in fact finished. The detector will sometimes be wrong; this is the way out. | Done in the preview |
| 37 | **A cut answer shows where it stopped.** "Stopped here" marks the last word spoken. The unsaid rest is dropped, so the model does not believe it told you something you never heard. | Done in the preview (simulated) |
| 38 | **The transcript notes pauses and interruptions**, not only words: "Silence. Your sentence sounds unfinished, so Demido waits." You can see why it did what it did. | Done in the preview |
| 39 | **A "Behaviour" menu inside voice mode** flips the voice settings without leaving, so the two ways of answering can be compared in a minute. | Done in the preview |
| 40 | **No ceiling on waiting.** You asked that Demido never talks into a thinking pause, so it waits for as long as it takes. An optional quiet nudge on the screen after a long silence (never spoken) could be added. | Raised |
| 41 | **Small sounds should not stop Demido.** "Mm-hm", a cough, a door. Stopping should need a real word or about a third of a second of speech. Otherwise it will stop all the time. | Raised; matters for the real build |
| 42 | **What happens to a running task when you interrupt.** The preview cancels it. I think the better rule is: speech stops at once, the task keeps running unless you say stop. | Raised; worth your opinion |
| 43 | **Voice mode should belong to a session.** The talk is saved as a session and can be continued by typing. In the preview it opens from Voice and from the message box but is not saved. | Raised |
| 44 | **A model's indicator work is shown step by step**: wrote it, test run failed on line 5, fixed it, test run passed. The failure is not hidden. | Done in the preview (scripted) |
| 45 | **Every indicator has a row on the chart** with its value, hide and remove. Pine scripts carry a "Pine · local" badge. | Done in the preview |
| 46 | **A compatibility report for Pine scripts**: which functions a script uses that Demido's engine does not have yet, before you rely on the result. | Raised |
| 47 | **"Bring my setup over" as a short guided list** (built-ins, your scripts, two colours) would make the allowed, by-hand route take a few minutes once. | Raised |

## 2026-11-20, preview changes, second round

| # | Idea | Status |
|---|---|---|
| 48 | **Past the plan's limit, Demido takes the indicator itself**, without asking, and says so once. Only a closed-source script is refused, with the reason. (First built as a button, "Work it out here instead"; made automatic on the owner's word.) | Done in the preview |
| 49 | **Signing out keeps what Demido can do itself.** Indicators it has its own maths for stay, worked out locally; the rest go. | Done in the preview |
| 50 | **Use a second TradingView account.** The warning could suggest connecting an account you can afford to lose, not the one with your paid plan and followers. | Raised |
| 51 | **A pin knows what it points at.** An annotation pin records the element under it ("button Add to basket"), so a model that cannot see images still gets something. | Done in the preview |
| 52 | **Models could annotate too**: mark what they are about to click, or what they mean, in their own colour. | Raised |
| 53 | **A drawn ad appears when the blocker is paused**, so the switch visibly does something in the preview. | Done in the preview |
| 54 | **Every tool file says who reads it**: models always, models after picking the tool, people, or Demido. That is what an edit costs. | Done in the preview |
| 55 | **The model card says what the model is bad at on this computer**, not only what it is good at. | Done in the preview |
| 56 | **A report dialog for chats** can become a schedule ("every morning"). | Raised |
| 57 | **A human pace.** When a model sends through your account, it should send like a person: one message, a pause, never many alike. Good manners, and the thing the services watch for. | Raised; matters for the real build |
| 58 | **Connected-only navigation.** Sixteen places is a lot. A service you never connect could stay out of the navigation until you do, and any place could be hidden. | Raised; worth your opinion |
| 59 | **`@` for more than files**: sessions, tools, people. | Raised |
| 60 | **Per-part updates need a quiet moment**: the model runtime can only be replaced when no model is loaded. | Raised |
| 61 | **The model types, you send.** In the panel beside a chat, "write that I am on my way" puts the words into the message box and says so. Nothing leaves until you press Send. | Done in the preview |
| 62 | **"Finish my reply".** Once you have typed something, the Draft button changes its name and continues from your words instead of replacing them. | Done in the preview |
| 63 | **Only your part is rewritten.** In a reply or a forward, Draft and Improve touch the text above the quoted message and leave the quote alone. | Done in the preview |
| 64 | **Undo a send.** A sent mail can be taken back for a few seconds, and the writing window comes back with it. The real one has to hold the message for that long before it leaves. | Done in the preview |
| 65 | **Tones could be yours.** Save an instruction you use often ("short, no greeting, sign with J.") as a tone of your own. | Raised |
| 66 | **One ask panel everywhere.** The panel beside a chat and the one beside a mail are the same part. Files, a web page in the browser and a chart in Markets could get it too: ask about the thing in front of you without leaving it. | Raised |
| 67 | **Images per sender.** Between "ask each time" and "always": "always for this sender". Most of the benefit, little of the tracking. | Raised |
| 68 | **"Find it in the chat".** With media, links and files listed, the model could be asked "find the PDF she sent in October" and open it. | Raised |

## 2026-11-21, the core

| # | Idea | Status |
|---|---|---|
| 69 | **A setting says whether it is built.** The core's registry carries a "built" mark on every setting, so "No silent stubs" holds in the command line and the API too, not only on screen. | Done |
| 70 | **Ask the driver, do not guess.** Which CUDA build to download is decided by asking the NVIDIA driver what it supports, not by a table of driver versions. | Done |
| 71 | **Built-in graphics are shown honestly.** Intel's and Apple's graphics share the computer's memory; the system reports that as "their" memory. Demido shows "shares the computer's memory" instead of a misleading number. | Done |
| 72 | **`demido open`** gets a browser in without the key ever appearing in an address: a link that works once, for a minute. | Done |
| 73 | **Stable means llama.cpp's stable.** llama.cpp now marks a stable release that points at one of its per-commit builds. Demido should follow that pointer by default and let the user pick a newer build by hand. | Raised; for the runtime installer |
| 74 | **Proof of where a build came from.** GitHub can attach a signed statement to every release file saying which commit and which workflow built it; anyone can check it with one command. Free for a public repository. Not added yet: it can only be tried on a real release. | Raised |
| 75 | **Keep the pinned actions fresh.** The workflows name exact commits, which is safe and also goes stale. A bot can open a change when an action has a new version. It adds noise, so it waits for a yes. | Raised |
| 76 | **A rehearsal before every release.** The release workflow can be started by hand: it builds all six archives and publishes nothing. Worth doing before a tag, because a tag that fails to build is public. | Done |
| 77 | **Stay signed in across a restart.** A browser's session lives in the server's memory, so restarting the server signs every browser out and `demido open` is needed again. Keeping a fingerprint of each session in the database would fix it. | Done on 2026-11-21: a fingerprint of each session is kept beside the key, for thirty days |
| 78 | **Follow the accent while the window is in front.** The portal announces a change of accent. Listening for it needs a lasting D-Bus connection; today the window asks again when it gets the focus back, which covers changing it in the system's settings. | Raised |
| 79 | **Check a runtime against its publisher's signed build statement.** llama.cpp's releases link to GitHub attestations. Verifying one would prove the file was built by llama.cpp's own workflow, which a checksum from the same page cannot. | Raised |
| 80 | **A setting for a mirror.** Where GitHub is slow or blocked, people need another place to fetch the runtime from. It decides which program gets installed, so it needs a warning and probably a pinned checksum list. | Raised |
| 81 | **Say before downloading that NVIDIA's libraries are 424 MB.** The offer shows the size and whether this system seems to have them already, and leaves them out when it does. | Done |
| 82 | **The server stops when asked, even with a window open.** A window keeps a live connection to the server; that connection kept the server from stopping at all. Every open stream now ends when the server is asked to stop, a download under way is stopped cleanly, and nothing gets more than five seconds. Found while testing, not reported by anyone. | Done |
| 83 | **Try an installed runtime again without downloading it.** After a driver update, or when a build that did not start might start now. `demido runtime check`, and "Try again" on the Setup screen. | Done |
| 84 | **Send the UI's files compressed.** The script is 800 kB and a quarter of that compressed. On the same computer it makes no difference; from another device it will. Best done once, when the UI is built, not on every request. | Raised |
| 85 | **Do not leave a runtime behind in the first place.** Today a server that is killed leaves the runtime running, and the next start ends it. On Linux the runtime could be told to end when its parent dies; that needs a little low-level code. Until a server is started again, the memory stays taken. | Raised |
| 86 | **A models folder of your own choosing.** Models are large; people keep them on another disk, or share them with other programs. Today the folder is fixed inside Demido's data folder. | Raised |
| 87 | **Speed for free.** How fast a model answers on this computer does not need a benchmark: every real answer comes with its own timing. Demido keeps a running figure per model from real use, and the roster reads it. | Raised; comes with the chat loop |
| 88 | **A big one that plans, small ones that do.** The clearest win for "a model per agent" is a strong model (a large local one, or a provider's) as the orchestrator with small fast local models as workers. A session could have that as a one-press setup: "plan with X, do with Y". | Raised |
| 89 | **Say what a turn cost, under every answer.** Tokens written, tokens a second, and how much of the prompt was read and how much was not read again. It is how a person sees that caching works, and when a conversation is getting expensive. | Done |
| 90 | **One conversation, every window.** Because the core runs the turn and tells it as news, an answer started in a terminal is written in the open window at the same moment. | Done |
| 91 | **"Answer now."** A small model can think for a thousand tokens about a greeting. While it thinks, one press ends the thought and makes it answer with what it has. The runtime has the control for it; the voice mode in the preview had the same idea. | Done |
| 92 | **A limit on thinking, set once.** Besides the button: a number of tokens after which a thought is ended by itself. The runtime takes such a budget only when it starts, not per question, so Demido would count and end the thought itself. | Raised |
| 93 | **Show what the model still knows.** When a conversation is made shorter, the window does not hide it: the older messages stay, dimmed, and a note at the cut prints the account the model now has in their place. A person can read it and see what was lost before it matters. | Done |
| 94 | **Check an account before trusting it.** The smaller test model still answers the conversation, in place of writing notes on it, about once in three. A cheap check is possible without a second model: every number and every capitalised name in the older messages should appear in the account; if many are missing, ask once more, and if it is still poor, keep more messages word for word and say so. | Raised |
| 95 | **Let a person mend the account.** The account is plain text the model reads. A person who sees a wrong or missing fact in it could correct it in place, and the model would know from the next turn. | Raised |
| 96 | **Pin a message.** "Never lose this one": a pinned message stays word for word through every shortening. Useful for a specification or a list of rules given at the start. | Raised |
| 97 | **A stronger model writes the account.** With a model for each agent (DECISIONS O), the account could be written by the most careful model that is loaded, not by the session's own. It is rare work where care pays more than speed. | Raised; with item 7 |
| 98 | **Try each model once, and word things for it.** No wording of the tool question was right for both families of models tried: one over-uses tools, the other under-uses them. The first time a model is used, Demido could run a short fixed trial (twenty requests, a few seconds on a graphics card), keep the wording that suits that model, and show the person the score: "this model uses tools right 21 times in 26". It would also tell the roster (DECISIONS O) which models can be trusted with tools at all. | **Set aside by the owner for later** (2026-11-21); written out in `docs/LATER.md` |
| 99 | **Show the plan before it is walked.** With Plan first, the list appears and is carried out at once. A person could be shown the steps first, to cross one out, reword one or add one, and then press Go. Cheap to build on what is there, and it puts the person where a small model is weakest. | Raised |
| 100 | **Do a failed task again.** A task that failed, or gave something wrong, could be done again by itself, with a word from the person about what was wrong, without redoing the ones before it. | Raised |
| 101 | **The date is a setting of the conversation, not of the computer.** Demido tells the model the day in this computer's time zone. In server mode the person may be somewhere else. The window knows its own time zone and could say so. | Raised; with server mode |
| 102 | **End a thought that goes round.** An answer that repeats itself is stopped. A thought can go round in the same way, and today only "Answer now" ends it. The same check could press that button. | Raised |
| 103 | **A cheaper model for the small questions.** "Does this need a tool?" and "what did this task give?" are short, mechanical questions. With a model for each job (DECISIONS O), the smallest loaded model could answer them while the session's model keeps its memory of the conversation for the answer. Not as simple as it sounds: the small model would have to read the conversation too. | Raised; with item 7 |
| 104 | **Show what a change changed.** Undo is there; what is missing is seeing the change: the lines taken out and put in, side by side, in the answer that made it, before deciding whether to keep it. | Raised |
| 105 | **Let a program's changes be looked at before they land.** When programs can run, a command can do anything inside the folder, and one press of Undo cannot cover it. The sandbox can run the command on a copy-on-write view of the folder: afterwards Demido lists what it changed, and Balanced lets through what only adds or edits and asks for what deletes. This computer's bubblewrap can do it; older ones cannot. | Raised; with running programs |
| 106 | **Say when an answer came from nowhere.** Small models describe files they did not read. A line under the answer now says when no file was read or changed. It could go further: a session could refuse to show such an answer as an answer at all when the request named a file. | Done in its plain form |
| 107 | **A model that is too small for files.** The 0.6B test model got 2 of 9 file requests right and made up the rest. A session could warn, when a folder is given to a model that is known to be this weak (LATER 1 would know), that it will mostly invent. | Raised |
| 108 | **Pick the folder, do not type it.** A path typed into a field is right for a server somewhere else. On your own computer a folder picker is what you expect. It needs the desktop shell (plan item 10). | Raised |
| 109 | **What need not ask again, seen and taken back.** "Yes, for this session" is remembered until Demido stops. It is shown in the folder panel; it cannot yet be taken back one tool at a time, and it is not kept across a restart. | Raised |

## Pushback on the brief so far

- **Logo checkpoint.** I started exploring new logo concepts before learning a logo existed, and stopped that
  work when you pointed to it. Nothing from it is in the kit.
- **Research order.** The brief asks for stack research before choosing. The research run stalled when the
  usage limit hit; four of ten areas finished and none was fact-checked. The brand kit did not depend on it,
  but the preview does. See PLAN.
- **Workflows.** You asked me to stop using multi-agent workflows mid-way. The rest of the kit was built
  directly. I will keep working that way unless you say otherwise.
- **Reading the TradingView account (DECISIONS M6).** I recommend against building it. TradingView's terms
  forbid reading an account by program, whatever the purpose, and they ban accounts for it. I read that on
  their Help Center myself. The preview shows the feature with a made-up account so you can judge it, marked
  "Not built". The by-hand route is allowed and is what I would build.
- **"As many indicators as you want without premium."** True for Demido's own chart, because Demido does the
  work itself. It must not become a way to lift TradingView's limits inside TradingView.
- **Pine Script will never match exactly.** Any local engine imitates TradingView's. Some scripts will behave
  differently or not run. The UI should say so every time, and it does in the preview.
- **Green and red.** Done as you asked. It is the pair that people with red-green colour blindness confuse
  most, so "Hollow and solid" stays one click away.
- **TradingView sign-in: you decided to build it.** Done as you asked: a warning, a tick box, then automatic.
  My view has not changed: it breaks TradingView's terms and can cost users their accounts. One thing I
  would still hold to: Demido should never publish or save to the account without asking. The limit question
  is settled: past the plan's limit Demido works the extra indicators out itself (DECISIONS M13).
- **Acting through personal chat accounts** is the same kind of risk, three times. Telegram allows it.
  WhatsApp and Discord do not, and Discord closes accounts for it. The preview warns before each. I would
  ship with "Drafts" as the default and keep "Replies on its own" narrow: only to people you chose.
- **A blocker that models can loosen is a door.** A page could write "turn off your ad blocker" hoping a
  model obeys. Page text is data and cannot give orders, but the setting "only after asking me" exists for
  anyone who wants the door shut.
- **Sixteen places in the navigation.** It fits a 900-pixel window now; it will not fit a smaller one without
  scrolling. See idea 58.
- **The services' logos.** I used plain icons with the names. Their real marks come with brand rules I have
  not read.
- **Discord's look.** You asked me to reproduce Discord's interface. I reproduced its arrangement (servers,
  channels, talk, people), which is how anyone knows where things are, and drew it with Demido's own parts.
  I did not copy Discord's colours, shapes or artwork; that would invite a complaint and would also look
  foreign inside Demido.
- **Angry on request.** "Angry" is in the list because you asked for it. I wrote it as firm, not rude. A
  model will write whatever tone it is asked for; a message sent in anger still carries your name.
- **The old repository's new name.** The brief says to rename it to `demido-studio-third`. That name was
  already taken by another repository of yours (private, from September). I asked instead of guessing.
- **"My GPG key".** You asked for commits signed with your GPG key. This computer has no GPG secret key; git
  here signs with an SSH key, which GitHub accepts as a signing key and shows as verified. I kept that and
  said so, instead of making a new key or changing your settings.
- **The old name may still be watched.** If earlier versions of Demido look at `elpideus/demido-studio` for
  updates, they now look at this project. I cannot check, because I do not read the old code. It matters from
  the first release on, not before.
- **A model per agent: where I expect it to pay, and where not.** Three honest cautions. First, small models
  are poor judges of other models; I expect Demido's own rule (mechanical work goes to the fastest model
  that fits) to do most of the good, and the model's own choosing to add a little on top. That is why the rule
  comes first and the choosing is measured before it is on by default (DECISIONS O4, O8). Second, "quicker"
  is only true when the faster model is already loaded or the work is long enough: on your card a small
  model loads in under a second, a large one from disk takes much longer, and two loaded at once leave
  less room for the main one. Demido will not swap when the swap costs more than it saves (O5). Third, the
  moment providers exist, "another available model" can mean one that costs money and sends your work
  away. I decided that no model may assign such a one unless you allowed it, with a limit (O6). Say so if
  you want it looser.
- **"Vector reasoning": what I can and cannot give you.** You marked it experimental and asked what is
  feasible. Deciding by meaning with embeddings (which tools, which memory, which model) is real and I will
  build it. Steering a model with a control vector is possible in the runtime; I have no measurement that
  it beats a good prompt, so it gets one honest trial. Making an ordinary downloaded model reason in vectors
  and not in words is not possible: it needs models trained for it, and there are none in the formats the
  runtime loads. Research note 13 has the detail. If you meant something else by the term, tell me.
  **Answered on 2026-11-21:** you meant the last of the three, and withdrew it. Nothing is built under
  that name; the control-vector trial is dropped too.
- **Forcing a model's output is not the cure I assumed.** I planned to force every tool call into its
  schema. Recent papers say that makes small models' calls valid and often wrong, and my own measurement
  showed the real failure is elsewhere: tiny models do not know when to leave the tools alone. One change of
  shape fixed that completely (research note 13). I changed the plan to match the measurement.
- **A task list does not make a small model plan.** The brief says the task list is "really important for
  smaller models", and after measuring I agree about the list and disagree about who writes it. Small
  models do not decide to plan (0 of 12 tries on the three smallest), and cannot say when a plan is needed.
  What works is the other half: Demido keeps the list and asks for one task at a time. So a plan starts
  when you press Plan first, not when the model feels like it. An "Always" setting exists and is off,
  because the models then make lists for requests that need none.
- **One prompt does not fit all small models.** I looked for the one wording that makes every small model
  use tools properly. There is none: the two families I tried fail in opposite directions on the same
  words. The fix is to choose words per model by measuring it, which is brief 4.2 ("model-specific
  optimizations") reaching further than temperature settings. I built the default that errs towards doing
  nothing, and wrote the rest down as the next thing worth building for small models (IDEAS 98).
- **"Outside content is data" cannot be said to a small model.** The preview has a switch called "Treat
  outside content as data". I measured what saying that to a model does: nothing. Four of four models
  obeyed a file that told them what to reply, with or without being told the file was only data. What
  works is taking away what an obedient model could do with it: one folder, undo, no network, and
  decisions about asking made in code. I built those, and I would rename that switch when the rest of the
  defence exists, so that it does not promise what words cannot deliver.
- **Smart is not smart yet.** You asked for a level where the model decides when to ask. With files only,
  there is nothing for it to decide, so today it is Balanced under another name, and the window says so. I
  did not want to ship a second-look model that a four-billion-parameter model would be bad at, for
  decisions that do not exist yet.
- **Small is not all the same.** The design is tuned on 0.6B to 4B models because they are quick to
  download and fail loudly. On your card you would run 4B to 12B. The 4B model got 25 of 26 decisions and
  7 of 8 real requests right; the ones below it are where the work is. I have not measured anything
  between 4B and 12B.

