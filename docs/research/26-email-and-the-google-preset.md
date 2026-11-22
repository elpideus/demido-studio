# 26. Email, and what the Google preset would take

Written 2026-11-22, directly. Brief 4.28 asks for email with a Google preset, IMAP and SMTP (POP3 not
required), and 4.27 for "my email report every morning at 7". Nothing here is built. The question that needs the
owner first is how Demido may sign in to a Gmail account, because one of the ways costs money every year.

How sure each line is: **Read** (from Google's own pages, or crates.io, on 22 November 2026), **Known** (to be
confirmed when it is built), **Proposed** (mine).

## How a program reaches Gmail (Read)

- **IMAP and SMTP sign in with OAuth (`XOAUTH2`)**, Google's page for it says, and the scope is
  `https://mail.google.com/`, the whole mailbox. "If your public application uses scopes that permit access to
  certain user data, it must complete a verification process", and an app that asks for this scope "must show
  full utilization" of it and comply with the Google API Services User Data Policy.
- **That scope is restricted.** Google's page on restricted scopes: such an app must be "a permitted application
  type" and pass "additional reviews, which include a possible security assessment"; "if the app accesses or has
  the capability to access Google user data from or through a server ... the system must undergo an annual
  security assessment from an independent, third-party assessor that is approved by Google"; the process "can
  potentially take several weeks". The assessors are paid by the app's maker.
- **App passwords** still exist: "a 16-digit passcode that gives a less secure app or device permission to
  access your Google Account", only with 2-Step Verification on, and not offered for an account that uses only
  security keys, for "a work, school, or another organization account", or with Advanced Protection. Google says
  they "aren't recommended and are unnecessary in most cases".

## What that means for Demido

1. **Demido's own sign-in with Google**, one app for everyone, means Google's verification of the project, and
   perhaps the yearly paid assessment. Whether it applies to Demido is not plain: its "server" is the core on the
   person's own computer, and nothing reaches a server of the project; Google's wording is about data "from or
   through a server", and it decides. **This costs money and weeks, and needs the owner.**
2. **An app password** costs nothing and needs no one's approval: the person makes one in their Google account
   and gives it to Demido, which keeps it in the vault (U) like a key. It does not work for work or school
   accounts, and Google discourages it.
3. **The person's own Google project** (Known): someone who makes their own OAuth client in Google Cloud and
   uses it only for their own account is not a public app. Google limits such projects while they are not
   verified (a "Testing" project's sign-in lasts days, not months, as I know it); to be confirmed before it is
   offered. It is the way for the technically minded, not the default.

**Decided by the owner on 2026-11-22 (DECISIONS Y1):** the Google preset starts with app passwords and says
plainly what they are and where they do not work. Demido's own verified sign-in is not applied for; whether it
is worth its cost is the owner's to decide, if ever.
Other providers (Fastmail, mailbox.org, Proton through its Bridge, any IMAP server) are plain IMAP and SMTP with a
password or an app password, and need none of this.

## The libraries (Read)

| Part | Crate | Licence |
|---|---|---|
| Reading mail (IMAP) | `async-imap` 0.12.0, on `imap-proto` 0.17.0 | MIT OR Apache-2.0 |
| Sending mail (SMTP) | `lettre` 0.11.24 | MIT |
| Reading messages (MIME) | `mail-parser` 1.0.1 | Apache-2.0 OR MIT |
| Writing messages | `mail-builder` 2.0.1 | Apache-2.0 OR MIT |

All compatible with GPL-3.0-or-later; the licence check (`scripts/check-licences.mjs`) is run when one is added.

## What email needs beyond the libraries (Proposed)

- **A message from outside is text from outside** (T10, T11, N4): cleaned, fenced, and after it the person's
  request said again. Measured already: a page that gives an order is obeyed by small models. In mail, the
  order comes addressed to the person, signed by anyone.
- **Sending cannot be undone, so it always waits for the person's yes**, at every level, with the message shown
  whole: who it goes to, what it says, what is attached. ~~A schedule (note 24) never sends mail; it writes a
  draft and tells the person.~~ Changed by the owner on 2026-11-22: a schedule runs with every tool and asks
  nothing (DECISIONS J2), so a schedule may send; its notice says what it sent, and to whom.
- **Reading is read-only at first**: no deleting, no moving, no marking as read, until those are asked for.
- **The morning report** (4.27) is then a schedule that reads, writes a summary into its session, and tells the
  person: everything in it is J2's "needs nobody's yes".
