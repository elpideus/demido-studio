# 10. Messaging accounts, the old TradingView feature, and an ad blocker

Written 2026-11-20, directly. Three things the owner asked for in the second round of preview changes.

**Read** means I opened the page or file myself. **Search** means search results and their summaries only;
check before relying on it.

## 1. Acting through a person's own chat account

Asked: let a model handle the user's own WhatsApp, Telegram and Discord chats (read, mark as seen, reply,
report), after a warning.

| Service | What its rules say | Checked |
|---|---|---|
| WhatsApp | There is no official interface for a personal account; the official one is for businesses. The messaging guidelines list unofficial clients, auto-messaging and automation among prohibited conduct (the published wording ties it to harm to WhatsApp or its users). The terms let WhatsApp disable or suspend an account. Enforcement is reported as uneven. | Search (WhatsApp Messaging Guidelines, Terms of Service) |
| Telegram | Other apps may use a user account through Telegram's own interface, with an `api_id` the developer gets for their app. The app must say it uses the Telegram API and must not break Telegram's basic functions. The results did not find a rule about automating one's own account as such. Accounts that behave like spam are limited. | Search (Telegram API Terms of Service) |
| Discord | Automating a normal user account ("self-bot") outside the bot interface is forbidden and can end in the account being closed. Discord's safety pages add that using a user token in any application may lead to suspension. | Search (Discord support article "Automated user accounts (self-bots)", Discord safety pages) |

Risk, from lowest to highest: Telegram, WhatsApp, Discord.

**Bots.** Telegram's Bot API and Discord's bot accounts are the services' own, allowed way. A bot cannot read
the user's other chats; it lets the user talk to Demido from the chat app. WhatsApp has no equivalent for a
personal account.

**Not researched yet:** which libraries would do the work, and their licences (unofficial WhatsApp clients,
Telegram's TDLib or an MTProto client, Discord). Message length limits used in the preview (4,096 characters
on Telegram, 2,000 on Discord) are from memory and need checking.

## 2. What the previous Demido Studio did with TradingView

The owner allowed one look at the old code, for this feature only. I listed the old project's files that
mention TradingView and read these, and nothing else: the indicator catalogue, the indicator runner, the
sign-in module, and the first lines of the saved-indicators, indicator-hook and Pine modules.

What it did:

- **Sign-in.** A separate window showed TradingView's own sign-in page, in a browser profile of its own.
  When TradingView set its session cookies, Demido read them, checked them and stored them in the system's
  credential store. The password was never typed into Demido.
- **The Indicators menu** was read with that session, the way TradingView's own menu reads it: favourites,
  the user's own scripts, TradingView's built-ins, community scripts (by search), and the indicators saved in
  the user's chart layouts.
- **TradingView worked the indicators out.** Each indicator ran as a "study" on a chart session at
  TradingView, so the values were TradingView's own. The plan's limit of indicators per chart applied, and
  the old code told the user so in plain words. Without a session, the classics were computed locally.
- **Pine scripts written in Demido** were kept on the computer. TradingView compiled them without saving
  anything to the account, and they ran like any other indicator. A "trial" ran a script once and returned
  compile and runtime errors, so a model could fix it. Saving a script to the account was a separate step
  that asked first.
- It used an unofficial client library for TradingView's connection (listed in the old notices as ISC
  licensed; not checked again now), Lightweight Charts for drawing, and a separate source for history.

All of this reads the account by program, which TradingView's terms forbid (research note 09). The owner has
decided to offer it behind a warning.

**What I did not read:** what the old version did once TradingView refused an indicator for the plan's limit,
beyond telling the user. The owner remembers running more than three indicators on a free account. I cannot
say from what I read whether TradingView worked those out (its limit through that connection may differ from
the one its website shows) or whether the extras were computed locally. The new rule does not depend on the
answer: past the limit, Demido works them out itself (DECISIONS M13).

## 3. An ad blocker for the built-in browser

| Candidate | What it is | Licence | Checked |
|---|---|---|---|
| adblock-rust (Brave) | A library in Rust. Reads the usual filter lists and answers "block this request?" and "hide these elements". | MPL-2.0 | Search (package registries, Debian's copyright file) |
| uBlock Origin, the extension itself | The full blocker, as a browser extension. | GPL-3.0 | Search (Debian's copyright file; not the upstream licence file) |
| Filter lists: EasyList, EasyPrivacy, uBlock filters, Peter Lowe's list | Data, kept by their communities. | EasyList: GPL-3.0-or-later or CC BY-SA 3.0-or-later, as labelled in Debian's copy. Others not checked. | Search |

**Recommendation, not yet a decision:** adblock-rust inside Demido's core.

- Both licences fit GPL-3.0-or-later. That is not what separates them.
- A library in the core works the same in the window, on a server without a screen, and when a model drives
  the browser. An extension depends on which browser engine Demido ends up with and on that engine still
  accepting the extension.
- The core can then offer the blocker to models as a tool (see what was blocked, allow one domain on one
  site, pause on one site), which an extension would not let it do cleanly.
- The lists are downloaded from the people who keep them, like every other part. Nothing is redistributed.

## Sources

- Discord, "Automated user accounts (self-bots)": <https://support.discord.com/hc/articles/115002192352>
- Discord, platform manipulation policy: <https://discord.com/safety/platform-manipulation-policy-explainer>
- WhatsApp, Messaging Guidelines: <https://www.whatsapp.com/legal/messaging-guidelines?lang=en>
- WhatsApp, Terms of Service: <https://www.whatsapp.com/legal/terms-of-service-uk>
- Telegram, API Terms of Service: <https://core.telegram.org/api/terms>
- Telegram, Bot Platform Developer Terms: <https://telegram.org/tos/bot-developers>
- adblock-rust: <https://github.com/brave/adblock-rust>
- Debian, copyright file of uBlock Origin: <https://metadata.ftp-master.debian.org/changelogs/main/u/ublock-origin/stable_copyright>
