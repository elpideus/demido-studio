# 31. The browser a model drives

Written 2026-11-24, directly, for plan item 8 (brief 4.25: "The model can interact with it in every way a normal
user could"; web fetch "falling back to the integrated browser when a website requires JavaScript"). What is
driven and how, what the model is shown, what it may do at each level, how a page is kept from giving orders,
and what can shut WebRTC (IDEAS 142).

How sure each line is: **Tried** (on this computer, 23 and 24 November 2026), **Read** (the maker's own pages or
files, with the address).

## Which browser

| Choice | What it is | Why or why not |
|---|---|---|
| **Chrome for Testing's `chrome-headless-shell`, stable** | Google's own builds for automation, per platform: `linux64`, `linux-arm64`, `mac-arm64`, `mac-x64`, `win32`, `win64`. Listed by version in JSON at `googlechromelabs.github.io/chrome-for-testing/` (`last-known-good-versions-with-downloads.json`), downloaded from `storage.googleapis.com/chrome-for-testing-public/<version>/<platform>/chrome-headless-shell-<platform>.zip`. Stable on 24 November 2026: 158.0.8127.88, 132 MB zipped, 296 MB unpacked (Read, Tried) | **Taken.** It follows Chrome's stable channel, with its security fixes, which matters for a program that opens any page on the web. It is the headless browser alone, without Chrome's sync, sign-in or extensions |
| Chromium's own snapshots (`chromium-browser-snapshots`) | Per-commit builds of the open-source Chromium | Not a release channel: a commit's build is not patched when a hole is found. No Linux arm64 |
| A browser installed on the system | The person's Chrome, Edge, Chromium | Not used: the owner's rule for programs (U26: "everything contained") and, more, a person's own browser is where their sign-ins are |
| The desktop shell's webview | Not built (plan item 10); it shows Demido's own window, and is not driven by a model | Not for this |
| Firefox through WebDriver BiDi | Possible | Chromium's debugging protocol is the one tried here (Q15's trials), and the one the headless shell speaks |

**Its terms (Read, from the archive itself).** Its `ABOUT` says "Google Chrome … Copyright 2026 Google LLC … See
the Terms of Service at chrome://terms", and `LICENSE.headless_shell` (43,602 lines) holds the BSD licence of
Chromium and the licences of the open-source parts in it. So it is Google's build, used under Google's terms by
the person who has it downloaded. Demido does not ship it, link it or change it: it is downloaded from Google
when the person asks, as the model runtime is (K8), and run as a separate program. The licence check of the
GPL (R12) covers what Demido links, which this is not.

**How it is checked.** Chrome for Testing lists no checksum. Google's storage says, with every file, its MD5 and
CRC32C (`x-goog-hash: md5=H3PoWE72ADi565OU5pDvNQ==` for the Linux shell, Read; the downloaded file's MD5 was the
same, Tried). That proves the file arrived whole as the storage holds it; that it is Google's rests on the
connection being to Google's own storage over HTTPS. Said so where it is downloaded.

## How it is driven (Tried)

Over Chromium's debugging protocol, **through a pipe** (`--remote-debugging-pipe`: messages as JSON, ended by a
NUL byte, on the program's file descriptors 3 and 4), never a port. A debugging port on `127.0.0.1` is open to
every program and every user of the computer, and whoever reaches it drives the browser. Tried with the headless
shell 158: `Browser.getVersion` answered in 0.05 s; `Target.createBrowserContext` gave a context of its own (its
own cookies and storage), and `Browser.close` ended it. On Windows the pipe is given as handles, not file
descriptors; not tried.

## What the model is shown

- **The page's text after its scripts ran**, read the way `read_page` reads a page (N9): the parts that match
  what was asked, 5,000 characters at most.
- **A numbered list of what can be pressed or filled**: links, buttons, fields, each with its words. A small
  model picks "3" far more reliably than a point on a picture, and a number can only be one of what is there.
- **Not a picture.** Sending pictures to a model is not built (U14), and none of the four test models reads
  them. The picture of the page is taken for the person, under the step.

## What it may do, and when it asks

The rule of section T and N holds: what protects is what the model cannot do, decided by code.

- **Opening an address** is `read_page`'s: an address given in the session opens unasked, one the model wrote
  itself asks (N2), only the public internet (N3), a site's `robots.txt` for the page itself (N10).
- **Pressing** something on the page does what the page offers, with nothing of the person's in it: the profile
  is empty. It does not ask, but at Cautious.
- **Typing** is the model's own words going into a page, which can carry what it has read elsewhere (the reason
  N2 asks for an address the model wrote). It asks at Balanced unless the words are the person's own, written in
  the session; at Smart the model decides (T6); at Autonomous it does not ask. A password field is never typed
  in: nothing of the person's is there to type, and a page that asks a model for a password is a trick.

## The network of the browser (Tried, IDEAS 142)

Everything the browser loads goes through a proxy of Demido's own, on this computer, which allows only the public
internet: the browser looks nothing up itself, the proxy looks the name up into public numbers only (N3's
resolver), for every request, a picture or a script as much as the page.

WebRTC can send what a page holds to any address, past every policy a page can be given (Q15). Measured with
`core/dev/page-frame-trial/run.sh` (a probe page that tries STUN over UDP and TURN over TCP towards a listener on
this computer), with Chromium's switches:

| Browser and switches | UDP (STUN) | TCP (TURN) |
|---|---|---|
| Chromium 157 (Arch's package), none | reached | reached |
| Chromium 157, `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` | reached | reached |
| Chromium 157, that and a proxy (`--proxy-server`, loopback not bypassed) | reached | stopped (it went to the proxy) |
| Chromium 157, `--disable-blink-features=RTCPeerConnection` | reached | reached |
| Chromium 157, `default_public_interface_only` | reached (the switch is read: the candidate became the public address) | reached |
| **Chrome for Testing headless shell 158, `disable_non_proxied_udp`** | **stopped** | reached (no proxy given) |

So in the headless shell, `disable_non_proxied_udp` shuts UDP, and TCP then goes only through the proxy. A TURN
connection through a proxy starts with a STUN message (a two-byte type, its length, and the magic cookie
`0x2112A442`), where an HTTPS one starts with a TLS record (`0x16 0x03`): the proxy refuses a tunnel whose first
bytes are STUN's. Why Chromium 157 as packaged here did not keep to the same switch was not found out.

For the window's own page frame (Q15): it runs in the person's browser, or later the desktop shell's webview.
Demido gives the person's browser no switches, so nothing shuts it there; the shell can (WebKitGTK has a setting
for WebRTC; WebView2 takes Chromium's switches). That is plan item 10's.

## How a page is kept from giving orders

As for every page (N5, T10, T11): its text is cleaned of turn markers and hidden characters, fenced, and the
person's request is said again after it. A session that used the browser has read the internet, so a change to a
file then asks (N4). Nothing a page says changes what is asked: that is decided by code from the tool and the
level. Said once, from the measurements: a small model does what the last thing it read tells it to (T11); what
keeps that harmless here is that pressing sends nothing of the person's and typing asks.

## Not done

Running a script of the model's own in a page, the network and console for the model, several tabs, the
person driving the browser from the window, pictures for a model that reads them, Windows. Logged-in pages are
the owner's to decide.
