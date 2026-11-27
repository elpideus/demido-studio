# 37. The person's own WhatsApp, Telegram and Discord accounts

Written 2026-11-26, directly, after the owner's direction that morning: "Build whatsapp, telegram and discord, and
allow for user account on discord and telegram, not just bots." Bots for Telegram and Discord exist (DECISIONS G12
to G15). This note is about linking the person's own accounts, which G2 decided on 2026-11-20 (behind a warning
and a tick box) and G16 left unbuilt, and about WhatsApp, which has no bots.

How sure each line is: **Read** (crates.io through `cargo info`, the crate's own source, or the page named, on
26 November 2026); **Read (unofficial)** (community documentation of what a service does not document); **Not
tried** (nothing here was tried against a real service: each needs the owner's phone or account).

## The libraries (Read)

| Part | Crate | Version | Licence | Note |
|---|---|---|---|---|
| Telegram as the person (MTProto) | `grammers-client`, `-session`, `-mtsender` | 0.12.0 | MIT OR Apache-2.0 | Repository `codeberg.org/Lonami/grammers`. Sign-in: `request_login_code(phone, api_hash)`, `sign_in(token, code)`, and `check_password` when the account has a password (`SignInError::PasswordRequired`). Its session is a small trait (`Session`: the home data centre, each data centre's address and authorization key, the peers it knows, the update state); the default `sqlite-storage` brings `libsql`, so Demido takes `default-features = false` and keeps the session itself. With the `serde` feature its types serialize |
| WhatsApp as the person (linked device) | `whatsapp-rust` (with `wacore`) | 0.7.0 | MIT | A Rust port of whatsmeow (Go), its own docs say. Rust 1.94 or later, stable (the nightly-only SIMD of its 0.1 line is gone). Linking by QR code or by a code typed on the phone ("Link with phone number instead"). Its store is a trait of some 80 methods (Signal's keys and sessions, app state, devices); the store it ships is SQLite through Diesel with `libsqlite3-sys` 0.37 |
| Discord as the person | none taken | | | Discord's own documentation is for bots; a person's account speaks the same REST API and Gateway with its token (without `Bot `). Demido's bot client (G12) is most of it |

**The clash, measured** (`cargo tree` in a scratch project with both crates): Cargo let only `rusqlite` 0.39 sit
beside `whatsapp-rust`'s store, because only one `libsqlite3-sys` may be linked into a program (`links =
"sqlite3"`), and Demido's core is on `rusqlite` 0.43. So either Demido's own database library goes back four
versions and stays tied to an unofficial client's, or the core writes WhatsApp's 80-method store itself (Signal's
sessions and keys: a mistake there breaks every conversation), or WhatsApp runs in **a helper program of its own**,
built in its own Cargo workspace, beside the core. The helper is what G7 already proposed for whatsmeow and
Baileys, and it keeps an unofficial protocol's code out of the core's process.

## What each needs from the person (Read)

**Telegram.** An `api_id` and `api_hash` (API terms 2.1: "You must obtain your own api_id for your application",
at my.telegram.org, which signs in with a phone number). Then the phone number, the code Telegram sends to the
person's Telegram app, and the account's password if it has one. Telegram's test servers sign in numbers
`99966XYYYY` with the code `XXXXX` (X the data centre, 1 to 3; Telethon's and Pyrogram's documentation agree), but
still need an `api_id`: Demido has none, and using another app's is what 2.1 forbids. So a real sign-in is **not
tried** here.

**Discord.** The account's token. Discord offers no way for another program to get one: the person copies it from
their own browser, signed in to discord.com (the developer tools, the `Authorization` header of a request to
`/api`; from memory, as is the next sentence). Discord's own sign-in by QR code (the phone app scans a code the
desktop shows) gives a token too, but is undocumented and sometimes asks for a captcha; not taken. A Discord token is the whole account: it is kept in the
vault (U) like a key.

**WhatsApp.** The person's phone with WhatsApp on it: Settings, Linked devices, and either the QR code Demido
shows, or "Link with phone number instead" and the eight-letter code Demido shows. How long WhatsApp keeps a linked device
whose phone is away is from memory (about 14 days), **not read** here.

## The services' rules (Read; as in note 32, and what changed)

- **Telegram, API terms**: allowed for a client that keeps them. 1.4: no actions "without the user's knowledge and
  consent", no "tampering with the 'read' statuses". 1.5: data from Telegram may not be used to "train, fine-tune
  or otherwise engage in the development, enhancement or deployment of artificial intelligence". Demido trains
  nothing; whether a person's own model reading their chats for them is "deployment" is not plain. The owner
  asked for it to be built; that reading stays theirs, and the warning says it. 2.3: an app's name must not
  include "Telegram" unless preceded by "Unofficial" (Demido's is Demido Studio).
- **Discord**: "Automating normal user accounts (generally called "self-bots") outside of the OAuth2/bot API is
  forbidden, and can result in an account termination if found." The owner decided on it behind a warning (G2)
  and asked again on 2026-11-26.
- **WhatsApp**: its terms forbid using its services through unofficial means ("reverse engineer", "automated
  means"); a linked device that is not WhatsApp's own is that. The risk is a ban.

## Discord as the person, what is needed (Read (unofficial): discord-userdoccers.vercel.app, its Gateway pages)

- Identify (op 2) with `token`, `properties` (`os`, `browser`, `device`) and `capabilities`, a bitfield of
  behaviours; a person's account needs no intents ("do not require intents"). With no capabilities asked, READY
  gives the old shapes: `private_channels` with their `recipients`, `guilds` with `name` and `channels` on
  them; `DEDUPE_USER_OBJECTS` (16) gives `recipient_ids` and a list of `users` instead, and `CLIENT_STATE_V2`
  (1024) puts a server's name under `properties`. Demido asks for none, and reads both shapes.
- History: `GET /channels/{id}/messages?limit=50`; sending: `POST /channels/{id}/messages` (as for bots).
- `MESSAGE_CREATE` comes for the account's own messages too (sent from its phone): kept as its own.

## What follows (Proposed, then decided in G17 to G22)

1. One place in the core for the person's own accounts on the three services ("linked accounts"): each account,
   its chats, and their messages kept in the database; the secret of each in the vault; the window reads and
   sends through it, and the core is the only thing that speaks to the service.
2. Telegram through `grammers` 0.12 in the core, its session kept in the vault; the person's own api_id and
   api_hash. Discord through Demido's own client (the bot client's Gateway and REST, with a person's token).
   WhatsApp through a helper program, `demido-whatsapp`, on `whatsapp-rust` 0.7, in its own workspace, speaking
   to the core in lines of JSON on its standard input and output, its store in Demido's folder.
3. Linking only after the warning with the service's facts and a tick box (G2), and the person types every code
   and password; Demido never asks for or keeps the phone's PIN.
4. What a model may do with these accounts (G3) comes after the window: reading first.
5. Tried against stand-ins of each service on this computer (a Discord REST and Gateway stand-in, a Telegram
   client stand-in at the library's edge, a helper stand-in); a real account is the owner's to link.
