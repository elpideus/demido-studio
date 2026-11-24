# 32. Messaging: the libraries, the services' terms, and what each risks

Written 2026-11-24, directly, finishing what DECISIONS G7 started (plan item 9). The owner decided on 2026-11-20
that Demido acts through the person's own WhatsApp, Telegram and Discord accounts behind a warning (G2), and
through bots for Telegram and Discord (G4). Linking an account needs the owner's phone, so here: what can be built
without it, and what each way risks.

How sure each line is: **Read** (the service's own page, or crates.io, on 24 November 2026, address given).
Nothing here was tried against a real service.

## The libraries (Read, crates.io)

| Part | Crate or program | Version | Licence | Note |
|---|---|---|---|---|
| Telegram, as the person (MTProto) | `grammers-client` | 0.12.0 | MIT OR Apache-2.0 | Its repository is now `codeberg.org/Lonami/grammers` (the GitHub one was archived: G7's open question) |
| Telegram, as the person | TDLib, through `tdlib-rs` | 1.4.0 | TDLib BSL-1.0; the crate MIT OR Apache-2.0 | C++ to build or a prebuilt library |
| Telegram bots | `teloxide` | 0.17.0 | MIT | Large; the Bot API is plain HTTPS and JSON |
| Discord bots | `serenity` / `twilight-gateway` | 0.12.5 / 0.18.0 | ISC | Large; the Gateway is a WebSocket and JSON |
| WhatsApp, as the person | whatsmeow (Go) / Baileys (JavaScript) | | MPL-2.0 / MIT | Unofficial; a helper program beside the core |

All fit GPL-3.0-or-later. **Taken for the bots: none of them.** A bot is a few documented calls: the Telegram Bot
API (`getUpdates` with long polling, `sendMessage`) and Discord's (the Gateway's Hello, Identify, Heartbeat and
`MESSAGE_CREATE`; `POST /channels/{id}/messages`). Demido's own thin clients over what is already in the build
(reqwest, tokio-tungstenite on tokio-rustls) are smaller than either library, and each call can be tried
against a stand-in.

## What the services say (Read)

**Telegram, API Terms of Service** (<https://core.telegram.org/api/terms>), for an app that is a client of a
person's account:
- 1.4: "It is forbidden to interfere with the basic functionality of Telegram. This includes … making actions on
  behalf of the user without the user's knowledge and consent, … tampering with the 'read' statuses of messages".
  So Demido acts only as the person set it (G3) and never hides or fakes "read".
- 1.5: "you are prohibited from using, accessing or aggregating data obtained from the Telegram platform to
  train, fine-tune or otherwise engage in the development, enhancement or deployment of artificial intelligence,
  machine learning models and similar technologies." Demido trains nothing. Whether a person's own model reading
  their own chats for them is "deployment" in Telegram's sense is not plain from the words: **the owner's to
  weigh** before linking an account is offered.
- 2.1: "You must obtain your own api_id for your application" (from my.telegram.org, with a phone number);
  2.3: the app's title must not include "Telegram" unless preceded by "Unofficial"; 2.4: not its logo.
- Bots are separate: a bot is made with @BotFather and has a token. Limits (<https://core.telegram.org/bots/faq>):
  "In a single chat, avoid sending more than one message per second"; groups 20 a minute; 429 beyond. Updates
  wait on the server for 24 hours (<https://core.telegram.org/bots/api>, `getUpdates`).

**Discord** (<https://support.discord.com/hc/en-us/articles/115002192352>): "Automating normal user accounts
(generally called "self-bots") outside of the OAuth2/bot API is forbidden, and can result in an account
termination if found." Its Developer Policy, 21: "Do not use message content obtained through the APIs to train
machine learning or AI models (including large language models) unless express permission is granted by
Discord." So: a bot is the allowed way; acting as the person (G2's Discord row) is exactly what the rule forbids,
and G2 already says so.

**WhatsApp, Terms of Service (EEA)** (<https://www.whatsapp.com/legal/terms-of-service-eea>): you must not
"directly or through automated means: (a) reverse engineer, alter, modify … our Services; … (c) gain or attempt
to gain unauthorized access to our Services or systems; … (h) distribute or make our Services available over a
network where they could be used by multiple devices at the same time, except as authorized through tools we have
expressly provided". An unofficial client (whatsmeow, Baileys) works by reverse-engineering the linked-device
protocol. The only allowed way for a program is the WhatsApp Business Platform, for businesses.

## What each risks, for the account

| Way | Allowed by the service | Risk |
|---|---|---|
| Telegram bot | Yes | None to the person's account: the bot is its own |
| Discord bot | Yes | None to the person's account |
| Telegram as the person | Yes, for clients that keep the API terms | The project needs an api_id of its own (a phone number); 1.5 to weigh |
| WhatsApp as the person | No (unofficial clients) | A ban, "for a time, then for good" (G2) |
| Discord as the person | No ("self-bots") | "account termination" |

## What follows (Proposed, then decided in G12 to G16)

1. Bots first, for Telegram and Discord, with Demido's own thin clients: the allowed way, with no phone.
2. Only the owner of a bot gives it orders (G4): the person pairs it with a code Demido shows (`/start <code>`
   on Telegram, a direct message with the code on Discord), and that account alone is answered; anyone else is
   told the bot answers only its owner, and nothing they write reaches a model.
3. A human pace when sending: one message a second in a chat, as Telegram asks.
4. The chats a bot has are kept in a store of Demido's own, which the WhatsApp, Telegram and Discord screens read.
5. Linking a person's own account waits for the owner: their phone, Telegram's api_id, and 1.5.
